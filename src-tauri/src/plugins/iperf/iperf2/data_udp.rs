//! iperf2 UDP 数据流（对齐 2.2.1 源码）
//!
//! UDP **没有 TCP 控制连接**——所有通信走单个 UDP socket，发往服务器端口
//! （默认 5001，与控制端口相同，无 +1）。
//! 数据包格式（网络序）：
//! ```text
//! [UDP_datagram: id/tv_sec/tv_usec/id2 (16B)]
//! [client_hdr_v1 (24B)] [client_hdrext (40B)] [pattern 载荷]
//! ```
//! - 首包携带完整测试头；后续包只更新 id/tv_sec/tv_usec（头字节静态）
//! - **序号从 1 开始**（64 位，id 低 32 位 + id2 高 32 位）
//! - 结束：客户端发**负序号 FIN 包**，每 10ms 重传（≤2s）等待回报；
//!   服务器收到 FIN 立即回 `[UDP_datagram 全零 + server_hdr(40B)]`，
//!   客户端继续发包则重试（≤10 次）
//! - 带宽在 `hdrext.lRate/uRate`（`mWinBand` 从不写）；默认 1 Mbps
//! - 抖动 = RFC 1889/3550 `J += (|D|-J)/16`；丢包 = 序号间隙

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use super::stats::{IntervalAccumulator, SharedByteCounter};
use super::test_hdr::{
    udp_packet_header, ClientHdrExt, ClientHdrV1, ServerHdrV1, UdpDatagram,
};
use super::types::Iperf2Interval;
use crate::plugins::iperf::{IperfDirection, IperfProtocol, IperfRole, IperfSummary, IperfVersion};
use tauri::Emitter;

/// 默认 UDP 数据报总长（1470 = 以太网 MTU 1500 - 20 IP - 8 UDP）
const DEFAULT_UDP_PAYLOAD: usize = 1470;
/// 默认 UDP 带宽（iperf2 未指定 -b 时的默认值）
const DEFAULT_UDP_BPS: u64 = 1_000_000;
/// 接收轮询超时（服务端：无数据则认为测试结束）
// 空闲扫描超时须大于合法低带宽测试的包间隔：-b 1K 的间隔 ≈11.8s，
// 5s 会把一次测试拆成多条记录并截断客户端统计；30s 覆盖至 ~400bps 的
// 极端低带宽（更慢的目标属病态，容忍误收尾换取及时释放资源）
const RECV_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// 客户端 FIN 重传窗口（对齐 RETRYCOUNT 200 × 10ms）
const FIN_RETRY_WINDOW: Duration = Duration::from_secs(2);
/// 服务端报告重发上限（对齐 TRYCOUNT 10）
const REPORT_MAX_RETRIES: usize = 10;

/// UDP 测试统计结果
pub struct UdpStats {
    pub bytes_sent: u64,
    #[allow(dead_code)] // 服务端统计在接待侧使用
    pub bytes_received: u64,
    #[allow(dead_code)]
    pub packets_sent: u64,
    #[allow(dead_code)]
    pub packets_received: u64,
    pub jitter_ms: Option<f64>,
    pub lost_packets: Option<u64>,
    pub total_packets: Option<u64>,
    pub lost_percent: Option<f64>,
    /// 服务器统计回报是否收到（None = 用户中止未等待回报；Some(false) =
    /// 回报超时——UDP 无连接检测，目标不可达时发送侧"看似满速"实为零接收）
    pub server_report_received: Option<bool>,
    pub intervals: Vec<Iperf2Interval>,
}

/// 构造 UDP 数据包（UDP_datagram + 测试头 + 载荷；时间戳为发送时刻）
fn build_packet(payload: &mut [u8], seqno: i64, header: &[u8]) {
    let mut datagram = UdpDatagram::new(seqno as u64);
    datagram.stamp(); // 服务器据此计算抖动（接收时刻 - 发送时刻）
    let dg = datagram.serialize();
    payload[..UdpDatagram::SIZE].copy_from_slice(&dg);
    payload[UdpDatagram::SIZE..UdpDatagram::SIZE + header.len()].copy_from_slice(header);
    // 载荷填充（从测试头之后开始写，保持 iperf2 的 pattern 行为）
    let start = UdpDatagram::SIZE + header.len();
    for (i, b) in payload[start..].iter_mut().enumerate() {
        *b = ((i + 1) % 256) as u8;
    }
}

/// 解析数据包（返回 64 位序号与发送时间戳毫秒）
fn parse_packet(payload: &[u8]) -> Option<(i64, f64)> {
    if payload.len() < UdpDatagram::SIZE {
        return None;
    }
    let dg = UdpDatagram::deserialize(&payload[..UdpDatagram::SIZE]).ok()?;
    Some((dg.seqno() as i64, dg.send_ms()))
}

/// 服务器统计回报包（UDP_datagram 全零 + server_hdr，56B）
fn build_server_report(server_hdr: &ServerHdrV1) -> Vec<u8> {
    let mut buf = UdpDatagram::new(0).serialize();
    buf.extend_from_slice(&server_hdr.serialize());
    buf
}

/// 解析服务器统计回报（返回 server_hdr，不足 56B 则 None）
fn parse_server_report(payload: &[u8]) -> Option<ServerHdrV1> {
    if payload.len() < UdpDatagram::SIZE + ServerHdrV1::SIZE {
        return None;
    }
    ServerHdrV1::deserialize(&payload[UdpDatagram::SIZE..]).ok()
}

// ── 客户端 ─────────────────────────────────────────────

/// 客户端 UDP 发送（限速，多流共享全局序号，序号从 1 开始）。
///
/// 结束后发负序号 FIN 并等待服务器 `server_hdr` 回报（重传 ≤2s）。
pub fn run_udp_client(
    server_addr: &str,
    params: &super::types::Iperf2TestParams,
    abort: &Arc<AtomicBool>,
    emit: &dyn Fn(IperfDirection, &Iperf2Interval),
) -> Result<UdpStats, String> {
    let rate = params.bandwidth_bps.unwrap_or(DEFAULT_UDP_BPS).max(1);
    // 每包间隔（秒）= 数据报总长(bits) / 目标带宽
    let per_packet_delay = DEFAULT_UDP_PAYLOAD as f64 * 8.0 / rate as f64;

    let counter = Arc::new(SharedByteCounter::new());
    let packet_counter = Arc::new(AtomicI64::new(0));
    let global_id = Arc::new(AtomicI64::new(1)); // 真实客户端 packetID 从 1 开始

    // 每包测试头（base + extend，64B）——UDP 无状态，每包携带
    let header = {
        let base = ClientHdrV1::new_client(
            true,
            params.parallel_streams,
            params.port,
            params.duration_secs,
        );
        let ext = ClientHdrExt::new_client(params.bandwidth_bps);
        udp_packet_header(&base, &ext)
    };

    // 单 socket 共享：发送线程并发 send（UDP 线程安全），主线程结束后用同一
    // socket 收服务器统计回报（回报发往发包源端口）。
    // socket 族与绑定地址跟随目标地址族（此前硬编码 IPv4：IPv6 目标必败）。
    // -w：bind 之前设 SO_SNDBUF/SO_RCVBUF（对齐 tcp_window_size.c 语义）
    let server_sa: SocketAddr = server_addr
        .parse()
        .map_err(|e| format!("UDP 目标地址无效 {}: {}", server_addr, e))?;
    let domain = if server_sa.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let sock = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("创建 UDP socket 失败: {}", e))?;
    if let Some(w) = params.window_size {
        let _ = sock.set_send_buffer_size(w as usize);
        let _ = sock.set_recv_buffer_size(w as usize);
    }
    let bind_addr: SocketAddr = if server_sa.is_ipv4() {
        "0.0.0.0:0".parse().map_err(|e| format!("UDP 绑定地址无效: {}", e))?
    } else {
        "[::]:0".parse().map_err(|e| format!("UDP 绑定地址无效: {}", e))?
    };
    let bind_sock_addr: socket2::SockAddr = bind_addr.into();
    sock.bind(&bind_sock_addr)
        .map_err(|e| format!("UDP socket 绑定失败: {}", e))?;
    let socket: UdpSocket = sock.into();
    socket
        .connect(server_addr)
        .map_err(|e| format!("UDP connect 失败 {}: {}", server_addr, e))?;
    // SO_SNDTIMEO（对齐真实 iperf2，Client.cpp 对 UDP 同样设置）：发送缓冲
    // 满时 send 有界返回（TimedOut），发送线程必然退出——否则可能无限阻塞
    socket
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| format!("设置 UDP 写超时失败: {}", e))?;
    let socket = Arc::new(socket);

    let mut handles = Vec::new();
    for _ in 0..params.parallel_streams.max(1) {
        let socket = socket.clone();
        let counter = counter.clone();
        let packet_counter = packet_counter.clone();
        let global_id = global_id.clone();
        let abort = abort.clone();
        let header = header.clone();
        handles.push(std::thread::spawn(move || {
            let mut payload = vec![0u8; DEFAULT_UDP_PAYLOAD];
            while !abort.load(Ordering::Relaxed) {
                let id = global_id.fetch_add(1, Ordering::Relaxed);
                build_packet(&mut payload, id, &header);
                match socket.send(&payload) {
                    Ok(n) => {
                        counter.add(n as u64);
                        packet_counter.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        // 发送缓冲满（SO_SNDTIMEO 触发）：等待后重试，而非放弃
                        // 整个测试——与 TCP 写循环的语义一致
                        log::debug!("[iperf2] UDP 发送缓冲满，重试");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => {
                        log::debug!("[iperf2] UDP 数据流发送结束: {}", e);
                        break;
                    }
                }
                // 限速 pacing（不设 1s 上限：低带宽目标如 -b 1K 的包间隔可达秒级，
                // 截断会导致实际速率远超目标）。混合配速：先 sleep 整毫秒，再忙等
                // 余量——Windows 定时器粒度（≥1ms，默认 ~15.6ms）下纯 sleep 会把
                // 实际速率封顶在 ~12Mbps（1ms）/~0.75Mbps（15.6ms）
                let delay = Duration::from_secs_f64(per_packet_delay);
                let whole_ms = delay.as_millis();
                if whole_ms > 0 {
                    std::thread::sleep(Duration::from_millis(whole_ms as u64));
                }
                let deadline = Instant::now() + delay;
                while Instant::now() < deadline {
                    std::hint::spin_loop();
                }
            }
        }));
    }

    // 主线程：计时 + 区间实时上报
    let mut intervals = Vec::new();
    let start = Instant::now();
    let total_secs = params.duration_secs.max(1) as f64;
    let interval_secs = params.report_interval_secs.max(1) as f64;

    let mut prev_bytes = 0u64;
    let mut next_report_secs = interval_secs;
    // 计时循环退出时先记录是否为用户中止（abort 即将被我们置位用于停发送线程）
    let user_aborted;
    loop {
        let elapsed = start.elapsed();
        if elapsed >= Duration::from_secs_f64(total_secs) || abort.load(Ordering::Relaxed) {
            user_aborted = abort.load(Ordering::Relaxed);
            break;
        }
        let elapsed_secs = elapsed.as_secs_f64();
        if elapsed_secs >= next_report_secs {
            let total = counter.total();
            let interval = Iperf2Interval {
                start_secs: next_report_secs - interval_secs,
                end_secs: next_report_secs,
                transferred_bytes: total - prev_bytes,
                bandwidth_bps: (total - prev_bytes) as f64 * 8.0 / interval_secs,
                jitter_ms: None, // 发送侧无抖动（抖动在对端测量）
                lost_packets: None,
                total_packets: None,
                lost_percent: None,
            };
            emit(IperfDirection::Fwd, &interval);
            intervals.push(interval);
            prev_bytes = total;
            next_report_secs += interval_secs;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    abort.store(true, Ordering::Relaxed);
    // 有界等待（对齐 TCP 客户端）：UDP send 一般不阻塞，此处仅作兜底
    super::join_handles_with_timeout(&handles, super::ENGINE_JOIN_TIMEOUT, "UDP 发送");

    let bytes_sent = counter.total();
    let packets_sent = packet_counter.load(Ordering::Relaxed) as u64;
    let elapsed = start.elapsed().as_secs_f64().max(0.001);
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

    log::info!(
        "[iperf2] UDP 客户端发送完成: {} bytes, {} packets",
        bytes_sent,
        packets_sent
    );

    // 结束协议：发负序号 FIN → 等待服务器回报（jitter/loss 来自对端测量）。
    // 用户中止时跳过回报等待，立即返回（abort 此时已由收尾置位）。
    let (report, report_received) = if user_aborted {
        (None, None)
    } else {
        let last_seqno = global_id.load(Ordering::Relaxed) - 1;
        let r = await_server_report(&socket, last_seqno, &header);
        (r, Some(r.is_some()))
    };

    Ok(UdpStats {
        bytes_sent,
        bytes_received: 0,
        packets_sent,
        packets_received: 0,
        jitter_ms: report.map(|r| r.0),
        lost_packets: report.map(|r| r.1),
        total_packets: report.map(|r| r.2),
        lost_percent: report.map(|r| r.3),
        server_report_received: report_received,
        intervals,
    })
}

/// 客户端结束协议：重传负序号 FIN（每 10ms，≤2s），解析服务器回报。
///
/// FIN 包 = 负序号的完整数据报（复用测试头字节，对齐真实客户端 mBuf 行为）。
/// 返回 `(jitter_ms, lost, total, lost_pct)`；超时返回 None（容忍）。
fn await_server_report(
    socket: &Arc<UdpSocket>,
    last_seqno: i64,
    header: &[u8],
) -> Option<(f64, u64, u64, f64)> {
    let mut fin_payload = vec![0u8; DEFAULT_UDP_PAYLOAD];
    let mut fin_seq = -last_seqno.max(1);
    let deadline = Instant::now() + FIN_RETRY_WINDOW;
    let mut buf = vec![0u8; 65536];
    // 注意：不检查 abort——调用方收尾时已置位 abort（停发送线程），
    // 用户中止时根本不会进入本函数；2s 期限已保证有界。
    while Instant::now() < deadline {
        build_packet(&mut fin_payload, fin_seq, header);
        let _ = socket.send(&fin_payload);
        // 等待回报（10ms 轮询）
        let _ = socket.set_read_timeout(Some(Duration::from_millis(10)));
        if let Ok(n) = socket.recv(&mut buf) {
            if let Some(hdr) = parse_server_report(&buf[..n]) {
                let lost = hdr.error_cnt.max(0) as u64;
                let total = hdr.datagrams.max(0) as u64;
                let lost_pct = if total > 0 {
                    lost as f64 * 100.0 / total as f64
                } else {
                    0.0
                };
                log::debug!(
                    "[iperf2] 已收到服务器回报: jitter={:.3}ms lost={} total={}",
                    hdr.jitter_ms(),
                    lost,
                    total
                );
                return Some((hdr.jitter_ms(), lost, total, lost_pct));
            }
            // 非 server_hdr 包（不应出现）——继续
        }
        fin_seq -= 1;
    }
    log::debug!("[iperf2] 服务器统计回报超时/未收到（容忍）");
    None
}

// ── 服务端 ─────────────────────────────────────────────

/// 单个 UDP 客户端的接待状态（统计累积 + 区间上报）。
///
/// 行为对齐真实 iperf2（Listener.cpp udp_accept）：正序号首包才受理新客户端、
/// 负序号残留排水、每客户端独立接待/统计/收尾、报告按需重发（≤TRYCOUNT）。
struct UdpClientState {
    /// 会话内自增配对键（started/done/interval 事件携带，前端按此归位记录）
    seq: u64,
    /// 总体统计（不随区间 reset，供 server_hdr 报告）
    acc: IntervalAccumulator,
    /// 区间统计（每个 -i 边界 reset，供实时上报）
    interval_acc: IntervalAccumulator,
    /// 字节计数
    counter: SharedByteCounter,
    intervals: Vec<Iperf2Interval>,
    start: Instant,
    next_report_secs: f64,
    prev_bytes: u64,
    last_activity: Instant,
}

impl UdpClientState {
    fn new(seq: u64, first_packet: &[u8]) -> Self {
        let mut st = Self {
            seq,
            acc: IntervalAccumulator::new(),
            interval_acc: IntervalAccumulator::new(),
            counter: SharedByteCounter::new(),
            intervals: Vec::new(),
            start: Instant::now(),
            next_report_secs: 1.0,
            prev_bytes: 0,
            last_activity: Instant::now(),
        };
        // 首包计入统计（真实服务器 udp_accept 收到的首包即测试数据）
        if let Some((seqno, send_ms)) = parse_packet(first_packet) {
            st.counter.add(first_packet.len() as u64);
            st.acc.record_udp(false, seqno, send_ms);
            st.interval_acc.record_udp(false, seqno, send_ms);
        }
        st
    }

    /// 累积一个数据包（抖动/丢包统计 + 字节计数）
    fn record<R: tauri::Runtime>(&mut self, n: usize, seqno: i64, send_ms: f64, app: &tauri::AppHandle<R>, session_id: &str) {
        self.last_activity = Instant::now();
        self.counter.add(n as u64);
        self.acc.record_udp(false, seqno, send_ms);
        self.interval_acc.record_udp(false, seqno, send_ms);
        self.maybe_report_interval(app, session_id);
    }

    /// 区间边界检查 + 实时上报（每 -i 边界一次）
    fn maybe_report_interval<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, session_id: &str) {
        const INTERVAL_SECS: f64 = 1.0;
        let elapsed_secs = self.start.elapsed().as_secs_f64();
        if elapsed_secs < self.next_report_secs {
            return;
        }
        let total = self.counter.total();
        let interval = Iperf2Interval {
            start_secs: self.next_report_secs - INTERVAL_SECS,
            end_secs: self.next_report_secs,
            transferred_bytes: total - self.prev_bytes,
            bandwidth_bps: (total - self.prev_bytes) as f64 * 8.0 / INTERVAL_SECS,
            jitter_ms: (self.interval_acc.packets() > 0).then(|| self.interval_acc.jitter_ms()),
            lost_packets: (self.interval_acc.packets() > 0).then(|| self.interval_acc.lost()),
            total_packets: (self.interval_acc.packets() > 0).then(|| self.interval_acc.packets()),
            lost_percent: if self.interval_acc.packets() > 0 {
                Some(self.interval_acc.lost() as f64 * 100.0 / self.interval_acc.packets() as f64)
            } else {
                None
            },
        };
        super::emit_interval_seq(
            app,
            session_id,
            IperfRole::Server,
            IperfDirection::Fwd,
            IperfProtocol::Udp,
            &interval,
            self.seq,
        );
        // 存储区间必须与实时事件一致（counter 差值；acc 累计器无字节计数，
        // 快照语义不符——汇总区间以本份数据为准）
        self.intervals.push(interval);
        self.interval_acc.reset();
        self.prev_bytes = total;
        self.next_report_secs += INTERVAL_SECS;
    }
}

/// 收尾窗口内的客户端（server_hdr 报告重发状态；对齐上游 TRYCOUNT 重试）。
struct ClosingClient {
    report: Vec<u8>,
    retries: usize,
    deadline: Instant,
}

/// 服务端 UDP 接待循环：单 reader + 按客户端地址多路复用。
///
/// 取代上游"connect() 过滤 + 移交子线程 + 重建监听 socket"架构——
/// 行为等价（并发接待、逐客户端统计/收尾/报告），且避开 Windows 上
/// socket 移交的复杂度。接收循环按地址分派：
/// - 新地址正序号包 → 建接待状态、发 iperf-test-started（带 seq）
/// - 已知地址正序号包 → 累积统计 + 区间上报
/// - 负序号 FIN → 收尾该客户端（done + 报告入收尾窗口，重传则重发）
/// - 空闲扫描：超 RECV_IDLE_TIMEOUT 的客户端收尾（安全网，真实客户端总发 FIN）
/// - abort / 致命错误 → 收尾所有剩余客户端（保持 done 必发契约）再退出
pub fn run_udp_server_loop<R: tauri::Runtime>(
    socket: &UdpSocket,
    abort: &Arc<AtomicBool>,
    test_running: &Arc<AtomicBool>,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
    app: &tauri::AppHandle<R>,
    session_id: &str,
    session: &Arc<Mutex<super::TcpSession>>,
) -> Result<(), String> {
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| format!("设置读超时失败: {}", e))?;

    let mut buf = vec![0u8; 65536];
    let mut clients: HashMap<SocketAddr, UdpClientState> = HashMap::new();
    let mut closing: HashMap<SocketAddr, ClosingClient> = HashMap::new();
    let mut seq_counter: u64 = 0;

    loop {
        if abort.load(Ordering::Relaxed) {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let Some((seqno, send_ms)) = parse_packet(&buf[..n]) else {
                    continue;
                };
                if seqno > 0 {
                    // 正序号：接待中 → 累积；收尾窗口内 → 视为新测试；
                    // 全新地址 → 受理新客户端
                    if let Some(st) = clients.get_mut(&addr) {
                        st.record(n, seqno, send_ms, app, session_id);
                    } else {
                        closing.remove(&addr);
                        seq_counter += 1;
                        let seq = seq_counter;
                        let st = UdpClientState::new(seq, &buf[..n]);
                        log::info!(
                            "[iperf2] UDP 客户端开始测试 #{}: {} (session={})",
                            seq, addr, session_id
                        );
                        // test_running 经 TcpSession 锁派生（TCP||UDP），
                        // 避免两引擎独立清空误伤并发中的另一方
                        super::update_test_running(session, test_running, true);
                        let _ = app.emit("iperf-test-started", serde_json::json!({
                            "session_id": session_id,
                            "role": "server",
                            "direction": "fwd",
                            "target": null,
                            "protocol": "udp",
                            "seq": seq,
                        }));
                        clients.insert(addr, st);
                    }
                } else {
                    // 负序号 FIN：接待中 → 收尾；收尾窗口内 → 重发报告；
                    // 其余 → 残留排水（对齐上游 drainstalepkts）
                    if let Some(st) = clients.remove(&addr) {
                        finalize_udp_client(
                            socket, app, session_id, addr, st, last_summary, &mut closing,
                        );
                        if clients.is_empty() {
                            super::update_test_running(session, test_running, false);
                        }
                    } else if let Some(c) = closing.get_mut(&addr) {
                        resend_report(socket, addr, c);
                    } else {
                        log::debug!("[iperf2] 丢弃残留 FIN 包 (seq={})", seqno);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                let now = Instant::now();
                // 空闲扫描：客户端不发 FIN 也不发包 → 强制收尾
                let mut expired: Vec<SocketAddr> = Vec::new();
                for (addr, st) in clients.iter() {
                    if st.acc.packets() > 0 && st.last_activity.elapsed() >= RECV_IDLE_TIMEOUT {
                        expired.push(*addr);
                    }
                }
                for addr in expired {
                    if let Some(st) = clients.remove(&addr) {
                        finalize_udp_client(
                            socket, app, session_id, addr, st, last_summary, &mut closing,
                        );
                    }
                }
                if clients.is_empty() {
                    super::update_test_running(session, test_running, false);
                }
                // 收尾窗口清扫
                closing.retain(|_, c| c.deadline > now);
            }
            Err(e) => {
                // 致命接收错误：收尾所有进行中的客户端（done 必发）后上报
                let msg = format!("UDP 接收失败: {}", e);
                for (addr, st) in clients.drain() {
                    finalize_udp_client(socket, app, session_id, addr, st, last_summary, &mut closing);
                }
                super::update_test_running(session, test_running, false);
                return Err(msg);
            }
        }
    }

    // abort：收尾所有剩余客户端（保持 done 必发契约）
    for (addr, st) in clients.drain() {
        finalize_udp_client(socket, app, session_id, addr, st, last_summary, &mut closing);
    }
    super::update_test_running(session, test_running, false);
    Ok(())
}

/// 重发服务器统计报告（客户端 FIN 重传期间，≤TRYCOUNT 次）
fn resend_report(socket: &UdpSocket, addr: SocketAddr, c: &mut ClosingClient) {
    if c.retries == 0 {
        return;
    }
    c.retries -= 1;
    c.deadline = Instant::now() + Duration::from_millis(200);
    let _ = socket.send_to(&c.report, addr);
}

/// 收尾单个客户端：补发收尾区间 → 汇总 → done 事件 → 报告入收尾窗口。
fn finalize_udp_client<R: tauri::Runtime>(
    socket: &UdpSocket,
    app: &tauri::AppHandle<R>,
    session_id: &str,
    addr: SocketAddr,
    mut st: UdpClientState,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
    closing: &mut HashMap<SocketAddr, ClosingClient>,
) {
    // 收尾区间（最后一段未达一个完整区间时补发）
    let elapsed = st.start.elapsed().as_secs_f64().max(0.001);
    let last_end = st.intervals.last().map(|i| i.end_secs).unwrap_or(0.0);
    if last_end < elapsed - 0.3 {
        let total = st.counter.total();
        let interval = Iperf2Interval {
            start_secs: last_end,
            end_secs: elapsed,
            transferred_bytes: total - st.prev_bytes,
            bandwidth_bps: (total - st.prev_bytes) as f64 * 8.0 / (elapsed - last_end).max(0.001),
            jitter_ms: (st.interval_acc.packets() > 0).then(|| st.interval_acc.jitter_ms()),
            lost_packets: (st.interval_acc.packets() > 0).then(|| st.interval_acc.lost()),
            total_packets: (st.interval_acc.packets() > 0).then(|| st.interval_acc.packets()),
            lost_percent: if st.interval_acc.packets() > 0 {
                Some(st.interval_acc.lost() as f64 * 100.0 / st.interval_acc.packets() as f64)
            } else {
                None
            },
        };
        super::emit_interval_seq(
            app,
            session_id,
            IperfRole::Server,
            IperfDirection::Fwd,
            IperfProtocol::Udp,
            &interval,
            st.seq,
        );
        // 与实时事件同源（同上：汇总区间 = 收尾窗口的 counter 差值）
        st.intervals.push(interval);
    }

    let bytes_received = st.counter.total();
    let packets_total = st.acc.packets();
    let lost_total = st.acc.lost();
    let jitter_ms = if packets_total > 0 { Some(st.acc.jitter_ms()) } else { None };
    let lost_pct = if packets_total > 0 {
        Some(lost_total as f64 * 100.0 / packets_total as f64)
    } else {
        None
    };

    log::info!(
        "[iperf2] UDP 服务端接待完成 #{}: {} bytes, {} packets, lost={} (client={}, session={})",
        st.seq, bytes_received, packets_total, lost_total, addr, session_id
    );

    let summary = IperfSummary {
        version: IperfVersion::Iperf2,
        role: IperfRole::Server,
        protocol: IperfProtocol::Udp,
        duration_secs: elapsed,
        total_bytes: bytes_received,
        avg_bandwidth_bps: bytes_received as f64 * 8.0 / elapsed,
        intervals: st
            .intervals
            .into_iter()
            .map(|i| crate::plugins::iperf::IperfIntervalReport {
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
        lost_packets: if packets_total > 0 { Some(lost_total) } else { None },
        total_packets: if packets_total > 0 { Some(packets_total) } else { None },
        lost_percent: lost_pct,
    };
    // 兜底诊断：有字节传输但汇总区间带宽全 0 → 区间数据源回归时开发期可见
    if bytes_received > 0
        && !summary.intervals.is_empty()
        && summary.intervals.iter().all(|i| i.bandwidth_bps <= 0.0)
    {
        log::warn!(
            "[iperf2] UDP 服务端汇总区间退化 ({} bytes 已传输但区间带宽全 0, client={}, session={})",
            bytes_received, addr, session_id
        );
    }
    let mut last = super::lock_or_recover(last_summary, "last_summary");
    *last = Some(summary.clone());
    let _ = app.emit("iperf-test-done", serde_json::json!({
        "session_id": session_id,
        "success": true,
        "role": "server",
        "direction": "fwd",
        "protocol": "udp",
        "seq": st.seq,
        "summary": summary,
    }));

    // 服务器统计回报（UDP_datagram 全零 + server_hdr 40B）：立即发出一次；
    // 客户端 FIN 重传窗口内按需重发（≤TRYCOUNT，不阻塞 reader）
    let report = build_server_report(&ServerHdrV1::from_stats(
        bytes_received,
        lost_total.min(u32::MAX as u64) as u32,
        packets_total.min(u32::MAX as u64) as u32,
        jitter_ms.unwrap_or(0.0),
    ));
    let _ = socket.send_to(&report, addr);
    closing.insert(
        addr,
        ClosingClient {
            report,
            retries: REPORT_MAX_RETRIES,
            deadline: Instant::now() + FIN_RETRY_WINDOW,
        },
    );
}
