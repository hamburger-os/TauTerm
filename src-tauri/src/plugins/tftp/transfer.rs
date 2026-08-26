//! TFTP 传输引擎
//!
//! 实现核心传输循环（send/receive），对齐 `tftpd::Worker` 的设计。
//! 复用 `tftpd::Packet`、`tftpd::Socket`、`tftpd::ErrorCode`、
//! `tftpd::WindowRead`、`tftpd::WindowWrite`。
//!
//! RFC 1350: 基本 send/receive + SAS 修复 — ✅ 已实现
//! RFC 2347/2348/2349: 选项协商 (OACK) — ✅ 已实现（由调用方处理）
//! RFC 7440: 滑动窗口 — ✅ 已实现（WindowRead/WindowWrite）

use std::fs;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tftpd::{ErrorCode, Packet, Socket, WindowRead, WindowWrite};

use super::counting_socket::CountingSocket;
use super::TftpDynamicParams;
use super::TftpRollover;

/// 错误响应包最大字节数（TFTP 头 + 消息最小缓冲）
const MAX_ERROR_PACKET_SIZE: usize = 128;

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
/// 对齐 `tftpd::Worker::send_file` 的 RFC 7440 滑动窗口算法：
/// 1. 使用 `WindowRead` 预读 `windowsize` 块
/// 2. 非阻塞模式发送块并穿插收集 ACK（管道化）
/// 3. 窗口耗尽后切换阻塞模式等待 ACK
/// 4. 单计时器（最新未确认包超时），超时后从窗口起点重发
pub fn send_file(
    counting: &mut CountingSocket,
    file_path: PathBuf,
    remote: SocketAddr,
    params: &TftpDynamicParams,
    abort: &Arc<AtomicBool>,
) -> Result<TransferResult, String> {
    let block_size = counting.blksize();
    let window_size = params.windowsize.max(1);
    let timeout = Duration::from_secs(params.timeout_secs.max(1) as u64);
    let max_retries = params.max_retries.max(1);
    let repeat_count = params.repeat_count.max(1) as usize;
    let window_wait = if params.window_wait > 0 {
        Some(Duration::from_millis(params.window_wait))
    } else {
        None
    };

    let file = fs::File::open(&file_path).map_err(|e| format!("无法打开文件: {}", e))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    counting.set_total_size(file_size);

    log::info!(
        "[TFTP send_file] 开始: file={}, size={} bytes, blksize={}, windowsize={}, timeout={}s, rollover={:?}",
        file_path.display(), file_size, block_size, window_size, params.timeout_secs, params.rollover
    );

    let started = Instant::now();
    let mut hasher = crc32fast::Hasher::new();

    // 空文件快速返回
    if file_size == 0 {
        return Ok(TransferResult {
            bytes_transferred: 0,
            checksum: Some(0),
            duration_ms: 0,
        });
    }

    // ── RFC 7440: 窗口读缓冲 ──
    let mut window = WindowRead::new(window_size, block_size as u16, file);
    let mut more = window.fill().map_err(|e| format!("窗口填充失败: {}", e))?;

    // wr_idx: 窗口内发送偏移（从 0 起）
    // block_seq_win: 窗口基块号（最老未确认块的前一个块号）
    let mut block_seq_win: u16 = 0;
    let mut wr_idx: u16 = 0;
    let mut bytes_sent: u64 = 0;
    let mut retry_cnt = 0;
    let mut timeout_end = Instant::now() + timeout;

    // 设置读超时（对齐 Worker：Windows 额外 +15ms 弥补早期返回）
    #[cfg(windows)]
    counting
        .set_read_timeout(timeout + Duration::from_millis(15))
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    #[cfg(not(windows))]
    counting
        .set_read_timeout(timeout)
        .map_err(|e| format!("设置读超时失败: {}", e))?;

    // 初始非阻塞模式——先管道化发送再收集 ACK
    counting
        .set_nonblocking(true)
        .map_err(|e| format!("设置非阻塞失败: {}", e))?;

    loop {
        if abort.load(Ordering::Relaxed) {
            return Err("传输已取消".into());
        }

        // ── 发送阶段：从窗口发送一个包 ──
        if let Some(frame) = window.get_elements().get(wr_idx as usize) {
            let mut block_seq_tx = block_seq_win.wrapping_add(wr_idx + 1);

            // Block 号回绕处理（对齐 tftpd Worker 语义）
            if block_seq_tx < block_seq_win {
                match params.rollover {
                    TftpRollover::None => {
                        // 禁止回绕 — 块号溢出时报错退出
                        return send_rollover_error(counting, remote, abort);
                    }
                    TftpRollover::Enforce0 | TftpRollover::DontCare => {
                        // 允许回绕，自然使用块号 0
                    }
                    TftpRollover::Enforce1 => {
                        // 跳过块号 0，使用块号 1
                        block_seq_tx = block_seq_tx.wrapping_add(1);
                    }
                }
            }

            hasher.update(frame);
            counting.record_data_bytes(frame.len() as u64);

            let data_pkt = Packet::Data {
                block_num: block_seq_tx,
                data: frame.to_vec(),
            };

            // 带 repeat_count 的发送
            for i in 0..repeat_count {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
                counting
                    .send_to(&data_pkt, &remote)
                    .map_err(|e| format!("发送失败: {}", e))?;
            }

            bytes_sent += frame.len() as u64;
            wr_idx += 1;

            if wr_idx < window.len() {
                // 窗口内有更多数据→可选延迟→进入 ACK 阶段（非阻塞 peek）
                if let Some(wait) = window_wait {
                    std::thread::sleep(wait);
                }
            } else {
                // 窗口已耗尽→检查 abort→预读下批数据→切换阻塞模式等待 ACK
                if abort.load(Ordering::Relaxed) {
                    return Err("传输已取消".into());
                }
                window.prefill().map_err(|e| format!("预读失败: {}", e))?;
                counting
                    .set_nonblocking(false)
                    .map_err(|e| format!("设置阻塞失败: {}", e))?;
            }

            timeout_end = Instant::now() + timeout;
        }

        // ── ACK 收集阶段（非阻塞，排空 socket 缓冲）──
        let mut last_ack: Option<u16> = None;
        loop {
            if abort.load(Ordering::Relaxed) {
                return Err("传输已取消".into());
            }

            match counting.recv() {
                Ok(Packet::Ack(ack_num)) => {
                    if last_ack.is_none() {
                        // 收到第一个 ACK 时切换非阻塞以继续排空后续 ACK
                        counting
                            .set_nonblocking(true)
                            .map_err(|e| format!("设置非阻塞失败: {}", e))?;
                    }
                    last_ack = Some(ack_num);
                    continue; // 继续收集
                }
                Ok(Packet::Error { code, msg }) => {
                    return Err(format!("远端错误: {} - {}", code, msg));
                }
                Ok(_) => {
                    log::warn!("[TFTP send_file] 收到意外包");
                }
                Err(e) => {
                    if let Some(io_e) = e.downcast_ref::<std::io::Error>() {
                        match io_e.kind() {
                            ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                                if let Some(ack) = last_ack {
                                    // 计算窗口推进量（带回绕感知）
                                    let mut diff = ack.wrapping_sub(block_seq_win);
                                    if ack < block_seq_win
                                        && params.rollover == TftpRollover::Enforce1
                                    {
                                        // Enforce1 跳过了块号 0，实际发送量比 diff 少 1
                                        diff = diff.wrapping_sub(1);
                                    }

                                    if diff == 0 {
                                        // 窗口完全确认——回到发送阶段
                                        break;
                                    } else if diff <= window_size {
                                        // 部分确认——滑动窗口
                                        block_seq_win = ack;
                                        window.remove(diff).map_err(|_| "窗口移除超出范围")?;
                                        if !more && window.is_empty() {
                                            let duration_ms = started.elapsed().as_millis() as u64;
                                            return Ok(TransferResult {
                                                bytes_transferred: bytes_sent,
                                                checksum: Some(hasher.finalize()),
                                                duration_ms,
                                            });
                                        }
                                        more = more
                                            && window
                                                .fill()
                                                .map_err(|e| format!("窗口填充失败: {}", e))?;
                                        wr_idx = 0;
                                        break;
                                    } else {
                                        log::warn!(
                                            "[TFTP send_file] 意外 ACK: ack={}, win_base={}",
                                            ack,
                                            block_seq_win
                                        );
                                    }
                                }

                                // 无 ACK、窗口仍有数据、未超时→继续发送
                                if wr_idx < window.len() && Instant::now() < timeout_end {
                                    break;
                                }
                            }
                            ErrorKind::ConnectionReset => {
                                log::warn!("[TFTP send_file] 连接重置");
                            }
                            _ => {
                                log::warn!("[TFTP send_file] IO 错误: {io_e:?}");
                            }
                        }
                    } else {
                        log::warn!("[TFTP send_file] 未知错误: {e:?}");
                    }
                }
            }

            // 超时检查
            if Instant::now() >= timeout_end {
                log::warn!("[TFTP send_file] ACK 超时 {}/{}", retry_cnt, max_retries);
                if retry_cnt >= max_retries {
                    return Err(format!("超过最大重试次数 ({})", max_retries));
                }
                retry_cnt += 1;
                // 指数退避：每次重试等待时间翻倍（上限 30s）
                let backoff = timeout * 2u32.pow(retry_cnt as u32).min(60);
                std::thread::sleep(backoff.min(Duration::from_secs(30)));
                timeout_end = Instant::now() + timeout;
                // 从窗口起点重新发送
                wr_idx = 0;
                counting
                    .set_nonblocking(true)
                    .map_err(|e| format!("设置非阻塞失败: {}", e))?;
                break;
            }
        }
    }
}

/// 发送块号回绕错误（仅 `TftpRollover::None` 触发）并返回 Err
fn send_rollover_error(
    counting: &mut CountingSocket,
    remote: SocketAddr,
    abort: &Arc<AtomicBool>,
) -> Result<TransferResult, String> {
    if !abort.load(Ordering::Relaxed) {
        counting
            .send_to(
                &Packet::Error {
                    code: ErrorCode::IllegalOperation,
                    msg: "Block counter rollover error".to_string(),
                },
                &remote,
            )
            .ok();
    }
    Err("Block counter rollover error".into())
}

/// 接收文件（服务端 WRQ 响应 / 客户端 GET）
///
/// 对齐 `tftpd::Worker::receive_file` 的 RFC 7440 滑坡算法：
/// 1. 使用 `WindowWrite` 缓冲乱序到达的块
/// 2. 收到窗口内任意块立即 ACK 最后连续块号
/// 3. 窗口满或最终块时写盘
/// 4. 块号匹配时才加入窗口，不匹配则 re-ACK 已有的 block_number
///
/// `start_block`: 起始块号，默认 1。当客户端 GET 已在 OACK 协商阶段
/// 处理了 DATA[1] 时传入 2，文件以追加模式打开。
pub fn receive_file(
    counting: &mut CountingSocket,
    file_path: PathBuf,
    remote: SocketAddr,
    params: &TftpDynamicParams,
    abort: &Arc<AtomicBool>,
    start_block: u16,
) -> Result<TransferResult, String> {
    let block_size = counting.blksize();
    let window_size = params.windowsize.max(1);
    let max_retries = params.max_retries.max(1);
    let repeat_count = params.repeat_count.max(1) as usize;
    // 接收缓冲区取错误包和最大数据包中的较大值
    let max_pkt_size = std::cmp::max(MAX_ERROR_PACKET_SIZE, block_size);

    // 根据起始块号选择文件打开模式
    let file = if start_block <= 1 {
        fs::File::create(&file_path).map_err(|e| format!("无法创建文件: {}", e))?
    } else {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .map_err(|e| {
                format!(
                    "无法以追加模式打开文件（start_block={}）: {}",
                    start_block, e
                )
            })?
    };

    log::info!(
        "[TFTP receive_file] 开始: file={}, blksize={}, windowsize={}, timeout={}s, rollover={:?}, start_block={}",
        file_path.display(), block_size, window_size, params.timeout_secs, params.rollover, start_block
    );

    let started = Instant::now();
    let rcv_timeout = Duration::from_secs(params.timeout_secs.max(1) as u64);

    // start_block > 1 时 CRC32 由调用方负责（首块已由调用方处理）
    let mut hasher = if start_block <= 1 {
        Some(crc32fast::Hasher::new())
    } else {
        None
    };

    // ── RFC 7440: 窗口写缓冲 ──
    let mut window = WindowWrite::new(window_size, file);
    // block_number: 最后写入的连续块号（最近发出的 ACK 号）
    let mut block_number: u16 = start_block.wrapping_sub(1);
    let mut bytes_received: u64 = 0;
    let mut retry_cnt = 0;
    let mut last = false;
    let mut listen_all = false;
    let mut send_ack = false;

    // 设置读超时（对齐 send_file：Windows 额外 +15ms 弥补早期返回）
    #[cfg(windows)]
    counting
        .set_read_timeout(rcv_timeout + Duration::from_millis(15))
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    #[cfg(not(windows))]
    counting
        .set_read_timeout(rcv_timeout)
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    // 初始阻塞模式
    counting
        .set_nonblocking(false)
        .map_err(|e| format!("设置阻塞失败: {}", e))?;

    while !last {
        // ── 内层循环：收集 DATA 直到需要发送 ACK ──
        while !send_ack {
            if abort.load(Ordering::Relaxed) {
                if params.clean_on_error {
                    let _ = fs::remove_file(&file_path);
                }
                return Err("传输已取消".into());
            }

            match counting.recv_with_size(max_pkt_size) {
                Ok(Packet::Data {
                    block_num: received_block,
                    data,
                }) => {
                    // 计算期望的下一连续块号
                    let mut expected = block_number.wrapping_add(1);
                    if expected == 0 {
                        // 块号回绕处理（对齐 tftpd Worker 语义）
                        match params.rollover {
                            TftpRollover::None => {
                                // 禁止回绕 — 报错退出
                                if !abort.load(Ordering::Relaxed) {
                                    counting
                                        .send_to(
                                            &Packet::Error {
                                                code: ErrorCode::IllegalOperation,
                                                msg: "Block counter rollover error".to_string(),
                                            },
                                            &remote,
                                        )
                                        .ok();
                                }
                                return Err("Block counter rollover error".into());
                            }
                            TftpRollover::Enforce0 | TftpRollover::DontCare => {
                                // 允许块号 0，自然 wrapping
                            }
                            TftpRollover::Enforce1 => {
                                // 跳过块号 0，期望块号从 1 开始
                                expected = 1;
                                if received_block == 0 {
                                    return Err("Block counter rollover error".into());
                                }
                            }
                        }
                    }

                    if received_block == expected {
                        // ── 顺序块：加入窗口 ──
                        block_number = received_block;
                        last = data.len() < block_size;
                        window.add(data.clone()).map_err(|_| "窗口已满")?;
                        if let Some(ref mut h) = hasher {
                            h.update(&data);
                        }
                        counting.record_data_bytes(data.len() as u64);
                        bytes_received += data.len() as u64;
                        // 窗口满或最后一块时发送 ACK
                        send_ack = window.is_full() || last;
                    } else {
                        // ── 乱序 / 重复块 ──
                        log::debug!(
                            "[TFTP receive_file] 块号不匹配: 收到 {}，期望 {}",
                            received_block,
                            expected
                        );
                        send_ack = true; // 重发 ACK 通知发送端当前进度
                    }

                    // 收到数据后切换非阻塞以排空后续包
                    counting
                        .set_nonblocking(true)
                        .map_err(|e| format!("设置非阻塞失败: {}", e))?;
                    listen_all = true;
                }
                Ok(Packet::Error { code, msg }) => {
                    return Err(format!("远端错误: {} - {}", code, msg));
                }
                Ok(_) => {
                    log::warn!("[TFTP receive_file] 收到意外包");
                }
                Err(e) => {
                    if let Some(io_e) = e.downcast_ref::<std::io::Error>() {
                        match io_e.kind() {
                            ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                                if listen_all {
                                    // 排空完成，切回阻塞模式等待新数据
                                    counting
                                        .set_nonblocking(false)
                                        .map_err(|e| format!("设置阻塞失败: {}", e))?;
                                    listen_all = false;
                                } else {
                                    // 阻塞模式下超时——重试
                                    log::debug!(
                                        "[TFTP receive_file] ACK 超时 {}/{}",
                                        retry_cnt,
                                        max_retries
                                    );
                                    if retry_cnt >= max_retries {
                                        return Err(format!("超过最大重试次数 ({})", max_retries));
                                    }
                                    retry_cnt += 1;
                                    send_ack = true;
                                }
                            }
                            ErrorKind::ConnectionReset => {
                                log::warn!("[TFTP receive_file] 连接重置");
                                counting
                                    .set_nonblocking(false)
                                    .map_err(|e| format!("设置阻塞失败: {}", e))?;
                            }
                            _ => {
                                log::warn!("[TFTP receive_file] IO 错误: {io_e:?}");
                            }
                        }
                    } else {
                        log::warn!("[TFTP receive_file] 未知错误: {e:?}");
                    }
                }
            }
        }

        // ── 发送 ACK（重复以提高可靠性）──
        for i in 0..repeat_count {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
            counting
                .send_to(&Packet::Ack(block_number), &remote)
                .map_err(|e| format!("ACK 发送失败: {}", e))?;
        }
        send_ack = false;

        // ── 窗口写盘 ──
        window.empty().map_err(|e| format!("窗口写盘失败: {}", e))?;
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    Ok(TransferResult {
        bytes_transferred: bytes_received,
        checksum: hasher.map(|h| h.finalize()),
        duration_ms,
    })
}
