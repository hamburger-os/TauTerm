//! XModem 协议实现
//!
//! 发送端独立选择 128B / 1K 数据块；接收端通过 NAK / `C` 协商
//! checksum / CRC16。两者是正交维度，`G` 不代表 XMODEM-1K。
//!
//! 基于 lrzsz-0.12.20 `wcs`/`wcrx`/`wcputsec`/`wcgetsec` 标准流程实现。
//!
//! XMODEM 仅支持单文件传输（无批次模式）。

use std::fs;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::transfer::config::XModemReceiveMode;
use crate::transfer::crc::{self, crc16_ccitt_feedthrough_verify, crc16_ccitt_zero_pad};
use crate::transfer::io::{self, read_byte_with_timeout, CAN};
use crate::transfer::protocol::SerialTransferProtocol;
use crate::transfer::types::{
    BatchFileResult, FileInfo, FileTransferEvent, TransferDirection, TransferProgress,
};

// ── XMODEM 协议常量 ──────────────────────────────────

const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const C: u8 = 0x43;

const BLOCK_SIZE_128: usize = 128;
const BLOCK_SIZE_1K: usize = 1024;
const MAX_RETRIES: u32 = 10;
/// 启动握手总超时时间（秒）
const INIT_TIMEOUT_SECS: u32 = 30;

// ── XMODEM 角色与校验模式 ───────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XModemCheckMode {
    Checksum,
    Crc16,
}

impl XModemCheckMode {
    const fn init_byte(self) -> u8 {
        match self {
            Self::Checksum => NAK,
            Self::Crc16 => C,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum XModemRole {
    Sender { block_size: usize },
    Receiver { check_mode: XModemReceiveMode },
}

/// XMODEM 协议处理器。实例在创建时绑定本机角色，避免把发送参数误用于接收。
#[derive(Debug, Clone, Copy)]
pub struct XModem {
    role: XModemRole,
}

impl XModem {
    pub fn sender(block_size: usize) -> Self {
        Self {
            role: XModemRole::Sender { block_size },
        }
    }

    pub fn receiver(check_mode: XModemReceiveMode) -> Self {
        Self {
            role: XModemRole::Receiver { check_mode },
        }
    }
}

impl SerialTransferProtocol for XModem {
    fn send_files(
        &self,
        port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        let XModemRole::Sender { block_size } = self.role else {
            return Err("XModem 接收器不能执行发送".into());
        };
        xmodem_send(port, files, block_size, on_progress, on_file_event, cancel)
    }

    fn receive_files(
        &self,
        port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        let XModemRole::Receiver { check_mode } = self.role else {
            return Err("XModem 发送器不能执行接收".into());
        };
        xmodem_receive(
            port,
            download_dir,
            check_mode,
            on_progress,
            on_file_event,
            cancel,
        )
    }
}

// ── XMODEM 发送器 ─────────────────────────────────────

/// XMODEM 按 lrzsz 标准发送文件（仅处理第一个文件）
fn xmodem_send(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    files: &[FileInfo],
    block_size: usize,
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    let mut batch_results: Vec<BatchFileResult> = Vec::new();

    if files.is_empty() {
        return Ok(batch_results);
    }

    // XMODEM 仅支持单文件 — 只处理第一个
    let file_info = &files[0];

    // ── 阶段 1: 接收方只通过 NAK/C 协商校验方式；块长由发送端配置。
    let check_mode = getnak(port, cancel)?;

    log::info!(
        "XModem send: check={:?}, block_size={}, file=\"{}\", size={}",
        check_mode,
        block_size,
        file_info.name,
        file_info.size
    );

    // ── 发送文件开始事件 ──
    on_file_event(FileTransferEvent::FileStart {
        file_name: file_info.name.clone(),
        file_index: 0,
        total_files: 1,
        file_size: file_info.size,
    });

    // ── 阶段 2: 发送文件数据块 ──
    let mut file = std::io::BufReader::new(match fs::File::open(&file_info.path) {
        Ok(f) => f,
        Err(e) => {
            let err_msg = format!("无法打开文件: {}", e);
            on_file_event(FileTransferEvent::FileComplete {
                file_name: file_info.name.clone(),
                file_index: 0,
                total_files: 1,
                bytes_transferred: 0,
                success: false,
                error: Some(err_msg.clone()),
            });
            batch_results.push(BatchFileResult {
                file_name: file_info.name.clone(),
                status: "failed".into(),
                size: 0,
                error: Some(err_msg),
            });
            return Ok(batch_results);
        }
    });

    let mut block_num: u8 = 1;
    let mut total_sent: u64 = 0;
    let mut buf_read = [0u8; BLOCK_SIZE_1K];

    loop {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        let n = match file.read(&mut buf_read[..block_size]) {
            Ok(0) => break, // EOF — 所有数据已发送
            Ok(n) => n,
            Err(e) => {
                let err_msg = format!("读取文件错误: {}", e);
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: 0,
                    total_files: 1,
                    bytes_transferred: total_sent,
                    success: false,
                    error: Some(err_msg.clone()),
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "failed".into(),
                    size: file_info.size,
                    error: Some(err_msg),
                });
                io::send_cancel(port);
                return Ok(batch_results);
            }
        };

        // 构建发送缓冲区：数据 + 0x1A 填充（CPMEOF）
        let mut send_buf = [0x1Au8; BLOCK_SIZE_1K];
        send_buf[..n].copy_from_slice(&buf_read[..n]);

        if let Err(e) = send_block(
            port,
            block_num,
            &send_buf[..block_size],
            block_size,
            check_mode,
            cancel,
        ) {
            let err_msg = e.to_string();
            log::warn!(
                "XModem send: block {} failed for \"{}\": {}",
                block_num,
                file_info.name,
                err_msg
            );
            on_file_event(FileTransferEvent::FileComplete {
                file_name: file_info.name.clone(),
                file_index: 0,
                total_files: 1,
                bytes_transferred: total_sent,
                success: false,
                error: Some(err_msg.clone()),
            });
            batch_results.push(BatchFileResult {
                file_name: file_info.name.clone(),
                status: "failed".into(),
                size: file_info.size,
                error: Some(err_msg),
            });
            io::send_cancel(port);
            return Ok(batch_results);
        }

        total_sent += n as u64;
        on_progress(TransferProgress {
            file_name: file_info.name.clone(),
            bytes_transferred: total_sent,
            total_bytes: file_info.size,
            file_index: 0,
            total_files: 1,
            aggregate_bytes_transferred: total_sent,
            aggregate_total_bytes: file_info.size,
            direction: TransferDirection::Send,
        });

        // 块号 1..=255 循环（wrapping_add 处理回绕）
        block_num = block_num.wrapping_add(1);
        if block_num == 0 {
            block_num = 1;
        }
    }

    // ── 阶段 3: 发送 EOT ──
    if let Err(e) = send_eot(port, cancel) {
        let err_msg = e.to_string();
        log::warn!(
            "XModem send: EOT failed for \"{}\": {}",
            file_info.name,
            err_msg
        );
        on_file_event(FileTransferEvent::FileComplete {
            file_name: file_info.name.clone(),
            file_index: 0,
            total_files: 1,
            bytes_transferred: total_sent,
            success: false,
            error: Some(err_msg.clone()),
        });
        batch_results.push(BatchFileResult {
            file_name: file_info.name.clone(),
            status: "failed".into(),
            size: file_info.size,
            error: Some(err_msg),
        });
        return Ok(batch_results);
    }

    // ── 成功 ──
    on_file_event(FileTransferEvent::FileComplete {
        file_name: file_info.name.clone(),
        file_index: 0,
        total_files: 1,
        bytes_transferred: total_sent,
        success: true,
        error: None,
    });
    batch_results.push(BatchFileResult {
        file_name: file_info.name.clone(),
        status: "completed".into(),
        size: file_info.size,
        error: None,
    });

    log::info!(
        "XModem send: complete \"{}\" ({} bytes)",
        file_info.name,
        total_sent
    );

    Ok(batch_results)
}

/// 等待接收方发送 NAK/C 启动字节，返回协商的校验方式。
fn getnak(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<XModemCheckMode, Box<dyn std::error::Error>> {
    for retry in 0..INIT_TIMEOUT_SECS {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }
        match read_byte_with_timeout(port, 1000)? {
            Some(NAK) => return Ok(XModemCheckMode::Checksum),
            Some(C) => return Ok(XModemCheckMode::Crc16),
            Some(CAN) => return Err("接收方取消了传输".into()),
            Some(other) => log::debug!(
                "XModem getnak: ignoring unsupported init byte 0x{:02X} (retry {})",
                other,
                retry
            ),
            None => {}
        }
    }
    Err(format!(
        "等待 XModem 启动信号超时（{} 秒）。请先在设备终端中执行 XModem 接收命令（如 rx、loadx）。",
        INIT_TIMEOUT_SECS
    )
    .into())
}

/// 发送单个数据块。块长由发送端选择，校验方式由接收端握手选择。
fn send_block(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    block_num: u8,
    data: &[u8],
    block_size: usize,
    check_mode: XModemCheckMode,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let header_byte = match block_size {
        BLOCK_SIZE_128 => SOH,
        BLOCK_SIZE_1K => STX,
        other => return Err(format!("XModem 不支持 {} 字节块", other).into()),
    };
    let trailer_size = match check_mode {
        XModemCheckMode::Checksum => 1,
        XModemCheckMode::Crc16 => 2,
    };
    let mut packet = Vec::with_capacity(3 + block_size + trailer_size);
    packet.push(header_byte);
    packet.push(block_num);
    packet.push(!block_num);
    packet.extend_from_slice(data);

    match check_mode {
        XModemCheckMode::Checksum => {
            packet.push(crc::checksum(data));
        }
        XModemCheckMode::Crc16 => {
            let crc = crc16_ccitt_zero_pad(data);
            packet.push((crc >> 8) as u8);
            packet.push((crc & 0xFF) as u8);
        }
    }

    for retry in 0..MAX_RETRIES {
        if cancel() {
            return Err("传输已取消".into());
        }
        port.write_all(&packet)?;
        port.flush()?;
        match read_byte_with_timeout(port, 3000)? {
            Some(ACK) => return Ok(()),
            Some(CAN) => return Err("接收方取消了传输".into()),
            Some(NAK) | None => {
                if retry == MAX_RETRIES - 1 {
                    return Err(
                        format!("块 {} 重试次数耗尽（{} 次）", block_num, MAX_RETRIES).into(),
                    );
                }
            }
            Some(other) => {
                if retry == MAX_RETRIES - 1 {
                    return Err(
                        format!("块 {} 收到意外响应 0x{:02X}，重试耗尽", block_num, other).into(),
                    );
                }
            }
        }
    }
    Err(format!("块 {} 发送失败", block_num).into())
}

/// 发送 EOT 并等待 ACK 确认（对齐 lrzsz）
fn send_eot(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    for retry in 0..MAX_RETRIES {
        if cancel() {
            return Err("传输已取消".into());
        }

        port.write_all(&[EOT])?;
        port.flush()?;

        match read_byte_with_timeout(port, 5000)? {
            Some(ACK) => {
                log::debug!("XModem EOT: received ACK");
                return Ok(());
            }
            Some(NAK) => {
                log::info!(
                    "XModem EOT: received NAK, retransmitting (retry {})",
                    retry + 1
                );
                continue;
            }
            Some(CAN) => return Err("接收方取消了传输".into()),
            None => {
                if retry == MAX_RETRIES - 1 {
                    return Err("EOT 确认超时".into());
                }
                log::info!("XModem EOT: timeout, retransmitting (retry {})", retry + 1);
            }
            Some(other) => {
                log::debug!("XModem EOT: unexpected 0x{:02X}", other);
                if retry == MAX_RETRIES - 1 {
                    return Err(format!("EOT 收到意外响应 0x{:02X}", other).into());
                }
            }
        }
    }

    Err("EOT 发送失败：超过最大重试次数".into())
}

// ── XMODEM 接收器 ─────────────────────────────────────

/// XMODEM 按 lrzsz 标准接收文件
fn xmodem_receive(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    download_dir: &str,
    configured_check_mode: XModemReceiveMode,
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    fs::create_dir_all(download_dir)?;

    let mut batch_results: Vec<BatchFileResult> = Vec::new();

    // 生成接收文件名（XMODEM 没有元数据块，使用时间戳命名）
    let file_name = generate_receive_filename();
    let file_path = std::path::Path::new(download_dir).join(&file_name);

    log::info!("XModem receive: starting, output file={}", file_name);

    on_file_event(FileTransferEvent::FileStart {
        file_name: file_name.clone(),
        file_index: 0,
        total_files: 1,
        file_size: 0, // XMODEM 无法预知文件大小
    });

    // ── 阶段 1: 接收方选择校验方式；块长由实际 SOH/STX 帧头决定。
    let check_modes: &[XModemCheckMode] = match configured_check_mode {
        XModemReceiveMode::Auto => &[XModemCheckMode::Crc16, XModemCheckMode::Checksum],
        XModemReceiveMode::Crc16 => &[XModemCheckMode::Crc16],
        XModemReceiveMode::Checksum => &[XModemCheckMode::Checksum],
    };
    let attempts_per_mode = if check_modes.len() > 1 {
        INIT_TIMEOUT_SECS.div_ceil(check_modes.len() as u32)
    } else {
        INIT_TIMEOUT_SECS
    };
    let mut negotiated_check: Option<XModemCheckMode> = None;
    let mut first_block_data: Option<(u8, Vec<u8>)> = None;

    'init: for &check_mode in check_modes {
        for _ in 0..attempts_per_mode {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }
            port.write_all(&[check_mode.init_byte()])?;
            port.flush()?;
            match read_byte_with_timeout(port, 1000)? {
                Some(header @ (SOH | STX)) => {
                    let bnum = read_or_fail(port)?;
                    let bnum_neg = read_or_fail(port)?;
                    if bnum != !bnum_neg {
                        port.write_all(&[NAK])?;
                        port.flush()?;
                        continue;
                    }
                    let block_size = if header == STX {
                        BLOCK_SIZE_1K
                    } else {
                        BLOCK_SIZE_128
                    };
                    let mut data = vec![0u8; block_size];
                    for byte in &mut data {
                        *byte = read_or_fail(port)?;
                    }
                    let valid = verify_block(port, &data, check_mode)?;
                    if valid {
                        port.write_all(&[ACK])?;
                        port.flush()?;
                        negotiated_check = Some(check_mode);
                        first_block_data = Some((bnum, data));
                        break 'init;
                    }
                    port.write_all(&[NAK])?;
                    port.flush()?;
                }
                Some(CAN) => return Err("发送方取消了传输".into()),
                Some(other) => log::debug!(
                    "XModem RX: received 0x{:02X} while requesting {:?}",
                    other,
                    check_mode
                ),
                None => {}
            }
        }
    }

    let check_mode = negotiated_check.ok_or(
        "无法与发送方建立 XModem 连接。请确认发送方已启动 XModem 发送（如 sx --xmodem、sx -X）。",
    )?;
    log::info!("XModem receive: negotiated check mode {:?}", check_mode);

    // ── 阶段 2: 处理数据块循环 ──
    let mut received_data: Vec<u8> = Vec::new();
    let mut expected_block_num: u8 = 1; // XMODEM 块号从 1 开始
    let mut last_received_block_num: Option<u8> = None; // 用于重复块检测

    // 处理第一个数据块（在握手阶段已读取）
    if let Some((bnum, data)) = first_block_data.take() {
        // 第一个有效数据的块号即为起始期望号
        received_data.extend_from_slice(&data);
        last_received_block_num = Some(bnum);
        expected_block_num = next_block_num(bnum);

        let fsize = received_data.len() as u64;
        on_progress(TransferProgress {
            file_name: file_name.clone(),
            bytes_transferred: fsize,
            total_bytes: 0,
            file_index: 0,
            total_files: 1,
            aggregate_bytes_transferred: fsize,
            aggregate_total_bytes: 0,
            direction: TransferDirection::Receive,
        });
    }

    // 主接收循环
    loop {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        // 等待块头（每次尝试独立计数超时）
        let mut timeout_count: u32 = 0;

        let header = 'read_header: loop {
            match read_byte_with_timeout(port, 10000)? {
                Some(SOH) => break 'read_header SOH,
                Some(STX) => break 'read_header STX,
                Some(EOT) => break 'read_header EOT,
                Some(CAN) => return Err("发送方取消了传输".into()),
                Some(other) => {
                    log::debug!(
                        "XModem RX: unexpected byte 0x{:02X} waiting for header",
                        other
                    );
                    io::flush_port_buffer(port);
                }
                None => {
                    // 超时 — 发送 NAK 请求重传（对齐 lrzsz）
                    timeout_count += 1;
                    if timeout_count > MAX_RETRIES {
                        return Err("接收超时：发送方无响应".into());
                    }
                    log::debug!("XModem RX: timeout waiting for header, sending NAK");
                    port.write_all(&[NAK])?;
                    port.flush()?;
                }
            }
        };

        // ── EOT 处理 ──
        if header == EOT {
            // 对齐 lrzsz: 收到 EOT → ACK
            port.write_all(&[ACK])?;
            port.flush()?;

            log::info!(
                "XModem receive: EOT received, total {} bytes",
                received_data.len()
            );

            // 将数据写入文件
            let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let mut file = fs::File::create(&file_path)?;
                file.write_all(&received_data)?;
                Ok(())
            })();

            match write_result {
                Ok(()) => {
                    let fsize = received_data.len() as u64;
                    on_file_event(FileTransferEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: 0,
                        total_files: 1,
                        bytes_transferred: fsize,
                        success: true,
                        error: None,
                    });
                    on_progress(TransferProgress {
                        file_name: file_name.clone(),
                        bytes_transferred: fsize,
                        total_bytes: fsize,
                        file_index: 0,
                        total_files: 1,
                        aggregate_bytes_transferred: fsize,
                        aggregate_total_bytes: fsize,
                        direction: TransferDirection::Receive,
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "completed".into(),
                        size: fsize,
                        error: None,
                    });
                }
                Err(e) => {
                    let err_msg = format!("写入文件失败: {}", e);
                    on_file_event(FileTransferEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: 0,
                        total_files: 1,
                        bytes_transferred: received_data.len() as u64,
                        success: false,
                        error: Some(err_msg.clone()),
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "failed".into(),
                        size: received_data.len() as u64,
                        error: Some(err_msg),
                    });
                }
            }
            break;
        }

        // ── 数据块处理 ──
        // 根据实际收到的块头确定块大小（协议鲁棒性：防止变体与头字节不匹配）
        let actual_block_size = if header == STX {
            BLOCK_SIZE_1K
        } else {
            BLOCK_SIZE_128
        };

        // 读取块序号和反码
        let bnum = read_or_fail(port)?;
        let bnum_neg = read_or_fail(port)?;

        if bnum != !bnum_neg {
            log::warn!(
                "XModem RX: block num mismatch ({} vs ~{}), sending NAK",
                bnum,
                bnum_neg
            );
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // 读取数据
        let mut data = vec![0u8; actual_block_size];
        for b in data.iter_mut() {
            *b = read_or_fail(port)?;
        }

        // 读取并验证接收方在握手阶段选择的校验方式。
        let valid = verify_block(port, &data, check_mode)?;

        if !valid {
            log::debug!("XModem RX: block {} checksum/CRC failed, sending NAK", bnum);
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // ── 重复块检测 ──
        if Some(bnum) == last_received_block_num {
            // 重复块（我们的 ACK 丢失，发送方重传）
            // 对齐 lrzsz: ACK 但不写入数据
            log::debug!(
                "XModem RX: duplicate block {}, ACKing without writing",
                bnum
            );
            port.write_all(&[ACK])?;
            port.flush()?;
            continue;
        }

        if bnum != expected_block_num {
            // 意外块号 — 发送 NAK
            log::warn!(
                "XModem RX: unexpected block {} (expected {}), sending NAK",
                bnum,
                expected_block_num
            );
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // ── 有效数据块 ──
        received_data.extend_from_slice(&data);
        last_received_block_num = Some(bnum);
        port.write_all(&[ACK])?;
        port.flush()?;

        let fsize = received_data.len() as u64;
        on_progress(TransferProgress {
            file_name: file_name.clone(),
            bytes_transferred: fsize,
            total_bytes: 0, // 未知总大小
            file_index: 0,
            total_files: 1,
            aggregate_bytes_transferred: fsize,
            aggregate_total_bytes: 0,
            direction: TransferDirection::Receive,
        });

        expected_block_num = next_block_num(bnum);
    }

    Ok(batch_results)
}

fn verify_block(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
    data: &[u8],
    check_mode: XModemCheckMode,
) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(match check_mode {
        XModemCheckMode::Checksum => {
            let check = read_or_fail(port)?;
            crc::checksum_verify(data, check)
        }
        XModemCheckMode::Crc16 => {
            let crc_hi = read_or_fail(port)?;
            let crc_lo = read_or_fail(port)?;
            crc16_ccitt_feedthrough_verify(data, crc_hi, crc_lo)
        }
    })
}

/// 生成接收文件名（XMODEM 无元数据，使用时间戳命名）
fn generate_receive_filename() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("xmodem_received_{}.bin", ts)
}

/// 从串口读取一个字节（无超时回退，用于在已确认数据流到来时读取）
fn read_or_fail(
    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,
) -> Result<u8, Box<dyn std::error::Error>> {
    match read_byte_with_timeout(port, 3000)? {
        Some(b) => Ok(b),
        None => Err("读取超时：数据流中断".into()),
    }
}

/// 计算下一个预期的块号（1..=255 循环，跳过 0）
fn next_block_num(current: u8) -> u8 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}
