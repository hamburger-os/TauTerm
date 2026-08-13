//! iperf2 TCP 数据流（对齐 2.2.1 源码，单连接模型）
//!
//! TCP：客户端连接服务器端口后，**首个载荷即为 64B 测试头**（`client_hdr_v1` +
//! `client_hdrext`），之后数据在同一连接上传输——不存在独立的数据端口。
//! 并行流（-P N）为 N 条连接全部连同一端口，每条各自携带 64B 首包。
//! 发送侧线程持续写固定模式数据，主线程按 -i 间隔快照共享字节计数器并**实时**
//! 上报区间。结束：客户端 `shutdown(SHUT_WR)` 发 FIN，服务器 recv 返回 0 视为结束。
//!
//! 双向模式（对齐 2.2.1）：
//! - `-r` TradeOff：同一连接顺序反向——客户端发完 `shutdown(WR)` 后原地接收，
//!   服务端读到 EOF 后在同一 socket 回发（时长 = 声明时长 + 2s slop）
//! - `-d` DualTest：双连接——客户端先起本地监听（端口写入头部 `mPort`，flags
//!   带 `RUN_NOW`），服务端收到头后反向 connect 回客户端端口，第二条连接回发；
//!   两连接同时传输即双向同时测吞吐

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::control::{client_handshake, server_handshake};
use super::stats::SharedByteCounter;
use super::types::{Iperf2Interval, Iperf2TestParams, TestDirection};
use crate::plugins::iperf::IperfDirection;

/// 发送缓冲大小（对齐 kDefault_TCPBufLen = 128KB）
const SEND_BUF_SIZE: usize = 128 * 1024;
/// 接收缓冲大小
const RECV_BUF_SIZE: usize = 64 * 1024;
/// 发送超时（SO_SNDTIMEO，对齐真实 iperf2 的 ≤1s 封顶：对端停读/缓冲满时
/// 写调用有界返回，线程可及时退出，join 不会等满 10s 兜底）
const SEND_IO_TIMEOUT: Duration = Duration::from_secs(1);
/// 接收超时（SO_RCVTIMEO，对齐真实 iperf2 的半区间级轮询粒度；保证停止有界）
const DATA_IO_TIMEOUT: Duration = Duration::from_secs(5);
/// -d/-r 反向测试宽限（对齐 2.2.1 SLOPSECS = 2 秒）
pub const REVERSE_SLOPSECS: u32 = 2;
/// 反向相接收 deadline 附加余量（吸收对端启动/网络延迟；真实 iperf2 无显式
/// deadline，靠 slop 与对端 close 界定——此处防御挂死对端）
const REVERSE_DEADLINE_MARGIN: Duration = Duration::from_secs(10);

/// TCP 测试统计结果（fwd = 本端发送方向；rev = 双向模式下的接收方向）
pub struct TcpClientStats {
    pub bytes_sent: u64,
    pub intervals: Vec<Iperf2Interval>,
    /// -d/-r 反向接收统计（Normal 为 None）
    pub rev_bytes_received: u64,
    pub rev_intervals: Vec<Iperf2Interval>,
    /// 是否发生了反向相（-d/-r 恒 true；用于 done 事件是否补发 rev）
    pub rev_active: bool,
}

/// 客户端 TCP 发送（上行测速；-d/-r 时附带反向接收相）
///
/// 每条流独立连接服务器端口（握手内含 64B 测试头 + ack 读取），
/// 主线程计时并实时上报区间（`emit` 在每次区间边界调用，带方向）。
pub fn run_tcp_client(
    server_addr: &str,
    params: &Iperf2TestParams,
    abort: &Arc<AtomicBool>,
    emit: &dyn Fn(IperfDirection, &Iperf2Interval),
) -> Result<TcpClientStats, String> {
    let direction = params.direction;
    let bidirectional = direction.is_bidirectional();
    let counter = Arc::new(SharedByteCounter::new());
    let rev_counter = Arc::new(SharedByteCounter::new());
    let connect_failures = Arc::new(AtomicUsize::new(0));
    let n_streams = params.parallel_streams.max(1) as usize;

    // -d：先绑定反向监听端口（端口号写入测试头 mPort；对齐 2.2.1 客户端
    // Settings_GenerateListenerSettings——监听先于客户端启动）
    let reverse_listener: Option<(TcpListener, u16)> = if direction == TestDirection::DualTest {
        let listener = TcpListener::bind("0.0.0.0:0")
            .map_err(|e| format!("反向监听绑定失败: {}", e))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("读取反向监听端口失败: {}", e))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("设置反向监听非阻塞失败: {}", e))?;
        log::info!("[iperf2] -d 反向监听已启动 (port={})", port);
        Some((listener, port))
    } else {
        None
    };
    let listen_port = reverse_listener.as_ref().map(|(_, p)| *p);

    // -d 反向接待线程：accept 服务端回连（每条连接先握手再接收，计入 rev 计数）
    let mut rev_handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
    if let Some((listener, _)) = reverse_listener.as_ref() {
        let listener = listener.try_clone().map_err(|e| format!("克隆反向监听失败: {}", e))?;
        let rev_counter = rev_counter.clone();
        let abort = abort.clone();
        let duration = params.duration_secs.max(1);
        rev_handles.push(std::thread::spawn(move || {
            accept_reverse_connections(&listener, duration, &abort, &rev_counter);
        }));
    }

    let mut handles = Vec::new();
    for _ in 0..n_streams {
        let addr = server_addr.to_string();
        let p = params.clone();
        let abort = abort.clone();
        let counter = counter.clone();
        let failures = connect_failures.clone();
        let rev_counter = rev_counter.clone();
        handles.push(std::thread::spawn(move || {
            // 连接 + 64B 测试头 + ack（握手即数据连接的开始）
            let mut stream = match client_handshake(&addr, &p, listen_port, &abort) {
                Ok(s) => s,
                Err(e) => {
                    failures.fetch_add(1, Ordering::Relaxed);
                    log::warn!("[iperf2] 数据流握手失败 {}: {}", addr, e);
                    return;
                }
            };
            let _ = stream.set_nodelay(true);
            // 阻塞 socket + SO_SNDTIMEO（对齐真实 iperf2：Winsock 遵守该超时，
            // 对端停读/缓冲满时写调用在 SEND_IO_TIMEOUT 内有界返回，发送线程
            // 必然退出，不会泄漏）
            let _ = stream.set_write_timeout(Some(SEND_IO_TIMEOUT));
            // 测试头已在握手中发送；此后为纯数据流。
            // 双向模式发送相以 fwd 时长界定（主循环需继续覆盖反向相）；
            // Normal 由主循环结束时置 abort 界定
            let fwd_deadline = if bidirectional {
                Some(Instant::now() + Duration::from_secs(p.duration_secs.max(1) as u64))
            } else {
                None
            };
            send_loop(&mut stream, fwd_deadline, &abort, &counter);
            // 正常结束：shutdown(SHUT_WR) 发 FIN，服务器收 EOF 后打印汇总
            let _ = stream.shutdown(Shutdown::Write);
            // -r：同一连接反向接收（服务端读 EOF 后回发；deadline 防御挂死）
            if direction == TestDirection::TradeOff {
                let deadline =
                    Instant::now() + Duration::from_secs(p.duration_secs.max(1) as u64)
                        + Duration::from_secs(REVERSE_SLOPSECS as u64)
                        + REVERSE_DEADLINE_MARGIN;
                let _ = recv_tcp_stream(stream, &abort, &abort, &rev_counter, Some(deadline));
            }
        }));
    }

    // 主线程：计时 + 区间实时上报（fwd + rev 两相）
    let mut intervals = Vec::new();
    let mut rev_intervals = Vec::new();
    let start = Instant::now();
    let fwd_secs = params.duration_secs.max(1) as f64;
    // 双向模式主循环覆盖反向相（fwd + slop + 余量），否则仅 fwd
    let total_secs = if bidirectional {
        fwd_secs + REVERSE_SLOPSECS as f64 + REVERSE_DEADLINE_MARGIN.as_secs_f64()
    } else {
        fwd_secs
    };
    let interval_secs = params.report_interval_secs.max(1) as f64;

    let mut prev_bytes = 0u64;
    let mut rev_prev_bytes = 0u64;
    let mut next_report_secs = interval_secs;
    // 双向模式提前退出跟踪：fwd 相结束后 rev 字节 2s 无增长 → 反向相已完成
    //（-d/-r 任务时长 = ~2×duration + 2s，而非恒定吃到 deadline 余量）
    let mut stall_bytes = 0u64;
    let mut last_rev_change = start;
    loop {
        let elapsed = start.elapsed();
        if elapsed >= Duration::from_secs_f64(total_secs) || abort.load(Ordering::Relaxed) {
            break;
        }
        // 全部流连接失败 → 提前结束（避免 0 bps 空跑）
        if connect_failures.load(Ordering::Relaxed) >= n_streams {
            log::warn!("[iperf2] 所有数据流连接失败，提前结束");
            break;
        }
        if bidirectional {
            let rev_total = rev_counter.total();
            if rev_total != stall_bytes {
                stall_bytes = rev_total;
                last_rev_change = Instant::now();
            }
            if elapsed >= Duration::from_secs_f64(fwd_secs)
                && stall_bytes > 0
                && last_rev_change.elapsed() >= Duration::from_secs(2)
            {
                log::debug!("[iperf2] 反向相已完成，提前结束主循环");
                break;
            }
        }
        let elapsed_secs = elapsed.as_secs_f64();
        if elapsed_secs >= next_report_secs {
            // fwd 区间（发送相结束后字节冻结，自然为 0）
            let total = counter.total();
            if elapsed_secs <= fwd_secs + interval_secs {
                let interval = Iperf2Interval {
                    start_secs: next_report_secs - interval_secs,
                    end_secs: next_report_secs,
                    transferred_bytes: total - prev_bytes,
                    bandwidth_bps: (total - prev_bytes) as f64 * 8.0 / interval_secs,
                    jitter_ms: None,
                    lost_packets: None,
                    total_packets: None,
                    lost_percent: None,
                };
                emit(IperfDirection::Fwd, &interval);
                intervals.push(interval);
                prev_bytes = total;
            }
            // rev 区间（-d/-r）
            if bidirectional {
                let rev_total = rev_counter.total();
                let interval = Iperf2Interval {
                    start_secs: next_report_secs - interval_secs,
                    end_secs: next_report_secs,
                    transferred_bytes: rev_total - rev_prev_bytes,
                    bandwidth_bps: (rev_total - rev_prev_bytes) as f64 * 8.0 / interval_secs,
                    jitter_ms: None,
                    lost_packets: None,
                    total_packets: None,
                    lost_percent: None,
                };
                emit(IperfDirection::Rev, &interval);
                rev_intervals.push(interval);
                rev_prev_bytes = rev_total;
            }
            next_report_secs += interval_secs;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let all_failed = connect_failures.load(Ordering::Relaxed) >= n_streams;

    // 置 abort 让发送/接收线程退出（用户中止时 abort 已由调用方置位；正常结束时由我们置位）
    abort.store(true, Ordering::Relaxed);
    // 有界等待：发送线程受 SO_SNDTIMEO(1s) 约束必然退出；join 超时仅为
    // 极端情况（超时未被 OS 遵守等）的最终兜底——正常路径不会走到放弃
    super::join_handles_with_timeout(&handles, super::ENGINE_JOIN_TIMEOUT, "TCP 发送");
    super::join_handles_with_timeout(&rev_handles, super::ENGINE_JOIN_TIMEOUT, "TCP 反向接待");

    if all_failed {
        return Err(format!(
            "无法连接 iperf2 服务端 {}（{} 条流全部失败）",
            server_addr, n_streams
        ));
    }

    let bytes_sent = counter.total();
    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    // 收尾区间（最后一段未达一个完整区间时补发）
    let last_end = intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
    if intervals.is_empty() || last_end < elapsed - 0.5 {
        let interval = Iperf2Interval {
            start_secs: last_end,
            end_secs: elapsed,
            transferred_bytes: bytes_sent - prev_bytes,
            bandwidth_bps: (bytes_sent - prev_bytes) as f64 * 8.0 / (elapsed - last_end).max(0.001),
            jitter_ms: None,
            lost_packets: None,
            total_packets: None,
            lost_percent: None,
        };
        emit(IperfDirection::Fwd, &interval);
        intervals.push(interval);
    }
    // rev 收尾区间（双向模式恒补发——即使 0 字节，前端状态机按参数期待 rev done）
    let rev_bytes = rev_counter.total();
    let rev_last_end = rev_intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
    if bidirectional && (rev_intervals.is_empty() || rev_last_end < elapsed - 0.5) {
        let interval = Iperf2Interval {
            start_secs: rev_last_end,
            end_secs: elapsed,
            transferred_bytes: rev_bytes - rev_prev_bytes,
            bandwidth_bps: (rev_bytes - rev_prev_bytes) as f64 * 8.0
                / (elapsed - rev_last_end).max(0.001),
            jitter_ms: None,
            lost_packets: None,
            total_packets: None,
            lost_percent: None,
        };
        emit(IperfDirection::Rev, &interval);
        rev_intervals.push(interval);
    }

    log::info!(
        "[iperf2] TCP 客户端发送完成: {} bytes (fwd) / {} bytes (rev)",
        bytes_sent,
        rev_bytes
    );
    Ok(TcpClientStats {
        bytes_sent,
        intervals,
        rev_bytes_received: rev_bytes,
        rev_intervals,
        rev_active: bidirectional,
    })
}

/// 发送循环（客户端 fwd 与服务端 rev 共用）：滚动 0x00..0xFF 填充模式，
/// 直至 deadline（None = 直到 abort）或 abort。
///
/// 写超时（SO_SNDTIMEO 触发/对端停读）视为异常信号退出循环。
fn send_loop(
    stream: &mut TcpStream,
    deadline: Option<Instant>,
    abort: &Arc<AtomicBool>,
    counter: &Arc<SharedByteCounter>,
) {
    let mut buf = vec![0u8; SEND_BUF_SIZE];
    let mut pos: u64 = 0;
    while !abort.load(Ordering::Relaxed) && deadline.is_none_or(|d| Instant::now() < d) {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = pos.wrapping_add(i as u64) as u8;
        }
        match stream.write(&buf) {
            Ok(0) => {
                // 部分平台写超时返回 0——重试而非忙转（对齐上游 writen 的
                // case-0 重试语义）
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(n) => {
                pos = pos.wrapping_add(n as u64);
                counter.add(n as u64);
            }
            Err(e) => {
                // 写超时是异常信号(SO_SNDTIMEO 触发/对端停读);其余错误
                // (连接重置等)在测试结束时属正常,降为 debug
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock
                {
                    log::warn!("[iperf2] 数据流写超时(对端停读/缓冲满): {}", e);
                } else {
                    log::debug!("[iperf2] 数据流发送结束: {}", e);
                }
                break;
            }
        }
    }
}

/// 服务端反向发送（-d 反向连接 / -r 同 socket 回发）。
///
/// `header`：新连接（-d 反向）先发 64B 测试头（对齐真实 iperf2 反向客户端的
/// SendFirstPayload）；同 socket 回发（-r）传 None（连接首包已含测试头）。
pub fn send_tcp_stream(
    mut stream: TcpStream,
    duration_secs: u32,
    header: Option<&[u8]>,
    abort: &Arc<AtomicBool>,
    counter: &Arc<SharedByteCounter>,
) -> Result<(), String> {
    let _ = stream.set_nodelay(true);
    if let Some(h) = header {
        stream
            .set_write_timeout(Some(SEND_IO_TIMEOUT))
            .map_err(|e| format!("设置写超时失败: {}", e))?;
        stream
            .write_all(h)
            .map_err(|e| format!("发送反向测试头失败: {}", e))?;
    }
    let _ = stream.set_write_timeout(Some(SEND_IO_TIMEOUT));
    send_loop(
        &mut stream,
        Some(Instant::now() + Duration::from_secs(duration_secs.max(1) as u64)),
        abort,
        counter,
    );
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

/// -d 反向接待线程：accept 服务端回连（每条连接握手后接收，计入 rev 计数）。
///
/// 非阻塞 accept + 每连接一个处理线程；abort 置位时退出（残留处理线程由
/// recv 超时/abort 退出，主线程 join 有界等待）。
fn accept_reverse_connections(
    listener: &TcpListener,
    duration_secs: u32,
    abort: &Arc<AtomicBool>,
    rev_counter: &Arc<SharedByteCounter>,
) {
    let mut handlers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    while !abort.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let rev_counter = rev_counter.clone();
                let abort = abort.clone();
                handlers.push(std::thread::spawn(move || {
                    let deadline = Instant::now()
                        + Duration::from_secs(duration_secs.max(1) as u64)
                        + Duration::from_secs(REVERSE_SLOPSECS as u64)
                        + REVERSE_DEADLINE_MARGIN;
                    // 反向连接同样以 64B 测试头开头（对端反向客户端发送）
                    if let Err(e) = server_handshake(&mut stream, &abort) {
                        log::debug!("[iperf2] 反向连接握手失败: {}", e);
                        return;
                    }
                    let _ = recv_tcp_stream(stream, &abort, &abort, &rev_counter, Some(deadline));
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    super::join_handles_with_timeout(&handlers, super::ENGINE_JOIN_TIMEOUT, "反向连接处理");
}

/// 服务端单流接收循环（连接已由调用方完成握手；字节计入共享计数器）。
///
/// EOF（客户端 shutdown/close）、任一 abort 置位或 deadline 到期时退出。
pub fn recv_tcp_stream(
    mut stream: TcpStream,
    abort: &Arc<AtomicBool>,
    test_abort: &Arc<AtomicBool>,
    counter: &Arc<SharedByteCounter>,
    deadline: Option<Instant>,
) -> Result<u64, String> {
    stream
        .set_read_timeout(Some(DATA_IO_TIMEOUT))
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    let mut buf = vec![0u8; RECV_BUF_SIZE];
    let mut total = 0u64;
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break, // 客户端 shutdown(SHUT_WR)/close → EOF
            Ok(n) => {
                let n = n as u64;
                counter.add(n);
                total += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // 客户端停发但连接存活：由会话级 force-end 置 test_abort 结束；
                // 反向接收相由 deadline 界定（防御对端不 close 的挂死）
                if abort.load(Ordering::Relaxed)
                    || test_abort.load(Ordering::Relaxed)
                    || deadline.is_some_and(|d| Instant::now() >= d)
                {
                    break;
                }
            }
            Err(e) => {
                log::debug!("[iperf2] 数据流接收失败: {}", e);
                break;
            }
        }
    }
    Ok(total)
}
