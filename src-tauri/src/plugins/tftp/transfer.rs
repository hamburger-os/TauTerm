//! TFTP 传输引擎
//!
//! 实现核心传输循环（send/receive），替代 `tftpd::Worker`（因其内部类型未公开导出）。
//! 复用 `tftpd::Packet`、`tftpd::Socket`、`tftpd::ErrorCode`、`tftpd::OptionType`、`tftpd::TransferOption`。
//!
//! RFC 1350: 基本 send/receive + SAS 修复 — ✅ 已实现
//! RFC 2347/2348/2349: 选项协商 (OACK) — ✅ 已实现（由调用方处理）
//! RFC 7440: 滑动窗口 — ❌ 未实现（windowsize 参数预留）

use std::fs;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tftpd::{Packet, Socket};

use super::counting_socket::CountingSocket;
use super::TftpDynamicParams;
use super::TftpRollover;

/// 传输结果
#[derive(Debug)]
pub struct TransferResult {
    pub bytes_transferred: u64,
    /// 传输数据的 CRC32 校验值（None = 未计算，通常在上传时服务端不计算）
    pub checksum: Option<u32>,
    /// 传输耗时（毫秒）
    pub duration_ms: u64,
}

/// 发送文件（服务端 RRQ 响应 / 客户端 PUT）
///
/// 读取本地文件，分块发送 DATA 包，等待 ACK。
pub fn send_file(
    counting: &mut CountingSocket,
    file_path: PathBuf,
    remote: SocketAddr,
    params: &TftpDynamicParams,
    abort: &Arc<AtomicBool>,
) -> Result<TransferResult, String> {
    // 使用 CountingSocket 中存储的协商后 blksize，而非 params.blksize。
    // params 可能在传输开始前被前端更新（防抖竞态），导致协商值 ≠ 本地参数值。
    let block_size = counting.blksize();
    let timeout = Duration::from_secs(params.timeout_secs.max(1) as u64);
    let max_retries = params.max_retries.max(1);
    let repeat_count = params.repeat_count.max(1) as usize;

    let mut file = fs::File::open(&file_path)
        .map_err(|e| format!("无法打开文件: {}", e))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    counting.set_total_size(file_size);

    log::info!("[TFTP send_file] 开始: file={}, size={} bytes, blksize={}, timeout={}s, max_retries={}, rollover={:?}",
        file_path.display(), file_size, block_size, params.timeout_secs, max_retries, params.rollover);

    let started = std::time::Instant::now();
    let mut hasher = crc32fast::Hasher::new();
    let mut block_num: u16 = 1;
    let mut bytes_sent: u64 = 0;

    loop {
        if abort.load(Ordering::Relaxed) {
            return Err("传输已取消".into());
        }

        // 读一块
        let mut buf = vec![0u8; block_size];
        let n = file.read(&mut buf).map_err(|e| format!("文件读取失败: {}", e))?;
        buf.truncate(n);
        hasher.update(&buf);
        counting.record_data_bytes(n as u64);

        let data_pkt = Packet::Data {
            block_num,
            data: buf.clone(),
        };

        // 发送 DATA 并等待 ACK
        let mut retries = 0;
        loop {
            if abort.load(Ordering::Relaxed) {
                return Err("传输已取消".into());
            }

            // 发送 DATA（重复发包以提高不可靠网络下的可靠性）
            for i in 0..repeat_count {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
                counting.send_to(&data_pkt, &remote)
                    .map_err(|e| format!("发送失败: {}", e))?;
            }

            // 等待 ACK
            counting.set_read_timeout(timeout)
                .map_err(|e| format!("设置超时失败: {}", e))?;

            match counting.recv() {
                Ok(Packet::Ack(ack_num)) => {
                    if ack_num == block_num {
                        bytes_sent += n as u64;
                        break; // ACK 匹配，继续下一块
                    }
                    // 重复 ACK — SAS 修复：忽略
                }
                Ok(Packet::Error { code, msg }) => {
                    return Err(format!("远端错误: {} - {}", code, msg));
                }
                Ok(_) => {
                    // 意外包类型，重试
                }
                Err(_) => {
                    // 超时，重试
                }
            }

            retries += 1;
            if retries >= max_retries {
                log::error!("[TFTP send_file] 超过最大重试次数 block={}, retries={}, bytes_sent={}", block_num, retries, bytes_sent);
                return Err(format!("超过最大重试次数 ({})", max_retries));
            }

            log::warn!("[TFTP send_file] 重试 block={}, retry={}/{}, bytes_sent={}", block_num, retries, max_retries, bytes_sent);

            // 指数退避
            let backoff = timeout * 2u32.pow(retries as u32).min(60);
            std::thread::sleep(backoff.min(Duration::from_secs(30)));
        }

        // 最后一块
        if n < block_size {
            log::info!("[TFTP send_file] 最后一块 block={}, size={}, bytes_sent={}", block_num, n, bytes_sent);
            break;
        }

        // Block 号回绕
        let prev_block = block_num;
        block_num = block_num.wrapping_add(1);
        let after_wrap = block_num;
        block_num = match params.rollover {
            TftpRollover::None => block_num, // allow 0
            TftpRollover::Enforce0 => if block_num == 0 { 1 } else { block_num },
            TftpRollover::Enforce1 => match block_num { 0 => 2, 1 => 2, n => n },
            TftpRollover::DontCare => block_num,
        };
        if after_wrap != block_num {
            log::info!("[TFTP send_file] 块号回绕: {} → wrapping={} → {} (policy={:?}), bytes_sent={}",
                prev_block, after_wrap, block_num, params.rollover, bytes_sent);
        }
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    Ok(TransferResult {
        bytes_transferred: bytes_sent,
        checksum: Some(hasher.finalize()),
        duration_ms,
    })
}

/// 接收文件（服务端 WRQ 响应 / 客户端 GET）
///
/// 接收 DATA 包，发送 ACK，写入本地文件。
///
/// `start_block`: 起始块号，默认 1。当客户端 GET 已在 OACK 协商阶段
/// 处理了 DATA[1] 时传入 2，文件以追加模式打开，跳过块 1 的接收。
pub fn receive_file(
    counting: &mut CountingSocket,
    file_path: PathBuf,
    remote: SocketAddr,
    params: &TftpDynamicParams,
    abort: &Arc<AtomicBool>,
    start_block: u16,
) -> Result<TransferResult, String> {
    // 使用 CountingSocket 中存储的协商后 blksize，而非 params.blksize。
    let block_size = counting.blksize();
    let timeout = Duration::from_secs(params.timeout_secs.max(1) as u64);

    let repeat_count = params.repeat_count.max(1) as usize;

    // 根据起始块号选择文件打开模式
    let mut file = if start_block <= 1 {
        fs::File::create(&file_path)
            .map_err(|e| format!("无法创建文件: {}", e))?
    } else {
        std::fs::OpenOptions::new().append(true).open(&file_path)
            .map_err(|e| format!("无法以追加模式打开文件（start_block={}）: {}", start_block, e))?
    };

    log::info!("[TFTP receive_file] 开始: file={}, blksize={}, timeout={}s, rollover={:?}, start_block={}",
        file_path.display(), block_size, params.timeout_secs, params.rollover, start_block);

    let started = std::time::Instant::now();
    // start_block > 1 时 CRC32 由调用方负责（首块已由调用方处理）
    let mut hasher = if start_block <= 1 { Some(crc32fast::Hasher::new()) } else { None };
    let mut expected_block: u16 = start_block.max(1);
    let mut bytes_received: u64 = 0;

    loop {
        if abort.load(Ordering::Relaxed) {
            // 清理不完整文件
            if params.clean_on_error {
                let _ = fs::remove_file(&file_path);
            }
            return Err("传输已取消".into());
        }

        counting.set_read_timeout(timeout)
            .map_err(|e| format!("设置超时失败: {}", e))?;

        match counting.recv_with_size(block_size) {
            Ok(Packet::Data { block_num, data }) => {
                if block_num == expected_block {
                    // 正确块
                    file.write_all(&data).map_err(|e| format!("文件写入失败: {}", e))?;
                    if let Some(ref mut h) = hasher { h.update(&data); }
                    counting.record_data_bytes(data.len() as u64);
                    bytes_received += data.len() as u64;

                    // 发送 ACK（重复发包以提高可靠性）
                    for i in 0..repeat_count {
                        if i > 0 {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        counting.send_to(&Packet::Ack(block_num), &remote)
                            .map_err(|e| format!("ACK 发送失败: {}", e))?;
                    }

                    // 最后一块（DATA 长度 < block_size）
                    if data.len() < block_size {
                        log::info!("[TFTP receive_file] 最后一块 block={}, size={}, bytes_received={}", block_num, data.len(), bytes_received);
                        break;
                    }

                    // Block 号回绕
                    let prev = expected_block;
                    expected_block = expected_block.wrapping_add(1);
                    let after_wrap = expected_block;
                    expected_block = match params.rollover {
                        TftpRollover::None => expected_block, // allow 0
                        TftpRollover::Enforce0 => if expected_block == 0 { 1 } else { expected_block },
                        TftpRollover::Enforce1 => match expected_block { 0 => 2, 1 => 2, n => n },
                        TftpRollover::DontCare => expected_block,
                    };
                    if after_wrap != expected_block {
                        log::info!("[TFTP receive_file] 块号回绕: {} → wrapping={} → {} (policy={:?}), bytes_received={}",
                            prev, after_wrap, expected_block, params.rollover, bytes_received);
                    }
                } else if block_num == expected_block.wrapping_sub(1) {
                    // 重复包（SAS 修复）：重发 ACK
                    for i in 0..repeat_count {
                        if i > 0 {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        counting.send_to(&Packet::Ack(block_num), &remote)
                            .map_err(|e| format!("ACK 重发失败: {}", e))?;
                    }
                } else {
                    // 意外块号 — 忽略
                }
            }
            Ok(Packet::Error { code, msg }) => {
                return Err(format!("远端错误: {} - {}", code, msg));
            }
            Err(_) => {
                // 超时 — 继续等待（超时恢复由发送端负责）
            }
            Ok(_) => {
                // 意外包类型 — 忽略
            }
        }
    }

    file.flush().map_err(|e| format!("文件刷新失败: {}", e))?;

    let duration_ms = started.elapsed().as_millis() as u64;
    Ok(TransferResult {
        bytes_transferred: bytes_received,
        checksum: hasher.map(|h| h.finalize()),
        duration_ms,
    })
}

