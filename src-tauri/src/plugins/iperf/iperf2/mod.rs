//! iperf2 协议自研实现（对齐 2.2.1 源码，单连接模型）
//!
//! 覆盖：TCP/UDP 单向测速（客户端+服务端）、-t/-b/-u/-p/-P/-i/-w、-d（双连接
//! 双向同时）、-r（同连接顺序反向）、汇总统计（带宽/抖动/丢包）。
//!
//! 协议要点（依据 iperf 2.2.1 官方源码）：
//! - TCP：客户端在服务器端口发 64B 测试头（v1+extend，flags=0x4001_0080），
//!   数据同连接传输；服务器回 28B `client_hdr_ack`；结束客户端 `shutdown(SHUT_WR)`
//! - UDP：无 TCP 控制连接，全部走单个 UDP socket（发往服务器端口，无 +1）；
//!   每包 = `UDP_datagram`(16B) + 测试头(64B) + 载荷，序号从 1 开始；
//!   结束发负序号 FIN，服务器回 `[UDP_datagram 全零 + server_hdr(40B)]`
//! - 无统计结构交换（TCP 以数据连接 EOF 结束；UDP 的 jitter/loss 来自 server_hdr）
//! - `-r` TradeOff：头部 flags 带 VERSION1；服务端读 EOF 后同 socket 回发
//! - `-d` DualTest：头部 flags 带 VERSION1+RUN_NOW、mPort=客户端反向监听端口；
//!   服务端反向 connect 回客户端端口回发，两连接同时传输

mod control;
mod data_tcp;
mod data_udp;
mod stats;
mod test_hdr;
mod types;

use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};
use tauri::Emitter;

use crate::plugins::iperf::{
    IperfClientResult, IperfConfig, IperfDirection, IperfDynamicParams, IperfIntervalReport,
    IperfProtocol, IperfRole, IperfSummary, IperfVersion,
};

use control::server_handshake;
use data_tcp::{recv_tcp_stream, send_tcp_stream, REVERSE_SLOPSECS};
use test_hdr::{tcp_first_payload, ClientHdrExt, ClientHdrV1};
use types::{Iperf2Interval, Iperf2TestParams, ServerTestMode, TestDirection, TestMode};

/// 服务端区间默认间隔（真实服务器 -i 默认 1s）
const SERVER_INTERVAL_SECS: f64 = 1.0;
/// 服务端 force-end 宽限（客户端超时后仍不结束时的兜底）
const SERVER_FORCE_END_GRACE: Duration = Duration::from_secs(15);
/// force-end 时长上限：客户端头声明的时长 clamp 到 1 小时——恶意/畸形客户端
/// 可声明任意时长（头部字段来自网络），无上限会让每连接线程被无限期滞留
const SERVER_FORCE_END_MAX_SECS: f64 = 3600.0;
/// 引擎线程 join 超时：发送/接收线程阻塞（如 Windows SO_SNDTIMEO 失效导致
/// write 无限阻塞）时放弃等待，保证任务必然返回、done 事件必然发出
const ENGINE_JOIN_TIMEOUT: Duration = Duration::from_secs(10);

// 锁中毒恢复：统一出口提升至父模块（iperf::lock_or_recover），
// 供全插件（mod/server/client/commands 调用点）复用
use super::lock_or_recover;

/// 有界等待线程集合结束：轮询 `is_finished`，超时后放弃。
///
/// 放弃后线程可能泄漏（进程退出时回收），但调用方任务必然返回——
/// 这是"测试不结束/停止无效"的兜底（阻塞中的 write 无法被 abort 打断）。
/// 返回是否全部正常结束。
fn join_handles_with_timeout(
    handles: &[std::thread::JoinHandle<()>],
    timeout: Duration,
    what: &str,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if handles.iter().all(|h| h.is_finished()) {
            return true;
        }
        if Instant::now() >= deadline {
            log::warn!(
                "[iperf2] {} 线程等待超时 ({}s)，放弃 join（任务照常返回）",
                what,
                timeout.as_secs()
            );
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 转换动态参数为 iperf2 引擎参数（顺带 clamp 防御：并行流数决定线程数、
/// 时长决定 force-end 窗口，上限防本地资源耗尽）
fn to_test_params(params: &IperfDynamicParams) -> Iperf2TestParams {
    Iperf2TestParams {
        mode: match params.protocol {
            IperfProtocol::Tcp => TestMode::Tcp,
            IperfProtocol::Udp => TestMode::Udp,
        },
        duration_secs: params.duration_secs.clamp(1, 86_400),
        parallel_streams: params.parallel_streams.clamp(1, 64),
        bandwidth_bps: params.bandwidth_bps,
        window_size: params.window_size,
        report_interval_secs: params.report_interval_secs.max(1),
        port: params.port,
        // -d/-r 同真时取 tradeoff（对齐 2.2.1：两者取后者语义，前端已互斥）
        direction: if params.tradeoff {
            TestDirection::TradeOff
        } else if params.bidirectional {
            TestDirection::DualTest
        } else {
            TestDirection::Normal
        },
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LossStats {
    jitter_ms: Option<f64>,
    lost_packets: Option<u64>,
    total_packets: Option<u64>,
    lost_percent: Option<f64>,
}

struct ReverseSenderSignals<'a> {
    abort: &'a Arc<AtomicBool>,
    test_running: &'a Arc<AtomicBool>,
    last_summary: &'a Arc<Mutex<Option<IperfSummary>>>,
}

/// 构造 iperf2 汇总（fwd/rev 共用；UDP 抖动/丢包由调用方传入）
fn build_summary(
    role: IperfRole,
    protocol: IperfProtocol,
    duration_secs: f64,
    total_bytes: u64,
    intervals: Vec<Iperf2Interval>,
    loss: LossStats,
) -> IperfSummary {
    let LossStats {
        jitter_ms,
        lost_packets,
        total_packets,
        lost_percent,
    } = loss;
    IperfSummary {
        version: IperfVersion::Iperf2,
        role,
        protocol,
        duration_secs,
        total_bytes,
        avg_bandwidth_bps: if duration_secs > 0.0 {
            total_bytes as f64 * 8.0 / duration_secs
        } else {
            0.0
        },
        intervals: intervals
            .into_iter()
            .map(|i| IperfIntervalReport {
                start_secs: i.start_secs,
                end_secs: i.end_secs,
                transferred_bytes: i.transferred_bytes,
                bandwidth_bps: i.bandwidth_bps,
                jitter_ms: i.jitter_ms,
                lost_packets: i.lost_packets,
                total_packets: i.total_packets,
                lost_percent: i.lost_percent,
            })
            .collect(),
        jitter_ms,
        lost_packets,
        total_packets,
        lost_percent,
    }
}

/// 发射单个区间事件（供客户端/服务端引擎实时调用）。
///
/// 事件携带 `role` + `direction` + `protocol` 供前端按角色/方向路由——
/// 自测模式（软件同时作两端）下客户端与服务端的区间上报都经此发射；
/// 双向测试（-d/-r）下 fwd/rev 两条流并发，无方向标识会互相劫持记录。
pub(super) fn emit_interval<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    role: IperfRole,
    direction: IperfDirection,
    protocol: IperfProtocol,
    r: &Iperf2Interval,
) {
    let _ = app.emit(
        "iperf-interval-report",
        serde_json::json!({
            "session_id": session_id,
            "role": role,
            "direction": direction,
            "protocol": protocol,
            "start_secs": r.start_secs,
            "end_secs": r.end_secs,
            "transferred_bytes": r.transferred_bytes,
            "bandwidth_bps": r.bandwidth_bps,
            "jitter_ms": r.jitter_ms,
            "lost_packets": r.lost_packets,
            "total_packets": r.total_packets,
            "lost_percent": r.lost_percent,
        }),
    );
}

/// 区间实时上报（带 seq 配对键：UDP 服务端多路复用下并发记录按 seq 归位）
pub fn emit_interval_seq<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    role: IperfRole,
    direction: IperfDirection,
    protocol: IperfProtocol,
    r: &Iperf2Interval,
    seq: u64,
) {
    let _ = app.emit(
        "iperf-interval-report",
        serde_json::json!({
            "session_id": session_id,
            "role": role,
            "direction": direction,
            "protocol": protocol,
            "seq": seq,
            "start_secs": r.start_secs,
            "end_secs": r.end_secs,
            "transferred_bytes": r.transferred_bytes,
            "bandwidth_bps": r.bandwidth_bps,
            "jitter_ms": r.jitter_ms,
            "lost_packets": r.lost_packets,
            "total_packets": r.total_packets,
            "lost_percent": r.lost_percent,
        }),
    );
}

// ── 客户端引擎 ─────────────────────────────────────────

/// iperf2 客户端测速（单向上行；TCP/UDP 均发往服务器端口；-d/-r 附带反向相）。
pub fn run_client<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    target_host: &str,
    params: &IperfDynamicParams,
    abort_flag: &Arc<AtomicBool>,
) -> Result<IperfClientResult, String> {
    let p = to_test_params(params);
    if p.direction.is_bidirectional() && p.mode.is_udp() {
        return Err("iperf2 -d/-r 当前仅支持 TCP".into());
    }
    let server_addr = format_host_port(target_host, p.port);
    let protocol = p.mode.to_iperf_protocol();
    let emit = |dir: IperfDirection, r: &Iperf2Interval| {
        emit_interval(app, session_id, IperfRole::Client, dir, protocol, r)
    };

    let (fwd, rev, warning) = match p.mode {
        TestMode::Tcp => {
            let s = data_tcp::run_tcp_client(&server_addr, &p, abort_flag, &emit)?;
            let fwd = build_summary(
                IperfRole::Client,
                protocol,
                p.duration_secs as f64,
                s.bytes_sent,
                s.intervals,
                LossStats {
                    jitter_ms: None,
                    lost_packets: None,
                    total_packets: None,
                    lost_percent: None,
                },
            );
            let rev = if s.rev_active {
                let rev_duration = s.rev_intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
                Some(build_summary(
                    IperfRole::Client,
                    protocol,
                    rev_duration,
                    s.rev_bytes_received,
                    s.rev_intervals,
                    LossStats {
                        jitter_ms: None,
                        lost_packets: None,
                        total_packets: None,
                        lost_percent: None,
                    },
                ))
            } else {
                None
            };
            (fwd, rev, None)
        }
        TestMode::Udp => {
            let s = data_udp::run_udp_client(&server_addr, &p, abort_flag, &emit)?;
            // UDP 无连接检测：服务器回报超时 → 目标可能不可达，"满速"实为零接收
            let warning = (s.server_report_received == Some(false))
                .then(|| "未收到服务端统计回报（目标可能不可达，带宽按发送字节计算）".to_string());
            (
                build_summary(
                    IperfRole::Client,
                    protocol,
                    p.duration_secs as f64,
                    s.bytes_sent,
                    s.intervals,
                    LossStats {
                        jitter_ms: s.jitter_ms,
                        lost_packets: s.lost_packets,
                        total_packets: s.total_packets,
                        lost_percent: s.lost_percent,
                    },
                ),
                None,
                warning,
            )
        }
    };

    Ok(IperfClientResult { fwd, rev, warning })
}

// ── 服务端引擎 ─────────────────────────────────────────

/// 会话级 TCP 测试状态（同一客户端 -P N 连接共享，SUM 聚合）
struct TcpSession {
    /// fwd 接收字节计数
    counter: Arc<stats::SharedByteCounter>,
    /// rev 发送字节计数（-d 反向连接 / -r 同 socket 回发）
    send_counter: Arc<stats::SharedByteCounter>,
    /// 当前活动 fwd 流数（握手完成计 +1，接收结束计 -1）
    streams: usize,
    /// 当前活动 rev 流数（-d 反向发送线程 / -r 回发相；结束计 -1）
    rev_streams: usize,
    /// 首条流的测试头（时长提示、-d 反向端口与线程数）
    header: Option<ClientHdrV1>,
    /// 接待模式（普通 / -r / -d）
    mode: ServerTestMode,
    /// 首条流对端地址（-d 反向 connect 目标）
    peer_addr: Option<SocketAddr>,
    intervals: Vec<Iperf2Interval>,
    rev_intervals: Vec<Iperf2Interval>,
    started: bool,
    start: Option<Instant>,
    /// rev 相起点（-d 首流到达；-r 首个回发相开始——区间时钟相对此点）
    rev_start: Option<Instant>,
    next_report_secs: f64,
    rev_next_report_secs: f64,
    prev_bytes: u64,
    rev_prev_bytes: u64,
    /// rev 相进行中（-r 为回发期间；-d 为发送线程存活期间）
    rev_active: bool,
    /// amount 模式（非 time）force-end 进度跟踪
    last_progress: Instant,
    last_progress_bytes: u64,
    /// force-end 标志（超时兜底，接收线程检测后退出）
    test_abort: Arc<AtomicBool>,
    /// UDP 引擎是否在接待测试（test_running 派生源之一：
    /// TCP started 与 UDP udp_active 的 OR，读写均在 TcpSession 锁内）
    udp_active: bool,
}

impl TcpSession {
    fn new() -> Self {
        Self {
            counter: Arc::new(stats::SharedByteCounter::default()),
            send_counter: Arc::new(stats::SharedByteCounter::default()),
            streams: 0,
            rev_streams: 0,
            header: None,
            mode: ServerTestMode::Normal,
            peer_addr: None,
            intervals: Vec::new(),
            rev_intervals: Vec::new(),
            started: false,
            start: None,
            rev_start: None,
            next_report_secs: SERVER_INTERVAL_SECS,
            rev_next_report_secs: SERVER_INTERVAL_SECS,
            prev_bytes: 0,
            rev_prev_bytes: 0,
            rev_active: false,
            last_progress: Instant::now(),
            last_progress_bytes: 0,
            test_abort: Arc::new(AtomicBool::new(false)),
            udp_active: false,
        }
    }
}

/// iperf2 服务端监听（单监听器：TCP 监听 + UDP socket 同端口）。
///
/// - TCP：每个已接受连接一个处理线程（握手 → 同连接接收）；并行流（-P N）
///   连接共享会话计数器，会话级 SUM 区间实时上报，末条流结束时收尾
/// - UDP：单 socket 串行接待（对齐真实 udp_accept 行为）
/// - -w：listen/bind 前设 SO_RCVBUF（accept 出的连接继承监听 socket 缓冲）
///
/// 解析"IP:端口"（IPv6 支持：裸 ::1 自动补括号为 [::1]:port）
fn parse_host_port(ip: &str, port: u16) -> Result<SocketAddr, String> {
    if let Ok(addr) = format!("{}:{}", ip, port).parse::<SocketAddr>() {
        return Ok(addr);
    }
    let bracketed = if ip.starts_with('[') && ip.ends_with(']') {
        format!("{}:{}", ip, port)
    } else {
        format!("[{}]:{}", ip, port)
    };
    bracketed
        .parse::<SocketAddr>()
        .map_err(|e| format!("监听地址无效: {}", e))
}

/// 格式化"主机:端口"（IPv6 字面量加括号，保证 ToSocketAddrs 可解析）
fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

/// 服务端并发 TCP 连接上限（对齐客户端 -P 64 上限；防外部主机
/// 无界连接耗尽本机线程/内存）
const MAX_CONCURRENT_TCP_HANDLERS: usize = 64;

/// 返回监听地址字符串（正常停止时）；abort 中止视为正常停止。
pub fn run_server<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    config: &IperfConfig,
    dynamic_params: &Arc<Mutex<IperfDynamicParams>>,
    abort_flag: &Arc<AtomicBool>,
    test_running: &Arc<AtomicBool>,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
) -> Result<String, String> {
    let listen_addr: SocketAddr = parse_host_port(&config.listen_ip, config.listen_port)?;

    let window_size = lock_or_recover(dynamic_params, "dynamic_params").window_size;

    // TCP 监听 + UDP socket（同一端口，不同协议可共存）。
    // socket2：listen/bind 前设 SO_RCVBUF（-w；Windows 上 >64KB 仅在
    // listen() 前设置才生效，accept 出的连接继承监听 socket 缓冲）
    let domain = if listen_addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let tcp_socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .map_err(|e| format!("创建 TCP 监听 socket 失败: {}", e))?;
    if let Some(w) = window_size {
        let _ = tcp_socket.set_recv_buffer_size(w as usize);
    }
    tcp_socket
        .bind(&listen_addr.into())
        .map_err(|e| format!("无法绑定 TCP 监听端口 {}: {}", listen_addr, e))?;
    tcp_socket
        .listen(128)
        .map_err(|e| format!("TCP listen 失败: {}", e))?;
    let listener: TcpListener = tcp_socket.into();

    // UDP socket 族跟随监听地址（此前硬编码 IPv4：IPv6 监听下 TCP 成功而
    // UDP bind 失败，整个服务端起不来）
    let udp_socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("创建 UDP socket 失败: {}", e))?;
    if let Some(w) = window_size {
        let _ = udp_socket.set_recv_buffer_size(w as usize);
    }
    udp_socket
        .bind(&listen_addr.into())
        .map_err(|e| format!("无法绑定 UDP 端口 {}: {}", listen_addr, e))?;
    let udp_socket: UdpSocket = udp_socket.into();

    // 实际绑定地址（解析后的 SocketAddr，IPv6 呈 [::1]:port 形式）
    let listen_str = listen_addr.to_string();
    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": true,
            "listen_addr": listen_str,
        }),
    );
    log::info!(
        "[iperf2] 服务端监听中 (session={}, {}, TCP+UDP)",
        session_id,
        listen_str
    );

    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置非阻塞失败: {}", e))?;

    let session = Arc::new(Mutex::new(TcpSession::new()));

    // UDP 接待线程（单 socket 串行；共享 TcpSession 用于锁内派生 test_running）
    let udp_handle = {
        let app = app.clone();
        let sid = session_id.to_string();
        let abort = abort_flag.clone();
        let running = test_running.clone();
        let last = last_summary.clone();
        let session = session.clone();
        let udp_socket = udp_socket
            .try_clone()
            .map_err(|e| format!("克隆 UDP socket 失败: {}", e))?;
        std::thread::spawn(move || {
            if let Err(e) = data_udp::run_udp_server_loop(
                &udp_socket,
                &abort,
                &running,
                &last,
                &app,
                &sid,
                &session,
            ) {
                log::warn!("[iperf2] UDP 接待线程退出: {}", e);
            }
        })
    };

    // accept 循环（非阻塞）+ 会话计时/force-end（50ms 粒度）
    let mut tcp_handlers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    loop {
        if abort_flag.load(Ordering::Relaxed) {
            break;
        }
        // 剪枝已结束的 handler 句柄：并发上限按存活线程计——不剪枝则
        // 长跑后死句柄占满上限、新连接被永久拒绝
        tcp_handlers.retain(|h| !h.is_finished());
        match listener.accept() {
            Ok((stream, addr)) => {
                // 并发连接上限：无界 spawn 会让外部主机耗尽本机线程/内存
                if tcp_handlers.len() >= MAX_CONCURRENT_TCP_HANDLERS {
                    log::warn!(
                        "[iperf2] 并发 TCP 连接已达上限 ({})，拒绝 {} (session={})",
                        MAX_CONCURRENT_TCP_HANDLERS,
                        addr,
                        session_id
                    );
                    continue;
                }
                log::info!("[iperf2] TCP 客户端连接: {} (session={})", addr, session_id);
                let app = app.clone();
                let sid = session_id.to_string();
                let abort = abort_flag.clone();
                let running = test_running.clone();
                let last = last_summary.clone();
                let session = session.clone();
                let h = std::thread::spawn(move || {
                    handle_tcp_connection(&app, &sid, stream, &session, &abort, &running, &last);
                });
                tcp_handlers.push(h);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                log::warn!("[iperf2] accept 失败: {}", e);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        tick_session(app, session_id, &session);
    }

    // 停止：强制结束进行中的测试
    {
        let s = lock_or_recover(&session, "TcpSession");
        if s.started {
            s.test_abort.store(true, Ordering::Relaxed);
        }
    }
    // 有界等待处理线程退出（对齐客户端：join 超时兜底，任务必然返回；
    // 极端情况下卡住的握手线程可能被放弃，进程退出时回收）
    join_handles_with_timeout(&tcp_handlers, ENGINE_JOIN_TIMEOUT, "TCP 处理");
    join_handles_with_timeout(
        std::slice::from_ref(&udp_handle),
        ENGINE_JOIN_TIMEOUT,
        "UDP 接待",
    );
    Ok(listen_str)
}

/// 单次 TCP 连接处理：握手 → 同连接接收（-r 后续同 socket 回发）→ 会话流计数。
fn handle_tcp_connection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    mut stream: std::net::TcpStream,
    session: &Arc<Mutex<TcpSession>>,
    abort: &Arc<AtomicBool>,
    test_running: &Arc<AtomicBool>,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
) {
    // 握手（失败时静默退出——可能是端口探测连接）
    let hs = match server_handshake(&mut stream, abort) {
        Ok(hs) => hs,
        Err(e) => {
            log::debug!("[iperf2] 握手失败 ({}): {}", session_id, e);
            return;
        }
    };
    let hdr = hs.hdr;
    let mode = hs.mode;
    let peer_addr = hs.peer_addr;

    // 首条流 → 初始化/强制收尾上一测试；随后计流（单次锁内完成）
    let (first, stale_payload) = {
        let mut s = lock_or_recover(session, "TcpSession");
        // 上一测试已结束但尚未收尾（tick 未及）→ 在此收尾（置位与汇总构建
        // 同锁原子；emit 延后锁外、先于新测试的 started 事件）
        let stale_payload = if s.started && s.streams == 0 && s.rev_streams == 0 {
            s.started = false;
            Some(build_tcp_finalize_payload(&mut s, test_running))
        } else {
            None
        };
        let first = !s.started;
        if first {
            s.counter.reset();
            s.send_counter.reset();
            s.intervals.clear();
            s.rev_intervals.clear();
            s.header = Some(hdr.clone());
            s.mode = mode;
            s.peer_addr = Some(peer_addr);
            s.start = Some(Instant::now());
            s.next_report_secs = SERVER_INTERVAL_SECS;
            s.rev_next_report_secs = SERVER_INTERVAL_SECS;
            s.prev_bytes = 0;
            s.rev_prev_bytes = 0;
            s.rev_start = (mode != ServerTestMode::Normal).then(Instant::now);
            s.rev_active = mode != ServerTestMode::Normal;
            s.last_progress = Instant::now();
            s.last_progress_bytes = 0;
            s.test_abort.store(false, Ordering::Relaxed);
            s.started = true;
        }
        s.streams += 1;
        (first, stale_payload)
    };

    // 上一测试的收尾事件先于新测试的 started 发出（前端按序配对）
    if let Some(payload) = stale_payload {
        emit_tcp_finalize(app, session_id, payload, last_summary);
    }

    if first {
        test_running.store(true, Ordering::Relaxed);
        let _ = app.emit(
            "iperf-test-started",
            serde_json::json!({
                "session_id": session_id,
                "role": "server",
                "direction": "fwd",
                "target": null,
                "protocol": "tcp",
                // 看门狗提示：前端据此计算 done 兜底超时（-d/-r 总时长约为 fwd 两倍）
                "duration_secs": hdr.time_secs(),
                "bidirectional": mode != ServerTestMode::Normal,
            }),
        );
        // -d：反向发送线程组（对齐 2.2.1：服务端收到 RUN_NOW 头后立即反向
        // connect 客户端监听端口；线程数 = 头部 numThreads）
        if mode == ServerTestMode::DualTest {
            spawn_reverse_senders(
                app,
                session_id,
                session,
                &hdr,
                &peer_addr,
                ReverseSenderSignals {
                    abort,
                    test_running,
                    last_summary,
                },
            );
        }
    }

    // fwd 接收循环（同一连接；测试头已在握手中消费）。
    // -r 需在 EOF 后复用原 socket 回发：克隆一份给接收循环，原流留给回发
    let (counter, test_abort, send_counter) = {
        let s = lock_or_recover(session, "TcpSession");
        (
            s.counter.clone(),
            s.test_abort.clone(),
            s.send_counter.clone(),
        )
    };
    match stream.try_clone() {
        Ok(recv_stream) => {
            let bytes = recv_tcp_stream(recv_stream, abort, &test_abort, &counter, None);
            match bytes {
                Ok(b) => log::debug!("[iperf2] 流结束: {} bytes (session={})", b, session_id),
                Err(e) => log::warn!("[iperf2] 流接收失败: {} (session={})", e, session_id),
            }
            // -r：同 socket 回发（对齐 2.2.1 Server.cpp:733——反向时长 =
            // 头部声明时长 + SLOPSECS 2s；发送相 deadline 有界）
            if mode == ServerTestMode::TradeOff {
                {
                    let mut s = lock_or_recover(session, "TcpSession");
                    s.rev_streams += 1;
                    s.rev_active = true;
                    if s.rev_start.is_none() {
                        s.rev_start = Some(Instant::now());
                    }
                }
                let rev_duration = hdr.time_secs().max(1) + REVERSE_SLOPSECS;
                if let Err(e) = send_tcp_stream(stream, rev_duration, None, abort, &send_counter) {
                    log::warn!("[iperf2] 反向回发失败 ({}): {}", session_id, e);
                }
                let mut s = lock_or_recover(session, "TcpSession");
                s.rev_streams = s.rev_streams.saturating_sub(1);
            }
        }
        Err(e) => {
            log::warn!(
                "[iperf2] 克隆数据流失败，反向回发跳过 ({}): {}",
                session_id,
                e
            );
            let _ = recv_tcp_stream(stream, abort, &test_abort, &counter, None);
        }
    }

    // 末条流 → 收尾（fwd 与 rev 全部结束后；置位与汇总构建单次锁内完成，
    // 防止与新测试初始化竞争；emit 在锁外——IPC 不持 TcpSession 锁）
    let payload = {
        let mut s = lock_or_recover(session, "TcpSession");
        s.streams = s.streams.saturating_sub(1);
        if s.started && s.streams == 0 && s.rev_streams == 0 {
            s.started = false;
            Some(build_tcp_finalize_payload(&mut s, test_running))
        } else {
            None
        }
    };
    if let Some(payload) = payload {
        emit_tcp_finalize(app, session_id, payload, last_summary);
    }
}

/// -d 反向发送线程组：从头部解析客户端反向监听端口，反向 connect 后各发一轮
/// （时长 = 头部声明时长 + 2s slop；新连接先发 64B 测试头，对齐 2.2.1 反向客户端）。
///
/// 线程全部结束后若 fwd 已收尾则补触发会话收尾（反向线程通常是最后结束方）。
fn spawn_reverse_senders<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    session: &Arc<Mutex<TcpSession>>,
    hdr: &ClientHdrV1,
    peer_addr: &SocketAddr,
    signals: ReverseSenderSignals<'_>,
) {
    let ReverseSenderSignals {
        abort,
        test_running,
        last_summary,
    } = signals;
    let n = hdr.num_threads.clamp(1, 64) as usize;
    let listen_port = i64::from(hdr.m_port).clamp(1, 65535) as u16;
    let duration = hdr.time_secs().clamp(1, 3600) + REVERSE_SLOPSECS;
    let target = SocketAddr::new(peer_addr.ip(), listen_port);
    // 反向连接的测试头（普通客户端头：flags 无 VERSION1/RUN_NOW）
    let rev_base = ClientHdrV1::new_client(false, n as u32, listen_port, duration);
    let rev_ext = ClientHdrExt::new_client(None);
    let rev_header = tcp_first_payload(&rev_base, &rev_ext);

    {
        let mut s = lock_or_recover(session, "TcpSession");
        s.rev_streams += n;
    }
    log::info!(
        "[iperf2] -d 反向发送启动 (session={}, target={}, streams={}, duration={}s)",
        session_id,
        target,
        n,
        duration
    );

    for _ in 0..n {
        let session = session.clone();
        let app = app.clone();
        let sid = session_id.to_string();
        let abort = abort.clone();
        let test_running = test_running.clone();
        let last_summary = last_summary.clone();
        let rev_header = rev_header.clone();
        std::thread::spawn(move || {
            let send_counter = {
                let s = lock_or_recover(&session, "TcpSession");
                s.send_counter.clone()
            };
            let result = (|| -> Result<(), String> {
                let stream = TcpStream::connect_timeout(&target, Duration::from_secs(10))
                    .map_err(|e| format!("反向连接 {} 失败: {}", target, e))?;
                send_tcp_stream(stream, duration, Some(&rev_header), &abort, &send_counter)
            })();
            if let Err(e) = result {
                log::warn!("[iperf2] 反向发送失败 (session={}): {}", sid, e);
            }
            let payload = {
                let mut s = lock_or_recover(&session, "TcpSession");
                s.rev_streams = s.rev_streams.saturating_sub(1);
                if s.started && s.streams == 0 && s.rev_streams == 0 {
                    s.started = false;
                    Some(build_tcp_finalize_payload(&mut s, &test_running))
                } else {
                    None
                }
            };
            if let Some(payload) = payload {
                emit_tcp_finalize(&app, &sid, payload, &last_summary);
            }
        });
    }
}

/// 派生 test_running = TCP 活动（started）|| UDP 活动（udp_active）。
///
/// TCP 与 UDP 引擎共享一个标志但此前各自独立清空——一方收尾会误清掉
/// 并发中的另一方（前端 serverTestRunning 中途翻 false）。读写均在
/// TcpSession 锁内进行，消除 check-then-act 竞态。
fn update_test_running(session: &Mutex<TcpSession>, test_running: &AtomicBool, udp_active: bool) {
    let mut s = lock_or_recover(session, "TcpSession");
    s.udp_active = udp_active;
    test_running.store(s.started || s.udp_active, Ordering::Relaxed);
}

/// TCP 会话收尾产物：构建于 TcpSession 锁内（保持"置 started=false 与
/// 汇总计算原子"的收尾契约），emit 于锁外（IPC 不持 TcpSession 锁，
/// 不阻塞 accept 循环的 tick 与 force-end 看门狗）。
struct TcpFinalizePayload {
    final_interval: Option<Iperf2Interval>,
    summary: IperfSummary,
    rev_final_interval: Option<Iperf2Interval>,
    rev_summary: Option<IperfSummary>,
    total: u64,
    rev_total: u64,
}

/// 锁内构建收尾产物（调用方须持锁且 `s.started == false` 已置位）。
/// intervals 以 mem::take 移交汇总（零克隆，此前为 2-3 倍全量克隆）。
fn build_tcp_finalize_payload(
    s: &mut TcpSession,
    test_running: &Arc<AtomicBool>,
) -> TcpFinalizePayload {
    let total = s.counter.total();
    let elapsed = s
        .start
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0)
        .max(0.001);
    let last_end = s.intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
    // 补收尾区间（与客户端对称；仅在有增量时——双向模式下 fwd 相早已冻结）
    let mut final_interval = None;
    if total > s.prev_bytes && (s.intervals.is_empty() || last_end < elapsed - 0.5) {
        let interval = Iperf2Interval {
            start_secs: last_end,
            end_secs: elapsed,
            transferred_bytes: total - s.prev_bytes,
            bandwidth_bps: (total - s.prev_bytes) as f64 * 8.0 / (elapsed - last_end).max(0.001),
            jitter_ms: None,
            lost_packets: None,
            total_packets: None,
            lost_percent: None,
        };
        s.intervals.push(interval.clone());
        final_interval = Some(interval);
    }
    let duration = s.intervals.last().map(|i| i.end_secs).unwrap_or(elapsed);

    let summary = build_summary(
        IperfRole::Server,
        IperfProtocol::Tcp,
        duration,
        total,
        std::mem::take(&mut s.intervals),
        LossStats {
            jitter_ms: None,
            lost_packets: None,
            total_packets: None,
            lost_percent: None,
        },
    );

    // rev 汇总（-d/-r）
    let (rev_final_interval, rev_summary, rev_total) = if s.mode != ServerTestMode::Normal {
        let rev_total = s.send_counter.total();
        let rev_elapsed = s
            .rev_start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
            .max(0.001);
        let rev_last_end = s.rev_intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
        let mut rev_interval = None;
        if rev_total > s.rev_prev_bytes
            && (s.rev_intervals.is_empty() || rev_last_end < rev_elapsed - 0.5)
        {
            let interval = Iperf2Interval {
                start_secs: rev_last_end,
                end_secs: rev_elapsed,
                transferred_bytes: rev_total - s.rev_prev_bytes,
                bandwidth_bps: (rev_total - s.rev_prev_bytes) as f64 * 8.0
                    / (rev_elapsed - rev_last_end).max(0.001),
                jitter_ms: None,
                lost_packets: None,
                total_packets: None,
                lost_percent: None,
            };
            s.rev_intervals.push(interval.clone());
            rev_interval = Some(interval);
        }
        let rev_duration = s
            .rev_intervals
            .last()
            .map(|i| i.end_secs)
            .unwrap_or(rev_elapsed);
        let rev_summary = build_summary(
            IperfRole::Server,
            IperfProtocol::Tcp,
            rev_duration,
            rev_total,
            std::mem::take(&mut s.rev_intervals),
            LossStats {
                jitter_ms: None,
                lost_packets: None,
                total_packets: None,
                lost_percent: None,
            },
        );
        (rev_interval, Some(rev_summary), rev_total)
    } else {
        (None, None, 0)
    };

    // 派生 test_running（started 已置 false）：TCP 收尾不得误清并发中的
    // UDP 测试的标志——两引擎共享一个标志，读写均在 TcpSession 锁内
    test_running.store(s.udp_active, Ordering::Relaxed);

    TcpFinalizePayload {
        final_interval,
        summary,
        rev_final_interval,
        rev_summary,
        total,
        rev_total,
    }
}

/// 锁外执行收尾 emit（顺序与旧实现一致：fwd 区间 → fwd done →
/// rev 区间 → rev done，done 必须落在全部区间之后）。
/// test_running 的复位在 build 阶段锁内完成（派生自 TCP/UDP 双引擎状态）。
fn emit_tcp_finalize<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    payload: TcpFinalizePayload,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
) {
    if let Some(interval) = &payload.final_interval {
        emit_interval(
            app,
            session_id,
            IperfRole::Server,
            IperfDirection::Fwd,
            IperfProtocol::Tcp,
            interval,
        );
    }
    let mut last = lock_or_recover(last_summary, "last_summary");
    *last = Some(payload.summary.clone());
    let _ = app.emit(
        "iperf-test-done",
        serde_json::json!({
            "session_id": session_id,
            "success": true,
            "role": "server",
            "direction": "fwd",
            "protocol": "tcp",
            "summary": payload.summary,
        }),
    );
    log::info!(
        "[iperf2] 服务端接待测试完成 (session={}, {} bytes)",
        session_id,
        payload.total
    );

    if let Some(rev_summary) = payload.rev_summary {
        if let Some(interval) = &payload.rev_final_interval {
            emit_interval(
                app,
                session_id,
                IperfRole::Server,
                IperfDirection::Rev,
                IperfProtocol::Tcp,
                interval,
            );
        }
        let _ = app.emit(
            "iperf-test-done",
            serde_json::json!({
                "session_id": session_id,
                "success": true,
                "role": "server",
                "direction": "rev",
                "protocol": "tcp",
                "summary": rev_summary,
            }),
        );
        log::info!(
            "[iperf2] 服务端反向回发完成 (session={}, {} bytes)",
            session_id,
            payload.rev_total
        );
    }
}

/// 会话计时 tick：fwd/rev SUM 区间实时上报 + force-end 兜底
/// （accept 循环内 50ms 粒度；收尾由连接处理线程/反向线程触发）
fn tick_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    session: &Arc<Mutex<TcpSession>>,
) {
    let mut s = lock_or_recover(session, "TcpSession");
    if !s.started {
        return;
    }
    let Some(start) = s.start else { return };
    let elapsed = start.elapsed();

    // force-end：time 模式按 clamp 后的声明时长 + 宽限；非 time 模式（-n amount）
    // 按无进展宽限兜底（长传输不被提前杀掉——真实 iperf2 对 amount 模式不设
    // 时间上限，只有停滞连接才需要兜底）
    let duration_hint = s.header.as_ref().map(|h| h.time_secs()).unwrap_or(0);
    let force_end = if duration_hint > 0 {
        let hint = (duration_hint as f64).min(SERVER_FORCE_END_MAX_SECS);
        let rev_slop = if s.mode != ServerTestMode::Normal {
            REVERSE_SLOPSECS as f64
        } else {
            0.0
        };
        elapsed >= Duration::from_secs_f64(hint + rev_slop) + SERVER_FORCE_END_GRACE
    } else {
        let total = s.counter.total();
        if total != s.last_progress_bytes {
            s.last_progress_bytes = total;
            s.last_progress = Instant::now();
        }
        s.last_progress.elapsed() >= SERVER_FORCE_END_GRACE
    };
    if force_end {
        log::warn!(
            "[iperf2] 会话超时强制结束 (session={}, elapsed={:.1}s)",
            session_id,
            elapsed.as_secs_f64()
        );
        s.test_abort.store(true, Ordering::Relaxed);
    }

    // fwd SUM 区间实时上报
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs >= s.next_report_secs {
        let total = s.counter.total();
        let interval = Iperf2Interval {
            start_secs: s.next_report_secs - SERVER_INTERVAL_SECS,
            end_secs: s.next_report_secs,
            transferred_bytes: total - s.prev_bytes,
            bandwidth_bps: (total - s.prev_bytes) as f64 * 8.0 / SERVER_INTERVAL_SECS,
            jitter_ms: None,
            lost_packets: None,
            total_packets: None,
            lost_percent: None,
        };
        emit_interval(
            app,
            session_id,
            IperfRole::Server,
            IperfDirection::Fwd,
            IperfProtocol::Tcp,
            &interval,
        );
        s.intervals.push(interval);
        s.prev_bytes = total;
        s.next_report_secs += SERVER_INTERVAL_SECS;
    }

    // rev SUM 区间（-d/-r；区间时钟相对 rev 相起点——-r 的 rev 起点晚于 fwd）
    if s.mode != ServerTestMode::Normal && s.rev_active {
        if let Some(rev_start) = s.rev_start {
            let rev_elapsed = rev_start.elapsed().as_secs_f64();
            if rev_elapsed >= s.rev_next_report_secs {
                let rev_total = s.send_counter.total();
                let interval = Iperf2Interval {
                    start_secs: s.rev_next_report_secs - SERVER_INTERVAL_SECS,
                    end_secs: s.rev_next_report_secs,
                    transferred_bytes: rev_total - s.rev_prev_bytes,
                    bandwidth_bps: (rev_total - s.rev_prev_bytes) as f64 * 8.0
                        / SERVER_INTERVAL_SECS,
                    jitter_ms: None,
                    lost_packets: None,
                    total_packets: None,
                    lost_percent: None,
                };
                emit_interval(
                    app,
                    session_id,
                    IperfRole::Server,
                    IperfDirection::Rev,
                    IperfProtocol::Tcp,
                    &interval,
                );
                s.rev_intervals.push(interval);
                s.rev_prev_bytes = rev_total;
                s.rev_next_report_secs += SERVER_INTERVAL_SECS;
            }
        }
    }
}

// ── 临时生命周期验证（验证后删除） ──

#[cfg(test)]
mod lifecycle_tests {
    use super::stats::SharedByteCounter;
    use super::test_hdr::{tcp_first_payload, ClientHdrExt, ClientHdrV1};
    use super::*;

    fn no_op_emit(_dir: IperfDirection, _r: &Iperf2Interval) {}

    /// 服务器 accept 后不读数据 → 客户端发送缓冲填满 → write 阻塞。
    /// 即使 SO_SNDTIMEO 失效（write 无限阻塞），join 超时兜底也应使任务
    /// 在时限（3s 时长 + 10s join + 余量）内返回，而非无限阻塞。
    #[test]
    fn tcp_client_returns_when_peer_stops_reading() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定失败");
        let port = listener.local_addr().expect("地址").port();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            // 故意不读——让客户端发送缓冲填满
            std::thread::sleep(Duration::from_secs(60));
        });
        let p = Iperf2TestParams {
            mode: TestMode::Tcp,
            duration_secs: 3,
            parallel_streams: 1,
            bandwidth_bps: None,
            window_size: None,
            report_interval_secs: 1,
            port,
            direction: TestDirection::Normal,
        };
        let abort = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let s = data_tcp::run_tcp_client(&format!("127.0.0.1:{}", port), &p, &abort, &no_op_emit)
            .expect("客户端应返回（而非无限阻塞）");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(18),
            "任务应有时限返回，实际 {}s",
            elapsed.as_secs_f64()
        );
        println!(
            "[生命周期] 对端停读场景 OK: {}s 内返回, {} bytes",
            elapsed.as_secs_f64(),
            s.bytes_sent
        );
        drop(server);
    }

    /// loopback 双向验证（真实 socket）：最小 iperf2 服务端按 -r 语义
    /// 握手 → 接收 → 同 socket 回发；客户端应收到 fwd 与 rev 双向字节。
    #[test]
    fn loopback_tradeoff_reverse_on_same_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定失败");
        let port = listener.local_addr().expect("地址").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let abort = Arc::new(AtomicBool::new(false));
            let hs = control::server_handshake(&mut stream, &abort).expect("握手失败");
            assert_eq!(hs.mode, ServerTestMode::TradeOff);
            // fwd 接收（克隆流给接收线程，原流留给同 socket 回发）
            let recv_abort = abort.clone();
            let recv_test_abort = Arc::new(AtomicBool::new(false));
            let recv_counter = Arc::new(SharedByteCounter::new());
            let recv_stream = stream.try_clone().expect("克隆流");
            let recv = std::thread::spawn(move || {
                data_tcp::recv_tcp_stream(
                    recv_stream,
                    &recv_abort,
                    &recv_test_abort,
                    &recv_counter,
                    None,
                )
            });
            let n = recv.join().expect("recv join").expect("fwd 接收失败");
            assert!(n > 0, "fwd 无字节");
            // 同 socket 回发（-r：声明时长 + 2s slop）
            let send_counter = Arc::new(SharedByteCounter::new());
            data_tcp::send_tcp_stream(
                stream,
                hs.hdr.time_secs().max(1) + REVERSE_SLOPSECS,
                None,
                &abort,
                &send_counter,
            )
            .expect("回发失败");
            assert!(send_counter.total() > 0, "rev 回发无字节");
        });

        let p = Iperf2TestParams {
            mode: TestMode::Tcp,
            duration_secs: 2,
            parallel_streams: 1,
            bandwidth_bps: None,
            window_size: Some(64 * 1024), // -w：connect 前 SO_SNDBUF 路径一并覆盖
            report_interval_secs: 1,
            port,
            direction: TestDirection::TradeOff,
        };
        let abort = Arc::new(AtomicBool::new(false));
        let s = data_tcp::run_tcp_client(&format!("127.0.0.1:{}", port), &p, &abort, &no_op_emit)
            .expect("客户端测速失败");
        assert!(s.bytes_sent > 0, "fwd 无字节");
        assert!(s.rev_bytes_received > 0, "rev 无字节");
        assert!(s.rev_active);
        server.join().expect("server join");
    }

    /// loopback 双向验证（真实 socket）：最小 iperf2 服务端按 -d 语义
    /// 握手 → 从头部读取客户端反向监听端口 → 反向 connect 回客户端 →
    /// 发头 + 数据（fwd 与 rev 并发）；客户端应收到双向字节。
    #[test]
    fn loopback_dualtest_reverse_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定失败");
        let port = listener.local_addr().expect("地址").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let abort = Arc::new(AtomicBool::new(false));
            let hs = control::server_handshake(&mut stream, &abort).expect("握手失败");
            assert_eq!(hs.mode, ServerTestMode::DualTest);
            let rev_port = u16::try_from(hs.hdr.m_port).expect("反向端口无效");
            assert!(rev_port > 0, "头部应携带客户端反向监听端口");
            // fwd 接收（并发；克隆流给接收线程）
            let recv_abort = abort.clone();
            let recv_test_abort = Arc::new(AtomicBool::new(false));
            let recv_counter = Arc::new(SharedByteCounter::new());
            let recv_stream = stream.try_clone().expect("克隆流");
            let recv = std::thread::spawn(move || {
                data_tcp::recv_tcp_stream(
                    recv_stream,
                    &recv_abort,
                    &recv_test_abort,
                    &recv_counter,
                    None,
                )
            });
            // 反向连接回客户端监听端口（新连接先发 64B 测试头，对齐 2.2.1 反向客户端）
            let rev = TcpStream::connect(("127.0.0.1", rev_port)).expect("反向连接失败");
            let rev_duration = hs.hdr.time_secs().max(1) + REVERSE_SLOPSECS;
            let rev_base = ClientHdrV1::new_client(false, 1, rev_port, rev_duration);
            let rev_ext = ClientHdrExt::new_client(None);
            let rev_hdr = tcp_first_payload(&rev_base, &rev_ext);
            let rev_counter = Arc::new(SharedByteCounter::new());
            data_tcp::send_tcp_stream(rev, rev_duration, Some(&rev_hdr), &abort, &rev_counter)
                .expect("反向发送失败");
            assert!(rev_counter.total() > 0, "rev 回发无字节");
            let n = recv.join().expect("recv join").expect("fwd 接收失败");
            assert!(n > 0, "fwd 无字节");
        });

        let p = Iperf2TestParams {
            mode: TestMode::Tcp,
            duration_secs: 2,
            parallel_streams: 1,
            bandwidth_bps: None,
            window_size: None,
            report_interval_secs: 1,
            port,
            direction: TestDirection::DualTest,
        };
        let abort = Arc::new(AtomicBool::new(false));
        let s = data_tcp::run_tcp_client(&format!("127.0.0.1:{}", port), &p, &abort, &no_op_emit)
            .expect("客户端测速失败");
        assert!(s.bytes_sent > 0, "fwd 无字节");
        assert!(s.rev_bytes_received > 0, "rev 无字节");
        assert!(s.rev_active);
        server.join().expect("server join");
    }
}
