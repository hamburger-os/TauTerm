//! XModem 协议实现
//!
//! 支持三种变体：
//! - Standard: 128B 块 + 1 字节校验和
//! - CRC: 128B 块 + 2 字节 CRC-16/CCITT
//! - OneK: 1024B 块 + 2 字节 CRC-16/CCITT
//!
//! 基于 lrzsz-0.12.20 `wcs`/`wcrx`/`wcputsec`/`wcgetsec` 标准流程实现。
//!
//! XMODEM 仅支持单文件传输（无批次模式）。

use std::fs;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::transfer::crc::{self, crc16_ccitt_feedthrough_verify, crc16_ccitt_zero_pad};
use crate::transfer::io::{self, read_byte_with_timeout, CAN};
use crate::transfer::protocol::TransferProtocol;
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
const G: u8 = 0x47;

const BLOCK_SIZE_128: usize = 128;
const BLOCK_SIZE_1K: usize = 1024;
const MAX_RETRIES: u32 = 10;
/// 启动握手总超时时间（秒）
const INIT_TIMEOUT_SECS: u32 = 30;

// ── XModem 变体枚举 ─────────────────────────────────

/// XMODEM 三种协议变体
#[derive(Debug, Clone, Copy, PartialEq)]
enum XModemVariant {
    /// 标准 XMODEM: 128B 块 + 1 字节校验和（接收方发送 NAK 0x15 启动）
    Standard,
    /// XMODEM-CRC: 128B 块 + 2 字节 CRC-16/CCITT（接收方发送 'C' 0x43 启动）
    Crc,
    /// XMODEM-1k: 1024B 块 + 2 字节 CRC-16/CCITT（接收方发送 'G' 0x47 启动）
    OneK,
}

impl XModemVariant {
    /// 返回该变体使用的数据块大小
    fn block_size(&self) -> usize {
        match self {
            XModemVariant::Standard | XModemVariant::Crc => BLOCK_SIZE_128,
            XModemVariant::OneK => BLOCK_SIZE_1K,
        }
    }

    /// 返回块头字节（SOH 或 STX）
    fn header_byte(&self) -> u8 {
        match self {
            XModemVariant::Standard | XModemVariant::Crc => SOH,
            XModemVariant::OneK => STX,
        }
    }

    /// 从启动字节反推变体
    fn from_init_byte(b: u8) -> Option<XModemVariant> {
        match b {
            NAK => Some(XModemVariant::Standard),
            C => Some(XModemVariant::Crc),
            G => Some(XModemVariant::OneK),
            _ => None,
        }
    }
}

// ── XModem 协议处理器 ─────────────────────────────────

/// XMODEM 协议处理器
///
/// 实现 `TransferProtocol` trait，提供标准的 XMODEM 文件收发功能。
/// XMODEM 仅支持单文件传输——`send_files` 入参为切片但仅处理第一个文件。
#[derive(Debug, Clone, Default)]
pub struct XModem;

impl TransferProtocol for XModem {
    fn send_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        xmodem_send(port, files, on_progress, on_file_event, cancel)
    }

    fn receive_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        xmodem_receive(port, download_dir, on_progress, on_file_event, cancel)
    }
}

// ── XMODEM 发送器 ─────────────────────────────────────

/// XMODEM 按 lrzsz 标准发送文件（仅处理第一个文件）
fn xmodem_send(
    port: &mut Box<dyn serialport::SerialPort>,
    files: &[FileInfo],
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

    // ── 阶段 1: 等待接收方发送启动字节（getnak）──
    // 接收方发送 NAK/C/G 表示就绪，同时声明其期望的变体
    let variant = getnak(port, cancel)?;

    log::info!(
        "XModem send: variant={:?}, file=\"{}\", size={}",
        variant,
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
    let block_size = variant.block_size();
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

        if let Err(e) = send_block(port, block_num, &send_buf[..block_size], &variant, cancel) {
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
        log::warn!("XModem send: EOT failed for \"{}\": {}", file_info.name, err_msg);
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

/// 等待接收方发送 NAK/C/G 启动字节，返回协商的变体（对齐 lrzsz getnak）
fn getnak(
    port: &mut Box<dyn serialport::SerialPort>,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<XModemVariant, Box<dyn std::error::Error>> {
    for retry in 0..(INIT_TIMEOUT_SECS) {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }
        match read_byte_with_timeout(port, 1000)? {
            Some(b) if XModemVariant::from_init_byte(b).is_some() => {
                let variant = XModemVariant::from_init_byte(b).unwrap();
                log::info!(
                    "XModem getnak: detected variant {:?} from 0x{:02X} (retry {})",
                    variant,
                    b,
                    retry
                );
                return Ok(variant);
            }
            Some(CAN) => return Err("接收方取消了传输".into()),
            Some(other) => {
                log::debug!(
                    "XModem getnak: ignoring byte 0x{:02X} while waiting for init",
                    other
                );
            }
            None => {
                // 超时 — 继续等待
            }
        }
    }
    Err(format!(
        "等待 XModem 启动信号超时（{} 秒）。请先在设备终端中执行 XModem 接收命令（如 rx、loadx）。",
        INIT_TIMEOUT_SECS
    )
    .into())
}

/// 发送单个数据块（对齐 lrzsz wcputsec）
///
/// 块格式: header_byte + block_num + ~block_num + data(128/1024B) + chk(1B)/crc(2B)
fn send_block(
    port: &mut Box<dyn serialport::SerialPort>,
    block_num: u8,
    data: &[u8],
    variant: &XModemVariant,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let block_size = variant.block_size();
    let header_byte = variant.header_byte();

    // 构建完整数据包
    let packet_size = 3 + block_size
        + match variant {
            XModemVariant::Standard => 1, // 1 字节校验和
            XModemVariant::Crc | XModemVariant::OneK => 2, // 2 字节 CRC-16
        };

    let mut packet = Vec::with_capacity(packet_size);
    packet.push(header_byte);
    packet.push(block_num);
    packet.push(!block_num); // 序号反码，对齐 lrzsz
    packet.extend_from_slice(data);

    match variant {
        XModemVariant::Standard => {
            // 校验和 = 数据字节算术和的两字节补码（对齐 lrzsz）
            let sum = crc::checksum(data);
            packet.push((0u8).wrapping_sub(sum));
        }
        XModemVariant::Crc | XModemVariant::OneK => {
            // lrzsz 零填充 CRC: updcrc(0, updcrc(0, crc))
            let crc = crc16_ccitt_zero_pad(data);
            packet.push((crc >> 8) as u8); // CRC hi byte
            packet.push((crc & 0xFF) as u8); // CRC lo byte
        }
    }

    // 发送 + 重试循环
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
                    return Err(format!("块 {} 重试次数耗尽（{} 次）", block_num, MAX_RETRIES).into());
                }
                log::debug!(
                    "XModem send: block {} NAK/timeout, retry {}/{}",
                    block_num,
                    retry + 1,
                    MAX_RETRIES
                );
            }
            Some(other) => {
                log::debug!(
                    "XModem send: block {} unexpected 0x{:02X}, retry {}/{}",
                    block_num,
                    other,
                    retry + 1,
                    MAX_RETRIES
                );
                if retry == MAX_RETRIES - 1 {
                    return Err(format!(
                        "块 {} 收到意外响应 0x{:02X}，重试耗尽",
                        block_num, other
                    )
                    .into());
                }
            }
        }
    }

    Err(format!("块 {} 发送失败", block_num).into())
}

/// 发送 EOT 并等待 ACK 确认（对齐 lrzsz）
fn send_eot(
    port: &mut Box<dyn serialport::SerialPort>,
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
                log::info!("XModem EOT: received NAK, retransmitting (retry {})", retry + 1);
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
    port: &mut Box<dyn serialport::SerialPort>,
    download_dir: &str,
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

    // ── 阶段 1: 启动握手 —— 发送启动字节尝试协商变体 ──
    // 优先级: OneK > CRC > Standard（对齐 lrzsz 接收方逐步降级策略）
    let probe_order = [
        (XModemVariant::OneK, G),
        (XModemVariant::Crc, C),
        (XModemVariant::Standard, NAK),
    ];

    let mut variant: Option<XModemVariant> = None;
    let mut first_block_data: Option<(u8, Vec<u8>)> = None; // (block_num, data)

    'init: for &(probe_variant, probe_byte) in &probe_order {
        for _retry in 0..INIT_TIMEOUT_SECS {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }

            port.write_all(&[probe_byte])?;
            port.flush()?;

            match read_byte_with_timeout(port, 1000)? {
                Some(h) if h == SOH || h == STX => {
                    // 已收到块头 h（SOH 或 STX），立即读取剩余数据包
                    variant = Some(probe_variant);

                    // 读取块序号和反码
                    let bnum = read_or_fail(port)?;
                    let bnum_neg = read_or_fail(port)?;

                    if bnum != !bnum_neg {
                        // 序列号不匹配，发送 NAK 并重新探测
                        port.write_all(&[NAK])?;
                        port.flush()?;
                        variant = None;
                        break;
                    }

                    // 读取数据
                    let block_size = probe_variant.block_size();
                    let mut data = vec![0u8; block_size];
                    for b in data.iter_mut() {
                        *b = read_or_fail(port)?;
                    }

                    // 读取并验证校验和/CRC
                    let valid = match probe_variant {
                        XModemVariant::Standard => {
                            let chk = read_or_fail(port)?;
                            crc::checksum_verify(&data, chk)
                        }
                        XModemVariant::Crc | XModemVariant::OneK => {
                            let crc_hi = read_or_fail(port)?;
                            let crc_lo = read_or_fail(port)?;
                            crc16_ccitt_feedthrough_verify(&data, crc_hi, crc_lo)
                        }
                    };

                    if valid {
                        port.write_all(&[ACK])?;
                        port.flush()?;
                        first_block_data = Some((bnum, data));
                        break 'init;
                    } else {
                        log::debug!(
                            "XModem RX: first block {:?} verification failed, sending NAK",
                            probe_variant
                        );
                        port.write_all(&[NAK])?;
                        port.flush()?;
                        variant = None; // 当前变体验证失败，尝试下一个
                        break;
                    }
                }
                Some(CAN) => {
                    io::send_cancel(port);
                    return Err("发送方取消了传输".into());
                }
                Some(other) => {
                    log::debug!(
                        "XModem RX: received 0x{:02X} while probing with 0x{:02X}",
                        other,
                        probe_byte
                    );
                }
                None => {
                    // 超时 — 继续发送探测
                }
            }
        }
    }

    let variant = variant.ok_or("无法与发送方建立 XModem 连接：所有变体探测均未收到响应。\n请确认发送方已启动 XModem 发送（如 sx --xmodem、sx -X）。")?;

    log::info!("XModem receive: negotiated variant {:?}", variant);

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
                Some(SOH) if variant.block_size() == BLOCK_SIZE_128 => break 'read_header SOH,
                Some(STX) if variant.block_size() == BLOCK_SIZE_1K => break 'read_header STX,
                Some(EOT) => break 'read_header EOT,
                Some(CAN) => {
                    io::send_cancel(port);
                    return Err("发送方取消了传输".into());
                }
                Some(SOH) | Some(STX) => {
                    // 收到与协商不符的块头类型（如期望 STX 却收到 SOH）
                    // 接受并以此调整块大小
                    break 'read_header SOH;
                }
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
        let actual_block_size = if header == STX { BLOCK_SIZE_1K } else { BLOCK_SIZE_128 };

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

        // 读取并验证校验和/CRC
        let valid = match variant {
            XModemVariant::Standard => {
                let chk = read_or_fail(port)?;
                crc::checksum_verify(&data, chk)
            }
            XModemVariant::Crc | XModemVariant::OneK => {
                let crc_hi = read_or_fail(port)?;
                let crc_lo = read_or_fail(port)?;
                crc16_ccitt_feedthrough_verify(&data, crc_hi, crc_lo)
            }
        };

        if !valid {
            log::debug!(
                "XModem RX: block {} checksum/CRC failed, sending NAK",
                bnum
            );
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // ── 重复块检测 ──
        if Some(bnum) == last_received_block_num {
            // 重复块（我们的 ACK 丢失，发送方重传）
            // 对齐 lrzsz: ACK 但不写入数据
            log::debug!("XModem RX: duplicate block {}, ACKing without writing", bnum);
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
    port: &mut Box<dyn serialport::SerialPort>,
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
