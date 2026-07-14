//! YModem 协议实现
//!
//! 基于 lrzsz-0.12.20 `wcs`/`wctx`/`wcputsec`（发送）和 `wcrx`/`wcgetsec`（接收）标准流程。
//!
//! ## 协议概览
//!
//! ### 发送流程
//! 1. 等待接收方发送 'C'（CRC 模式请求）
//! 2. 发送块 0（文件元数据：filename\0size mtime mode 0 filesleft totalleft，128 字节，lrzsz 格式）
//! 3. 接收方 ACK 块 0 后独立发送 'C' 请求数据块
//! 4. 发送数据块（默认 1024 字节，剩余 ≤ 896 字节时切换为 128 字节块）
//! 5. 发送 EOT，等待 ACK（lrzsz 标准：EOT → ACK）
//! 6. 下一文件重复步骤 2-5，或发送空块 0 结束批次
//!
//! ### 接收流程
//! 1. 发送 'C' 启动 CRC 模式（30 次探针，1s 间隔）
//! 2. 接收块 0（文件元数据）
//! 3. ACK + 发送 'C' 请求数据块
//! 4. 接收数据块，lrzsz 前馈 CRC 验证，写入磁盘
//! 5. 收到 EOT → ACK + 'C' 请求下一文件
//! 6. 收到空块 0 → 批次结束

use std::fs;
use std::io::{Read, Write};

use crate::transfer::crc::{
    checksum, crc16_ccitt_feedthrough_verify, crc16_ccitt_zero_pad,
};
use crate::transfer::io::{
    self, detect_cancel, drain_rx_buffer, read_byte_with_timeout, read_eot_response,
    wait_for_nak_or_c, EotResponse, WaitResult, C, CAN, G, NAK,
};
use crate::transfer::protocol::TransferProtocol;
use crate::transfer::types::{
    BatchFileResult, FileInfo, FileTransferEvent, TransferDirection,
    TransferProgress,
};

// ── YMODEM 协议常量 ──────────────────────────────────

const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;
const ACK: u8 = 0x06;
// NAK (0x15) and C (0x43) are imported from crate::transfer::io

const DATA_BLOCK_SIZE: usize = 1024;
const BLOCK0_SIZE: usize = 128;
const MAX_RETRIES: u32 = 10;
/// 接收方启动 'C' 探针次数（30 × 1s = 30s 总超时，对齐 lrzsz rb）
const INIT_C_RETRIES: u32 = 30;
/// 文件数据尾块阈值（对齐 lrzsz wctx: 896）
/// 剩余字节数 ≤ 此值时切换为 128 字节块
const TRAILER_BLOCK_THRESHOLD: u64 = 896;
/// CP/M EOF 填充字节（对齐 lrzsz filbuf: 0x1A / Ctrl-Z）
const CPMEOF: u8 = 0x1A;

// ── YModem 协议处理器 ─────────────────────────────────

/// YMODEM 协议处理器
///
/// 实现 `TransferProtocol` trait，提供标准的 YMODEM 文件收发功能。
#[derive(Debug, Clone)]
pub struct YModem {
    /// 默认数据块大小：128 或 1024（默认 1024，对齐 lrzsz `-k` 选项）
    pub block_size: usize,
    /// 接收端是否请求 YMODEM-g 流模式（发送 'G' 而非 'C'）
    pub streaming: bool,
}

impl Default for YModem {
    fn default() -> Self {
        YModem {
            block_size: DATA_BLOCK_SIZE,
            streaming: false,
        }
    }
}

impl TransferProtocol for YModem {
    fn send_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        ymodem_send(self, port, files, on_progress, on_file_event, cancel)
    }

    fn receive_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        ymodem_receive(self, port, download_dir, on_progress, on_file_event, cancel)
    }
}

// ── YMODEM 发送器 ─────────────────────────────────────

/// YMODEM 按 lrzsz 标准发送文件批次
///
/// `config.block_size` 用作默认数据块大小（128 或 1024）。
/// 当剩余数据 ≤ `TRAILER_BLOCK_THRESHOLD` 时自动切换为 128 字节块。
fn ymodem_send(
    config: &YModem,
    port: &mut Box<dyn serialport::SerialPort>,
    files: &[FileInfo],
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    let total_files = files.len() as u32;
    let user_block_size = config.block_size.clamp(BLOCK0_SIZE, DATA_BLOCK_SIZE);

    // ── 阶段 1: 等待接收方 CRC/校验和/流模式请求 ──
    // 对齐 lrzsz getnak(): 'C' = CRC-16, NAK = 校验和, 'G' = YMODEM-g 流模式
    // 每个探针在 ~1s 窗口内持续消费非信号字节（噪声/控制台输出），直到收到
    // 'C'/NAK/'G' 或窗口过期。避免因设备控制台输出污染协议数据而错误失败。
    let mut use_crc = true;
    let mut streaming = false;
    let mut last_can = false;
    let mut discarded_bytes: u32 = 0;
    let mut got_signal = false;

    for retry in 0..MAX_RETRIES * 3 {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        let probe_start = std::time::Instant::now();
        loop {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }
            if probe_start.elapsed() > std::time::Duration::from_millis(1000) {
                break; // 本探针窗口过期，发送下一个 'C' 重试（或超时）
            }
            match read_byte_with_timeout(port, 200)? {
                Some(C) => {
                    use_crc = true;
                    got_signal = true;
                    break;
                }
                Some(G) => {
                    streaming = true;
                    use_crc = true;
                    log::info!("ymodem_send: receiver requested YMODEM-g streaming mode");
                    got_signal = true;
                    break;
                }
                Some(NAK) => {
                    use_crc = false;
                    log::info!("ymodem_send: receiver requested checksum mode (NAK)");
                    got_signal = true;
                    break;
                }
                Some(b) if b == CAN => {
                    if detect_cancel(b, &mut last_can) {
                        return Err("接收方取消了传输".into());
                    }
                }
                Some(_) => {
                    // 非信号字节（噪声/控制台输出）：消费并丢弃
                    last_can = false;
                    discarded_bytes += 1;
                }
                None => {
                    last_can = false;
                    // 短超时，继续在本探针窗口内等待
                }
            }
        }

        if discarded_bytes > 0 {
            log::debug!(
                "ymodem_send: discarded {} non-signal byte(s) while waiting for receiver ready signal",
                discarded_bytes
            );
            discarded_bytes = 0;
        }

        if got_signal {
            break;
        }

        // 窗口过期且已到最大重试次数
        if retry == MAX_RETRIES * 3 - 1 {
            return Err(
                "等待设备 YModem 就绪信号超时。请先在设备终端中执行接收命令（如 loady、rb）。"
                    .into(),
            );
        }
    }

    // ── 计算批次聚合总大小 ──
    let mut aggregate_total: u64 = files.iter().map(|f| f.size).sum();
    let mut batch_results: Vec<BatchFileResult> = Vec::with_capacity(files.len());
    let mut aggregate_completed: u64 = 0;

    // ── 阶段 2: 逐文件传输 ──
    for (file_idx, file_info) in files.iter().enumerate() {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        // 文件间同步：等待接收方发送 'C' 请求下一文件块 0
        // 对齐 lrzsz wcsend(): 每文件循环顶部的 getnak() 调用
        // 接收方在 ACK EOT 后发送 'C'，由 wait_for_nak_or_c 直接消费
        if file_idx > 0 {
            match wait_for_nak_or_c(port, 10000, 5)? {
                WaitResult::WantCrc => {
                    log::debug!(
                        "ymodem_send: receiver ready for file {} (received 'C')",
                        file_idx + 1
                    );
                }
                WaitResult::WantChecksum => {
                    log::debug!(
                        "ymodem_send: receiver ready for file {} (received NAK — checksum)",
                        file_idx + 1
                    );
                }
                WaitResult::WantG => {
                    // 接收方在文件间切换到流模式
                    streaming = true;
                    log::info!(
                        "ymodem_send: receiver requested streaming for file {}",
                        file_idx + 1
                    );
                }
                WaitResult::Cancel => {
                    return Err("接收方取消了传输".into());
                }
            }
        }

        let fi = file_idx as u32;

        // 发送文件开始事件
        on_file_event(FileTransferEvent::FileStart {
            file_name: file_info.name.clone(),
            file_index: fi,
            total_files,
            file_size: file_info.size,
        });

        // 打开文件
        // 如果无法打开，在进入循环前修正 aggregate_total
        let file = match fs::File::open(&file_info.path) {
            Ok(f) => f,
            Err(e) => {
                let err_msg = format!("无法打开文件: {}", e);
                // 提前修正聚合总量
                aggregate_total -= file_info.size;
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
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
                continue;
            }
        };

        // ── 发送块 0（文件元数据，lrzsz 格式）──
        // 格式: filename\0size mtime mode 0 filesleft totalleft
        // 字节 126-127: 文件扇区数（IMP/KMD 兼容）
        let name_only = std::path::Path::new(&file_info.name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_info.name);
        let files_left = total_files - fi;
        let total_left: u64 = files[file_idx..].iter().map(|f| f.size).sum();
        let meta_str = format!(
            "{}\0{} {:o} {:o} 0 {} {}",
            name_only, file_info.size, file_info.mtime, 0o100644u32, files_left, total_left
        );
        let meta_bytes = meta_str.as_bytes();

        // 选择块大小：文件名过长（>125 字节）使用 1024 字节块（对齐 lrzsz）
        let b0_block_size = if meta_bytes.len() > 125 { user_block_size } else { BLOCK0_SIZE };
        let mut block0 = vec![0u8; b0_block_size];
        let copy_len = meta_bytes.len().min(b0_block_size);
        block0[..copy_len].copy_from_slice(&meta_bytes[..copy_len]);

        // 扇区计数（位置 126-127，对齐 lrzsz IMP/KMD 兼容）
        let sectors = (file_info.size + 127) >> 7;
        if b0_block_size >= 128 {
            block0[126] = sectors as u8;
            block0[127] = (sectors >> 8) as u8;
        }

        if let Err(e) = send_block(port, 0, &block0, b0_block_size, cancel, use_crc, streaming) {
            let err_msg = e.to_string();
            aggregate_total -= file_info.size;
            on_file_event(FileTransferEvent::FileComplete {
                file_name: file_info.name.clone(),
                file_index: fi,
                total_files,
                bytes_transferred: 0,
                success: false,
                error: Some(err_msg.clone()),
            });
            batch_results.push(BatchFileResult {
                file_name: file_info.name.clone(),
                status: "failed".into(),
                size: file_info.size,
                error: Some(err_msg),
            });
            continue;
        }

        // ── 等待接收方 'C' 请求数据块（对齐 lrzsz wctx getnak()）──
        // 接收方在 ACK 块 0 后会发送 'C' 请求数据块。必须在此等待消费，
        // 否则 send_block(1, ...) 会读到残留的 'C' 并触发重试/报错。
        match wait_for_nak_or_c(port, 10000, 5)? {
            WaitResult::WantCrc => {
                log::debug!("ymodem_send: receiver ready for data blocks (received 'C')");
            }
            WaitResult::WantChecksum => {
                log::debug!("ymodem_send: receiver ready for data blocks (received NAK)");
            }
            WaitResult::WantG => {
                streaming = true;
                log::info!("ymodem_send: receiver switched to streaming for data blocks");
            }
            WaitResult::Cancel => {
                io::send_cancel(port);
                return Err("接收方取消了传输".into());
            }
        }

        // ── 发送数据块 ──
        // 对齐 lrzsz wctx: 剩余字节 ≤ 896 时切换为 128B 块
        // 对齐 lrzsz filbuf: 按精确块大小读取，不足时用 CPMEOF (0x1A) 填充
        let mut block_num: u8 = 1;
        let mut total_sent: u64 = 0;
        let mut file = std::io::BufReader::new(file);

        loop {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }

            // ── 对齐 lrzsz wctx: 在读数据之前确定块大小 ──
            // 这是关键修复：不再先读取再决定块大小，避免 1K→128B 切换时截断数据。
            let remaining = file_info.size.saturating_sub(total_sent);
            let block_size = if remaining <= TRAILER_BLOCK_THRESHOLD {
                BLOCK0_SIZE  // 128
            } else {
                user_block_size  // 用户配置（默认 1024）
            };

            // ── 对齐 lrzsz filbuf: 精确读取 block_size 字节 ──
            // filbuf 返回实际读取的字节数 m（0 ≤ m ≤ count），剩余空间用 CPMEOF 填充。
            let mut data_buf = vec![CPMEOF; block_size];
            let n = match file.read(&mut data_buf) {
                Ok(0) => break, // EOF — 文件已读完
                Ok(n) => n,
                Err(e) => {
                    let err_msg = format!("读取文件错误: {}", e);
                    aggregate_total -= file_info.size;
                    on_file_event(FileTransferEvent::FileComplete {
                        file_name: file_info.name.clone(),
                        file_index: fi,
                        total_files,
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
                    // 对齐 lrzsz: 文件失败后发送 CAN 同步
                    io::send_cancel(port);
                    drain_rx_buffer(port);
                    // signal outer loop to skip EOT
                    total_sent = 0;
                    break;
                }
            };

            // n < block_size 时，data_buf[n..] 已经预填充 CPMEOF，无需额外处理。
            // 只发送 block_size 字节（data_buf 长度 == block_size，不会截断任何实际数据）。

            if let Err(e) = send_block(port, block_num, &data_buf[..block_size], block_size, cancel, use_crc, streaming) {
                let err_msg = e.to_string();
                aggregate_total -= file_info.size;
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
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

                log::warn!(
                    "YModem send: file \"{}\" failed at block {}; sending CAN to sync",
                    file_info.name,
                    block_num
                );
                io::send_cancel(port);
                drain_rx_buffer(port);
                total_sent = 0;
                break;
            }

            // total_sent 用实际读取字节数 n 递增（不含填充），对齐 lrzsz bytes_sent
            total_sent += n as u64;
            on_progress(TransferProgress {
                file_name: file_info.name.clone(),
                bytes_transferred: total_sent,
                total_bytes: file_info.size,
                file_index: fi,
                total_files,
                aggregate_bytes_transferred: aggregate_completed + total_sent,
                aggregate_total_bytes: aggregate_total,
                direction: TransferDirection::Send,
            });

            block_num = block_num.wrapping_add(1);
            // 块序号 0 保留用于元数据块，回绕后必须跳过（对齐 xmodem_receive 同款修复）
            if block_num == 0 {
                block_num = 1;
            }
        }

        // 如果是读错误跳出，跳过 EOT
        if total_sent == 0 && file_info.size > 0 {
            continue;
        }

        // ── 发送 EOT（lrzsz 标准：EOT → ACK）──
        let eot_result = send_eot(port, cancel);
        match eot_result {
            Ok(()) => {
                aggregate_completed += file_info.size;
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
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
                on_progress(TransferProgress {
                    file_name: file_info.name.clone(),
                    bytes_transferred: total_sent,
                    total_bytes: file_info.size,
                    file_index: fi,
                    total_files,
                    aggregate_bytes_transferred: aggregate_completed,
                    aggregate_total_bytes: aggregate_total,
                    direction: TransferDirection::Send,
                });
            }
            Err(e) => {
                let err_msg = e.to_string();
                aggregate_total -= file_info.size;
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
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
            }
        }
    }

    // ── 阶段 3: 发送批次结束（空块 0，不等待 ACK）──
    // 对齐 lrzsz: wcsend() 发送空块 0 后不等待 ACK 直接返回
    let empty_block0 = [0u8; BLOCK0_SIZE];
    let _ = send_block(port, 0, &empty_block0, BLOCK0_SIZE, cancel, use_crc, streaming);
    // fire-and-forget: 即使发送失败也不影响已传输的文件结果

    Ok(batch_results)
}

/// 发送单个块（对齐 lrzsz wcputsec）
///
/// - `crc_mode=true`: CRC-16（2 字节），`crc_mode=false`: 8 位校验和（1 字节）
/// - `streaming=true`: YMODEM-g 流模式，发送后立即返回不等待 ACK
fn send_block(
    port: &mut Box<dyn serialport::SerialPort>,
    block_num: u8,
    data: &[u8],
    block_size: usize,
    cancel: &mut dyn FnMut() -> bool,
    crc_mode: bool,
    streaming: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let header_byte = if block_size == DATA_BLOCK_SIZE { STX } else { SOH };

    let packet_size = if crc_mode {
        3 + block_size + 2  // SOH/STX + block + neg + data + CRC16(2)
    } else {
        3 + block_size + 1  // SOH/STX + block + neg + data + checksum(1)
    };
    let mut packet = Vec::with_capacity(packet_size);
    packet.push(header_byte);
    packet.push(block_num);
    packet.push(!block_num);
    packet.extend_from_slice(data);

    if crc_mode {
        // 使用 lrzsz 零填充 CRC（updcrc(0, updcrc(0, crc))）
        let crc = crc16_ccitt_zero_pad(data);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);
    } else {
        // 8 位算术校验和（对齐 lrzsz wcputsec checksum 模式）
        packet.push(checksum(data));
    }

    // YMODEM-g 流模式：发送后立即返回（对齐 lrzsz wcputsec line 1405-1407）
    if streaming {
        port.write_all(&packet)?;
        port.flush()?;
        return Ok(());
    }

    let mut last_can = false;
    for retry in 0..MAX_RETRIES {
        if cancel() {
            return Err("传输已取消".into());
        }

        port.write_all(&packet)?;
        port.flush()?;

        match read_byte_with_timeout(port, 3000)? {
            Some(ACK) => return Ok(()),
            Some(b) if b == CAN => {
                if detect_cancel(b, &mut last_can) {
                    return Err("接收方取消了传输".into());
                }
            }
            // WANTCRC — 接收方请求以 CRC 模式重传此块（对齐 lrzsz wcputsec）
            // 在 YMODEM CRC 模式中，接收方任何时候发送 'C' 都表示
            // "未收到有效块，请重发"，应视为重试信号而非意外响应。
            Some(C) => {
                log::debug!(
                    "send_block: block {} received 'C' (WANTCRC), retrying",
                    block_num
                );
                last_can = false;
                if retry == MAX_RETRIES - 1 {
                    return Err(format!("块 {} 收到 'C' 重试次数耗尽", block_num).into());
                }
            }
            Some(NAK) | None => {
                last_can = false;
                if retry == MAX_RETRIES - 1 {
                    return Err(format!("块 {} 重试次数耗尽", block_num).into());
                }
            }
            Some(other) => {
                last_can = false;
                log::warn!(
                    "send_block: block {} received unexpected byte 0x{:02X}, retrying",
                    block_num, other
                );
                if retry == MAX_RETRIES - 1 {
                    return Err(format!(
                        "块 {} 收到意外响应 0x{:02X}，重试次数耗尽",
                        block_num, other
                    ).into());
                }
            }
        }
    }

    Err(format!("块 {} 发送失败", block_num).into())
}

/// 发送 EOT（对齐 lrzsz wctx + rt-thread 设备双 EOT 兼容）
///
/// 实现两种 EOT 完成路径:
/// 1. **标准 lrzsz 路径**: EOT → ACK, 一步完成
/// 2. **rt-thread 设备双 EOT 路径**: EOT → NAK → EOT → ACK+C
///    (对应设备端 `_rym_do_fin()` lines 472-489)
///
/// 同时处理设备 `_rym_do_send_eot()` 的另一种变体: EOT → 'C'（直接就绪）
fn send_eot(
    port: &mut Box<dyn serialport::SerialPort>,
    cancel: &mut dyn FnMut() -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_can = false;
    for attempt in 0..MAX_RETRIES {
        if cancel() {
            return Err("传输已取消".into());
        }
        port.write_all(&[EOT])?;
        port.flush()?;

        match read_eot_response(port, 5000)? {
            EotResponse::Ack => {
                // ── 标准 lrzsz 路径: EOT → ACK ──
                // 调用方 (ymodem_send) 在文件循环顶部通过 wait_for_nak_or_c
                // 消费可能的残留 'C' 字节
                log::debug!("EOT: received ACK (single-EOT lrzsz path)");
                return Ok(());
            }
            EotResponse::Nak => {
                // ── rt-thread 设备双 EOT 路径: EOT → NAK → (need 2nd EOT) ──
                // 设备 `_rym_do_fin()`: 收到 EOT 后发送 NAK, 期望第二个 EOT 确认,
                // 然后发送 ACK + 'C' 请求下一文件
                last_can = false;
                log::debug!("EOT: received NAK, sending second EOT (two-EOT device path)");

                // 立即发送第二个 EOT
                port.write_all(&[EOT])?;
                port.flush()?;

                // 等待 ACK（设备在收到第二个 EOT 后发送 ACK）
                match read_eot_response(port, 5000)? {
                    EotResponse::Ack => {
                        log::debug!("EOT: two-EOT path complete (received ACK after 2nd EOT)");
                        // 设备 _rym_do_fin(): ACK 后立即发送 'C' 请求下一文件
                        // 调用方将在文件循环顶部通过 wait_for_nak_or_c 消费该 'C'
                        return Ok(());
                    }
                    EotResponse::Can => {
                        if detect_cancel(CAN, &mut last_can) {
                            return Err("接收方取消了传输".into());
                        }
                        continue;
                    }
                    EotResponse::Nak => {
                        // 收到 NAK 后设备仍请求更多 EOT，重试（极少发生）
                        log::warn!("EOT: received NAK after second EOT, retrying");
                        continue;
                    }
                    _ => {
                        log::warn!(
                            "EOT: unexpected response after second EOT (attempt {}/{})",
                            attempt + 1,
                            MAX_RETRIES
                        );
                        continue;
                    }
                }
            }
            EotResponse::WantCrc => {
                // ── 设备 `_rym_do_send_eot()` 变体: EOT → 'C' 直接就绪 ──
                // 设备发送端收到 'C' 后直接跳过 EOT 确认, 进入下一文件
                log::debug!("EOT: received 'C' directly (receiver ready for next file)");
                return Ok(());
            }
            EotResponse::Can => {
                if detect_cancel(CAN, &mut last_can) {
                    return Err("接收方取消了传输".into());
                }
                // 单个 CAN, 继续等待
                continue;
            }
            EotResponse::Timeout => {
                last_can = false;
                if attempt == MAX_RETRIES - 1 {
                    return Err("EOT 确认超时：设备可能正在处理文件（Flash 写入）".into());
                }
                log::debug!("EOT: timeout, retrying ({}/{})", attempt + 1, MAX_RETRIES);
                continue;
            }
            EotResponse::Other(b) => {
                last_can = false;
                log::warn!("EOT: unexpected byte 0x{:02X}, retrying", b);
                if attempt == MAX_RETRIES - 1 {
                    return Err(format!("EOT 收到意外响应 0x{:02X}，重试耗尽", b).into());
                }
                continue;
            }
        }
    }
    Err("EOT 发送失败：超过最大重试次数".into())
}

// ── YMODEM 接收器 ─────────────────────────────────────

/// YMODEM 按 lrzsz 标准接收文件批次
fn ymodem_receive(
    _config: &YModem,
    port: &mut Box<dyn serialport::SerialPort>,
    download_dir: &str,
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    fs::create_dir_all(download_dir)?;
    // 清空接收缓冲区，丢弃之前会话可能残留的杂散字节
    io::flush_port_buffer(port);

    let mut current_file: Option<(String, fs::File, u64, u64)> = None;
    let mut last_block_num: Option<u8> = None;
    let mut file_index: u32 = 0;
    let mut aggregate_bytes: u64 = 0;
    let mut aggregate_total: u64 = 0;
    let mut batch_results: Vec<BatchFileResult> = Vec::new();
    // 从块 0 的 filesleft 字段推导的总文件数（0 表示未知）
    let mut known_total_files: u32 = 0;

    // ── 阶段 1: 发送 'C' 启动 CRC 模式，原子化接收首个完整块 ──
    // 对齐 lrzsz wcgetsec 和 设备端 _rym_do_handshake:
    // 探测到 SOH/STX 后必须立即读取完整块（序号+反码+数据+CRC），
    // 否则外层循环会读到块序号（0x00）而非预期的 SOH/STX 头，导致协议失步。
    //
    // 关键：设备端 _rym_send_begin 在发送块 0（SOH）之前会通过 rt_kprintf
    // 向同一串口输出控制台文本（如 "Sending: xxx (N bytes)\n"）。
    // 因此每个 'C' 探针必须在 1s 窗口内持续消费并丢弃非头字节，
    // 直到收到真正的 SOH/STX，而非每读到一个非头字节就发送下一个 'C'。
    let mut last_can = false;
    let mut first_block: Option<(u8, Vec<u8>)> = None; // (block_num, data)
    let mut discarded_bytes: u32 = 0;

    for retry in 0..INIT_C_RETRIES {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }
        port.write_all(&[C])?;
        port.flush()?;

        // ── 内层循环：在 ~1s 窗口内持续读字节，丢弃非头字节 ──
        let probe_start = std::time::Instant::now();
        let header_byte: Option<u8> = loop {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }
            if probe_start.elapsed() > std::time::Duration::from_millis(1000) {
                break None; // 本探针窗口过期
            }
            match read_byte_with_timeout(port, 200)? {
                Some(b @ SOH) | Some(b @ STX) => break Some(b),
                Some(b) if b == CAN => {
                    if detect_cancel(b, &mut last_can) {
                        return Err("发送方取消了传输".into());
                    }
                }
                Some(_) => {
                    // 非头字节（控制台输出 / 噪声）：消费并丢弃
                    last_can = false;
                    discarded_bytes += 1;
                }
                None => {
                    last_can = false;
                    // 短超时（200ms），继续在内层循环中等待
                }
            }
        };

        if discarded_bytes > 0 {
            log::debug!(
                "YModem RX: discarded {} non-header byte(s) before detecting SOH/STX",
                discarded_bytes
            );
            discarded_bytes = 0;
        }

        let header_byte = match header_byte {
            Some(hdr) => hdr,
            None => {
                // 1s 窗口过期 → 发送下一个 'C'
                if retry == INIT_C_RETRIES - 1 {
                    return Err(
                        "启动传输超时（等待发送方响应 30 秒）。请确认发送方已启动 YModem 发送。"
                            .into(),
                    );
                }
                continue;
            }
        };

        // ── 已检测到 SOH/STX，立即读取完整块 ──
        let block_size = if header_byte == STX { DATA_BLOCK_SIZE } else { BLOCK0_SIZE };
        log::debug!(
            "YModem RX: received {} header after {} 'C' probes",
            if header_byte == STX { "STX (1024B)" } else { "SOH (128B)" },
            retry + 1
        );

        // 读取块序号
        let block_num = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => {
                port.write_all(&[NAK])?;
                port.flush()?;
                last_can = false;
                continue;
            }
        };

        // 读取块序号反码
        let block_num_neg = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => {
                port.write_all(&[NAK])?;
                port.flush()?;
                last_can = false;
                continue;
            }
        };

        // 验证序号: block_num + ~block_num 必须等于 0xFF
        if block_num != !block_num_neg {
            log::warn!(
                "YModem RX: first block seq mismatch ({} vs ~{}=0x{:02X})",
                block_num,
                block_num_neg,
                !block_num_neg
            );
            port.write_all(&[NAK])?;
            port.flush()?;
            last_can = false;
            continue;
        }

        // 读取数据（逐字节，每字节 1s 超时）
        let mut data = vec![0u8; block_size];
        let mut data_ok = true;
        for b in data.iter_mut() {
            match read_byte_with_timeout(port, 1000)? {
                Some(byte) => *b = byte,
                None => {
                    data_ok = false;
                    break;
                }
            }
        }
        if !data_ok {
            port.write_all(&[NAK])?;
            port.flush()?;
            last_can = false;
            continue;
        }

        // 读取 CRC（高字节在前，对齐 lrzsz/lsz CRC 格式）
        let crc_hi = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => {
                port.write_all(&[NAK])?;
                port.flush()?;
                last_can = false;
                continue;
            }
        };
        let crc_lo = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => {
                port.write_all(&[NAK])?;
                port.flush()?;
                last_can = false;
                continue;
            }
        };

        // ── lrzsz 前馈 CRC 验证 ──
        if !crc16_ccitt_feedthrough_verify(&data, crc_hi, crc_lo) {
            log::warn!("YModem RX: first block CRC failed, sending NAK");
            port.write_all(&[NAK])?;
            port.flush()?;
            last_can = false;
            continue;
        }

        // 块验证通过，发送 ACK
        log::info!(
            "YModem RX: first block {} received and validated (seq={}, {}B, CRC OK)",
            if block_num == 0 { "0 (metadata)" } else { "?" },
            block_num,
            block_size
        );
        port.write_all(&[ACK])?;
        port.flush()?;
        first_block = Some((block_num, data));
        last_can = false;
        break;
    }

    // ── 阶段 1.5: 处理握手期间捕获的首块（块 0）──
    // 对齐 lrzsz: 探测循环已消费 SOH/STX + 完整块体。
    // 必须在进入外循环前处理此块，否则外循环会等待永不出现的第二个 SOH/STX。
    if let Some((block_num, data)) = first_block.take() {
        if block_num != 0 {
            log::error!(
                "YModem RX: first block has unexpected seq {} (expected 0)",
                block_num
            );
            return Err(format!("协议错误：首个块序号为 {} 而非 0", block_num).into());
        }

        // 空块 0 → 批次结束（发送方无文件可传）
        if data[0] == 0 {
            log::info!("YModem RX: empty block 0 (handshake phase) — end of batch");
            port.write_all(&[ACK])?;
            port.flush()?;
            return Ok(batch_results);
        }

        // ── 解析文件元数据（lrzsz 格式）──
        // 格式: filename\0size mtime mode serialno filesleft totalleft
        let null_pos = data.iter().position(|&b| b == 0);
        if let Some(pos) = null_pos {
            let raw_name = String::from_utf8_lossy(&data[..pos]).to_string();
            let safe_name = std::path::Path::new(&raw_name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&raw_name)
                .to_string();
            if safe_name.is_empty() {
                log::warn!(
                    "YModem RX: block 0 (handshake) empty filename after sanitization, skipping"
                );
                port.write_all(&[ACK])?;
                port.flush()?;
                // 发送 'C' 请求下一文件
                port.write_all(&[C])?;
                port.flush()?;
            } else {
                let file_path = std::path::Path::new(download_dir).join(&safe_name);

                let rest = &data[pos + 1..];
                let info_str = rest
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| b as char)
                    .collect::<String>();
                let tokens: Vec<&str> = info_str.split_whitespace().collect();
                let total_size: u64 = tokens.first().and_then(|t| t.parse().ok()).unwrap_or(0);
                let filesleft: u32 = tokens.get(4).and_then(|t| t.parse().ok()).unwrap_or(0);
                // 对齐 lrzsz: filesleft 含当前文件
                known_total_files = file_index + filesleft;
                log::debug!(
                    "YModem RX: block 0 (handshake) size={}, filesleft={}, known_total={}",
                    total_size,
                    filesleft,
                    known_total_files
                );

                aggregate_total += total_size;

                match fs::File::create(&file_path) {
                    Ok(file) => {
                        current_file = Some((safe_name.clone(), file, total_size, 0u64));
                        last_block_num = None;
                        on_file_event(FileTransferEvent::FileStart {
                            file_name: safe_name.clone(),
                            file_index,
                            total_files: known_total_files,
                            file_size: total_size,
                        });
                        on_progress(TransferProgress {
                            file_name: safe_name,
                            bytes_transferred: 0,
                            total_bytes: total_size,
                            file_index,
                            total_files: known_total_files,
                            aggregate_bytes_transferred: aggregate_bytes,
                            aggregate_total_bytes: aggregate_total,
                            direction: TransferDirection::Receive,
                        });
                    }
                    Err(e) => {
                        return Err(format!("无法创建文件 {:?}: {}", file_path, e).into());
                    }
                }
                // 对齐 lrzsz: ACK block 0 后发送 'C' 请求数据块
                // 注意：ACK 已在探测循环中发送（CRC 验证通过后），此处仅发送 'C'
                port.write_all(&[C])?;
                port.flush()?;
            }
        }

        log::info!(
            "YModem RX: entering outer loop for file \"{}\" (index {})",
            current_file
                .as_ref()
                .map(|(n, _, _, _)| n.as_str())
                .unwrap_or("unknown"),
            file_index
        );
    }

    // ── 阶段 2: 接收文件数据 ──
    let mut outer_iter: u32 = 0;
    'outer: loop {
        outer_iter += 1;
        log::info!("YModem RX: outer loop iteration {} (waiting for header)", outer_iter);
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        // 读取块头
        // 持续消费非头字节（设备控制台输出 / 噪声），直到收到有效的头字节或超时。
        // 设备端 _rym_send_begin 在每个文件前通过 rt_kprintf 输出文本到同一串口。
        let header = 'read_header: loop {
            match read_byte_with_timeout(port, 5000)? {
                Some(SOH) => {
                    log::info!("YModem RX: got SOH header (128B block)");
                    break 'read_header SOH;
                }
                Some(STX) => {
                    log::info!("YModem RX: got STX header (1024B block)");
                    break 'read_header STX;
                }
                Some(EOT) => {
                    log::info!("YModem RX: <<< EOT received >>> file_index={}", file_index);
                    last_can = false;
                    // ── 文件结束 ──
                    if let Some((name, _, _total, bytes_written)) = current_file.take() {
                        log::info!(
                            "YModem RX: file complete \"{}\" ({} bytes, index {})",
                            name,
                            bytes_written,
                            file_index
                        );
                        let fsize = bytes_written;
                        aggregate_bytes += fsize;
                        on_file_event(FileTransferEvent::FileComplete {
                            file_name: name.clone(),
                            file_index,
                            total_files: known_total_files,
                            bytes_transferred: fsize,
                            success: true,
                            error: None,
                        });
                        on_progress(TransferProgress {
                            file_name: name.clone(),
                            bytes_transferred: fsize,
                            total_bytes: fsize,
                            file_index,
                            total_files: known_total_files,
                            aggregate_bytes_transferred: aggregate_bytes,
                            aggregate_total_bytes: aggregate_total,
                            direction: TransferDirection::Receive,
                        });
                        batch_results.push(BatchFileResult {
                            file_name: name,
                            status: "completed".into(),
                            size: fsize,
                            error: None,
                        });
                        file_index += 1;
                    }
                    // YMODEM 批量模式：ACK EOT → 延迟 → 发送 'C' 请求下一文件
                    // 延迟确保设备逐字节读取机制正确接收两个独立字节（对齐 _rym_do_send_eot）
                    log::info!("YModem RX: sending ACK for EOT...");
                    port.write_all(&[ACK])?;
                    port.flush()?;
                    log::info!("YModem RX: ACK sent, sleeping 10ms before 'C'");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    port.write_all(&[C])?;
                    port.flush()?;
                    log::info!("YModem RX: 'C' sent after EOT, continuing loop");
                    continue 'outer;
                }
                Some(CAN) => {
                    // 双 CAN 检测（对齐 lrzsz wcgetsec: 连续两个 CAN 才视为取消）
                    if detect_cancel(CAN, &mut last_can) {
                        return Err("发送方取消了传输".into());
                    }
                    // 单个 CAN：噪声，继续等待头字节
                }
                Some(other) => {
                    // 非头字节（控制台输出 / 噪声）：消费并丢弃，继续等待
                    // 设备端 rt_kprintf 输出文本（如 "Sending: xxx (N bytes)\n"）
                    // 通过同一串口传输，出现在块 0 SOH 之前
                    last_can = false;
                    log::debug!(
                        "YModem RX: discarding non-header byte 0x{:02X} ('{}') waiting for block",
                        other,
                        if other.is_ascii_graphic() { other as char } else { '?' }
                    );
                }
                None => return Err("等待块超时".into()),
            }
        };

        let block_size = if header == STX { DATA_BLOCK_SIZE } else { BLOCK0_SIZE };

        // 读取块序号和反码
        let block_num = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => return Err("读取块序号超时".into()),
        };
        let block_num_neg = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => return Err("读取块序号反码超时".into()),
        };

        if block_num != !block_num_neg {
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // 读取块数据
        let mut data = vec![0u8; block_size];
        for b in data.iter_mut() {
            *b = match read_byte_with_timeout(port, 1000)? {
                Some(byte) => byte,
                None => return Err("读取块数据超时".into()),
            };
        }

        // 读取 CRC
        let crc_hi = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => return Err("读取 CRC 超时".into()),
        };
        let crc_lo = match read_byte_with_timeout(port, 1000)? {
            Some(b) => b,
            None => return Err("读取 CRC 超时".into()),
        };

        // ── lrzsz 前馈 CRC 验证 ──
        if !crc16_ccitt_feedthrough_verify(&data, crc_hi, crc_lo) {
            port.write_all(&[NAK])?;
            port.flush()?;
            continue;
        }

        // ── 块 0 处理 ──
        if block_num == 0 {
            // 空块 0 → 批次结束
            if data[0] == 0 {
                log::info!("YModem RX: empty block 0 -- end of batch");
                if let Some((name, _, _total, bytes_written)) = current_file.take() {
                    on_file_event(FileTransferEvent::FileComplete {
                        file_name: name.clone(),
                        file_index,
                        total_files: known_total_files,
                        bytes_transferred: bytes_written,
                        success: true,
                        error: None,
                    });
                    batch_results.push(BatchFileResult {
                        file_name: name,
                        status: "completed".into(),
                        size: bytes_written,
                        error: None,
                    });
                }
                port.write_all(&[ACK])?;
                port.flush()?;
                break;
            }

            // 关闭上一个文件（如果存在）
            if let Some((prev_name, _, _prev_total, prev_bytes_written)) = current_file.take() {
                aggregate_bytes += prev_bytes_written;
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: prev_name.clone(),
                    file_index: file_index.saturating_sub(1),
                    total_files: known_total_files,
                    bytes_transferred: prev_bytes_written,
                    success: true,
                    error: None,
                });
                batch_results.push(BatchFileResult {
                    file_name: prev_name,
                    status: "completed".into(),
                    size: prev_bytes_written,
                    error: None,
                });
            }

            // ── 解析文件元数据：lrzsz 格式 ──
            // 格式: filename\0size mtime mode serialno filesleft totalleft
            let null_pos = data.iter().position(|&b| b == 0);
            if let Some(pos) = null_pos {
                let raw_name = String::from_utf8_lossy(&data[..pos]).to_string();
                // ── 路径净化：防止路径穿越攻击（对齐 lrzsz procheader junk_path）──
                // 仅保留基本文件名，拒绝空文件名和仅含目录分隔符的路径
                let safe_name = std::path::Path::new(&raw_name)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&raw_name)
                    .to_string();
                if safe_name.is_empty() {
                    log::warn!("YModem RX: block 0 has empty filename after sanitization, skipping");
                    port.write_all(&[ACK])?;
                    port.flush()?;
                    continue;
                }
                let file_path = std::path::Path::new(download_dir).join(&safe_name);

                // 解析 space-separated 字段（跳过第一个 null 后）
                // 格式: size mtime mode serialno filesleft totalleft
                let rest = &data[pos + 1..];
                let info_str = rest
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| b as char)
                    .collect::<String>();
                let tokens: Vec<&str> = info_str.split_whitespace().collect();
                // tokens[0] = size (decimal), tokens[1] = mtime (octal),
                // tokens[2] = mode (octal), tokens[3] = serialno ("0"),
                // tokens[4] = filesleft (decimal), tokens[5] = totalleft (decimal)
                let total_size: u64 = tokens.first().and_then(|t| t.parse().ok()).unwrap_or(0);
                let filesleft: u32 = tokens.get(4).and_then(|t| t.parse().ok()).unwrap_or(0);
                // 对齐 lrzsz: filesleft 含当前文件（lrzsz sprintf 用 Filesleft 在 --Filesleft 之前）
                known_total_files = file_index + filesleft;
                log::debug!(
                    "YModem RX: block 0 parsed size={}, filesleft={}, known_total={}",
                    total_size, filesleft, known_total_files
                );

                aggregate_total += total_size;

                match fs::File::create(&file_path) {
                    Ok(file) => {
                        current_file = Some((safe_name.clone(), file, total_size, 0u64));
                        last_block_num = None;
                        on_file_event(FileTransferEvent::FileStart {
                            file_name: safe_name.clone(),
                            file_index,
                            total_files: known_total_files,
                            file_size: total_size,
                        });
                        on_progress(TransferProgress {
                            file_name: safe_name,
                            bytes_transferred: 0,
                            total_bytes: total_size,
                            file_index,
                            total_files: known_total_files,
                            aggregate_bytes_transferred: aggregate_bytes,
                            aggregate_total_bytes: aggregate_total,
                            direction: TransferDirection::Receive,
                        });
                    }
                    Err(e) => {
                        return Err(format!("无法创建文件 {:?}: {}", file_path, e).into());
                    }
                }
            }
            port.write_all(&[ACK])?;
            port.flush()?;
            // 对齐 lrzsz: ACK block 0 后发送 'C' 请求数据块
            port.write_all(&[C])?;
            port.flush()?;
        } else {
            // ── 数据块: 重复包检测 ──
            if let Some(last) = last_block_num {
                if block_num == last {
                    log::warn!(
                        "YModem RX: duplicate block {}, sending ACK, skipping write",
                        block_num
                    );
                    port.write_all(&[ACK])?;
                    port.flush()?;
                    continue;
                }
            }

            // 写入当前文件
            if let Some((ref file_name, ref mut file, total_size, ref mut bytes_written)) =
                current_file
            {
                let write_len: usize = if total_size > 0 {
                    let remaining = (total_size - *bytes_written) as usize;
                    remaining.min(block_size)
                } else {
                    // 未知文件大小：回退到 0x1A 填充检测（对齐 lrzsz）
                    data.iter()
                        .rposition(|&b| b != 0x1A)
                        .map_or(0, |p| p + 1)
                };
                file.write_all(&data[..write_len])?;
                *bytes_written += write_len as u64;
                on_progress(TransferProgress {
                    file_name: file_name.clone(),
                    bytes_transferred: *bytes_written,
                    total_bytes: total_size,
                    file_index,
                    total_files: known_total_files,
                    aggregate_bytes_transferred: aggregate_bytes + *bytes_written,
                    aggregate_total_bytes: aggregate_total,
                    direction: TransferDirection::Receive,
                });
            }
            port.write_all(&[ACK])?;
            port.flush()?;
            log::info!("YModem RX: ACK sent for data block {}", block_num);
            last_block_num = Some(block_num);
        }
    }

    Ok(batch_results)
}
