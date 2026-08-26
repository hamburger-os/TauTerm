//! iperf3 集成（vendor fork of riperf3 crate，纯 Rust，wire-compatible）
//!
//! 与 TFTP/SSH 同为**进程内 Rust 库**，无外部二进制依赖。依赖为仓库内
//! vendor fork（`src-tauri/vendor/riperf3/`，见其 VENDOR-NOTES.md 补丁清单）。
//! - 客户端：`ClientBuilder` + `Client::run()` → `Report`（最终汇总）
//! - 服务端：`ServerBuilder(one_off)` + 循环 `Server::run_once()`（每次接待一个
//!   客户端返回 `Report`，官方注释明示 "library users who want it call run_once"）
//! - 中断：`with_interrupt(watch)` + abort 监视线程（watch receiver 释放后自动退出）
//!
//! 实时性说明（TauTerm fork 补丁）：fork 给 riperf3 增加了逐秒区间通道
//! `interval_channel(tx)`，reporter 每个统计周期把结构化 `Interval` 实时推入
//! std mpsc 通道（与 `json_stream` 标志独立）。本模块用消费线程实时转发
//! `iperf-interval-report` 事件，与 iperf2 实时流共用同一事件通道。
//! 时序保证：`run()` / `run_once()` 返回前已 await reporter 最终 flush（全部
//! 区间已入队），故 join（客户端）/ ping-ack 排水屏障（服务端）后 done 事件
//! 必然落在全部区间事件之后，前端按序渲染。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use riperf3::{Client, ClientBuilder, Report, Server, ServerBuilder, TransportProtocol};
use tauri::Emitter;

use crate::plugins::iperf::{
    IperfConfig, IperfDynamicParams, IperfIntervalReport, IperfProtocol, IperfRole, IperfSummary,
    IperfVersion,
};

/// 中断消息（触发 run 提前返回部分 Report）
const INTERRUPT_MSG: &str = "interrupt - test terminated";

/// 创建 current_thread tokio runtime（在 std::thread 内运行 async 库）
fn make_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("创建 tokio runtime 失败: {}", e))
}

/// 中断监视线程：abort_flag 置位时发送中断消息；watch receiver 释放后自动退出。
/// 返回句柄——调用方在 run 结束、释放 receiver（drop client/server）后 join，
/// 避免 detach 线程残留（receiver_count==0 后最迟 100ms 退出）。
fn spawn_interrupt_watcher(
    abort: &Arc<AtomicBool>,
    tx: tokio::sync::watch::Sender<Option<String>>,
) -> std::thread::JoinHandle<()> {
    let abort = abort.clone();
    std::thread::spawn(move || {
        loop {
            if abort.load(Ordering::Relaxed) {
                let _ = tx.send(Some(INTERRUPT_MSG.to_string()));
                break;
            }
            // receiver 被 drop（run 结束、client/server 释放）→ 退出
            if tx.receiver_count() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

// ── 逐秒区间事件 ─────────────────────────────────────

/// `Interval.sum` → `IperfIntervalReport`（逐秒通道与最终汇总共用同一映射；
/// 与现状回放一致仅取正向 sum，bidir 反向 sum_bidir_reverse 不单独成序列）
fn interval_report(interval: &riperf3::json_report::Interval) -> IperfIntervalReport {
    let sum = &interval.sum;
    IperfIntervalReport {
        start_secs: sum.start,
        end_secs: sum.end,
        transferred_bytes: sum.bytes,
        bandwidth_bps: sum.bits_per_second,
        jitter_ms: sum.jitter_ms,
        lost_packets: sum.lost_packets.map(|v| v as u64),
        total_packets: sum.packets.map(|v| v as u64),
        lost_percent: sum.lost_percent,
    }
}

/// 发送逐秒区间事件（payload 与 iperf2 实时流一致）
fn emit_interval_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    role: &str,
    protocol: IperfProtocol,
    iv: &IperfIntervalReport,
) {
    let _ = app.emit(
        "iperf-interval-report",
        serde_json::json!({
            "session_id": session_id,
            "role": role,
            "direction": "fwd",
            "protocol": protocol,
            "start_secs": iv.start_secs,
            "end_secs": iv.end_secs,
            "transferred_bytes": iv.transferred_bytes,
            "bandwidth_bps": iv.bandwidth_bps,
            "jitter_ms": iv.jitter_ms,
            "lost_packets": iv.lost_packets,
            "total_packets": iv.total_packets,
            "lost_percent": iv.lost_percent,
        }),
    );
}

// ── Report → IperfSummary 转换 ─────────────────────────

/// 从 Report 提取协议（UDP 由 end.streams 的 udp 变体判定）
fn protocol_from_report(report: &Report) -> IperfProtocol {
    if report.end.streams.iter().any(|s| s.udp.is_some()) {
        IperfProtocol::Udp
    } else {
        IperfProtocol::Tcp
    }
}

/// Report → IperfSummary（区间取 IntervalSum 结构化字段，无需解析文本）
fn summary_from_report(report: &Report, role: IperfRole) -> IperfSummary {
    let end = &report.end;
    let protocol = protocol_from_report(report);

    // 带宽/字节/时长：按角色与方向选择聚合（riperf3 语义：sum_sent = 本机
    // 发送方向、sum_received = 本机接收方向；方向标志在 start.test_start）
    let received = end.sum_received.as_ref();
    let test_start = report.start.test_start.as_ref();
    let reverse = test_start.map(|t| t.reverse > 0).unwrap_or(false);
    let bidir = test_start.map(|t| t.bidir > 0).unwrap_or(false);

    let (total_bytes, avg_bps, duration) = if role != IperfRole::Server {
        // 客户端：正向 = 本机发送（sum_sent 优先，回退 sum）；
        // 反向(-R) = 本机接收（sum_received）；--bidir = 两者合计。
        // 此前无 reverse/bidir 分支——接收端的 sum_sent 只计自身发送字节
        //（≈0），反向测试汇总恒显示 0 bytes/0 bps，与实时图表矛盾
        if bidir {
            let sent = end.sum_sent.as_ref().or(end.sum.as_ref());
            let bytes = sent.map(|s| s.bytes).unwrap_or(0) + received.map(|s| s.bytes).unwrap_or(0);
            let secs = sent
                .map(|s| s.seconds)
                .or(received.map(|s| s.seconds))
                .unwrap_or(0.0);
            (
                bytes,
                if secs > 0.0 {
                    bytes as f64 * 8.0 / secs
                } else {
                    0.0
                },
                secs,
            )
        } else if reverse {
            let recv = received.or(end.sum.as_ref());
            (
                recv.map(|s| s.bytes).unwrap_or(0),
                recv.map(|s| s.bits_per_second).unwrap_or(0.0),
                recv.map(|s| s.seconds).unwrap_or(0.0),
            )
        } else {
            let sent = end.sum_sent.as_ref().or(end.sum.as_ref());
            (
                sent.map(|s| s.bytes).unwrap_or(0),
                sent.map(|s| s.bits_per_second)
                    .or(received.map(|r| r.bits_per_second))
                    .unwrap_or(0.0),
                sent.map(|s| s.seconds)
                    .or(received.map(|r| r.seconds))
                    .unwrap_or(0.0),
            )
        }
    } else if bidir {
        // 服务端双向：正向接收（sum_received）+ 反向发送（sum_sent_bidir_reverse）
        let rev = end
            .sum_sent_bidir_reverse
            .as_ref()
            .or(end.sum_bidir_reverse.as_ref());
        let bytes = received.map(|s| s.bytes).unwrap_or(0) + rev.map(|s| s.bytes).unwrap_or(0);
        let secs = received
            .map(|s| s.seconds)
            .or(rev.map(|s| s.seconds))
            .unwrap_or(0.0);
        (
            bytes,
            if secs > 0.0 {
                bytes as f64 * 8.0 / secs
            } else {
                0.0
            },
            secs,
        )
    } else {
        // 服务端单向：正向 → 本机接收；反向 → 本机发送
        let s = if reverse {
            end.sum_sent.as_ref()
        } else {
            received
        };
        (
            s.map(|s| s.bytes).unwrap_or(0),
            s.map(|s| s.bits_per_second).unwrap_or(0.0),
            s.map(|s| s.seconds).unwrap_or(0.0),
        )
    };

    // 抖动/丢包：UDP 时取接收侧聚合（sum 为接收侧汇总；回退 sum_received / udp 流）
    let (jitter, lost, total_packets, lost_pct) = if protocol == IperfProtocol::Udp {
        let recv = end.sum.as_ref().or(received);
        let from_sum = (
            recv.and_then(|s| s.jitter_ms),
            recv.and_then(|s| s.lost_packets.map(|v| v as u64)),
            recv.and_then(|s| s.packets.map(|v| v as u64)),
            recv.and_then(|s| s.lost_percent),
        );
        // sum 无 jitter 字段时回退到 udp 流
        if from_sum.0.is_some() || from_sum.1.is_some() {
            from_sum
        } else {
            end.streams
                .iter()
                .find_map(|s| s.udp.as_ref())
                .map(|u| {
                    (
                        Some(u.jitter_ms),
                        Some(u.lost_packets as u64),
                        Some(u.packets as u64),
                        Some(u.lost_percent),
                    )
                })
                .unwrap_or((None, None, None, None))
        }
    } else {
        (None, None, None, None)
    };

    // 区间（-i 报告）：与逐秒通道共用同一映射
    let intervals = report.intervals.iter().map(interval_report).collect();

    IperfSummary {
        version: IperfVersion::Iperf3,
        role,
        protocol,
        duration_secs: duration,
        total_bytes,
        avg_bandwidth_bps: avg_bps,
        intervals,
        jitter_ms: jitter,
        lost_packets: lost,
        total_packets,
        lost_percent: lost_pct,
    }
}

// ── iperf3 客户端 ─────────────────────────────────────

/// iperf3 客户端测速（纯库：Client::run 一次性返回 Report）。
pub fn run_client<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    target_host: &str,
    params: &IperfDynamicParams,
    abort_flag: &Arc<AtomicBool>,
) -> Result<IperfSummary, String> {
    let rt = make_runtime()?;

    let mut builder = ClientBuilder::new(target_host)
        .port(Some(params.port))
        .protocol(if params.protocol == IperfProtocol::Udp {
            TransportProtocol::Udp
        } else {
            TransportProtocol::Tcp
        })
        .duration(params.duration_secs.max(1))
        .num_streams(params.parallel_streams.max(1))
        .interval(params.report_interval_secs.max(1) as f64)
        .no_delay(true)
        // fork 静音补丁：console 不打印测试文本；log::info! 仍流向 TauTerm
        // 文件日志，--get-server-output 捕获不受影响
        .quiet(true);
    if let Some(bps) = params.bandwidth_bps {
        if bps > 0 {
            builder = builder.bandwidth(bps);
        }
    }
    if params.reverse {
        builder = builder.reverse(true);
    }
    if params.bidir {
        builder = builder.bidir(true);
    }
    if params.omit_secs > 0 {
        builder = builder.omit(params.omit_secs);
    }
    // 逐秒区间通道（TauTerm fork 补丁）：reporter 每统计周期推送 Interval
    let (interval_tx, interval_rx) = std::sync::mpsc::channel::<riperf3::json_report::Interval>();
    builder = builder.interval_channel(interval_tx);

    let client: Client = builder
        .build()
        .map_err(|e| format!("iperf3 配置错误: {}", e))?;

    // 消费线程：实时转 iperf-interval-report 事件（与 iperf2 实时流一致）。
    // run() 返回前 reporter 已 flush 全部区间入队；sender 全部释放后 recv()
    // 出错退出——join 即屏障，done 事件必然落在全部区间事件之后
    let sid = session_id.to_string();
    let app_c = (*app).clone();
    let protocol = params.protocol;
    let consumer = std::thread::spawn(move || {
        while let Ok(interval) = interval_rx.recv() {
            emit_interval_event(
                &app_c,
                &sid,
                "client",
                protocol,
                &interval_report(&interval),
            );
        }
    });

    // 中断通道：abort_flag → run 提前返回（部分 Report）
    let (tx, rx) = tokio::sync::watch::channel(None);
    let watcher = spawn_interrupt_watcher(abort_flag, tx);
    let client = client.with_interrupt(rx);

    let report = rt
        .block_on(client.run())
        .map_err(|e| format!("iperf3 测试失败: {}", e))?;

    // 释放中断接收端（client 持有 rx）与全部 sender 克隆（client + runtime
    // 内 reporter task），随后 join 监视线程与消费线程
    drop(client);
    drop(rt);
    let _ = watcher.join();
    let _ = consumer.join();

    let summary = summary_from_report(&report, IperfRole::Client);
    log::info!(
        "[iperf3] 客户端测速完成 (session={}, avg={:.2} Mbps, {}s)",
        session_id,
        summary.avg_bandwidth_bps / 1_000_000.0,
        summary.duration_secs
    );
    Ok(summary)
}

// ── iperf3 服务端 ─────────────────────────────────────

/// iperf3 服务端监听（纯库：循环 `Server::run_once()`，每次接待一个客户端）。
///
/// 返回监听地址字符串（正常停止时）；abort 中止视为正常停止。
///
/// riperf3 无 per-test-start 回调（`run_once()` 阻塞至测试完成），故
/// `iperf-test-started` 在每次接待轮次开始前发出（`pending: true`——
/// 服务端待命/接待中，可能先于任何客户端出现）。前端对 pending 记录
/// 不设看门狗：空闲服务端可无限等待客户端。
pub fn run_server<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
    config: &IperfConfig,
    abort_flag: &Arc<AtomicBool>,
    test_running: &Arc<AtomicBool>,
    last_summary: &Arc<Mutex<Option<IperfSummary>>>,
) -> Result<String, String> {
    let rt = make_runtime()?;

    // 逐秒区间通道（TauTerm fork 补丁）：同一 tx 贯穿所有 run_once 轮次；
    // 消费线程实时转发区间事件。std mpsc 无 select，用 50ms 轮询（每秒至多
    // 1 条区间，开销可忽略）。ping 屏障仅在区间队列排空时应答——run_once()
    // 返回前 reporter 已 flush 全部区间入队，消费线程发出全部区间后主线程
    // 才发 done，事件顺序可靠
    let (interval_tx, interval_rx) = std::sync::mpsc::channel::<riperf3::json_report::Interval>();
    let (ping_tx, ping_rx) = std::sync::mpsc::channel::<()>();
    let (ack_tx, ack_rx) = std::sync::mpsc::channel::<()>();
    // 消费线程逐区间 emit 计数：Err/abort 路径用它判断本轮是否已产生区间
    // （有则补发失败 done，避免前端惰性建出的 running 记录永久停留）
    let emitted_count = Arc::new(AtomicU64::new(0));
    let sid = session_id.to_string();
    let app_c = (*app).clone();
    let emitted = emitted_count.clone();
    let consumer = std::thread::spawn(move || loop {
        match interval_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(interval) => {
                // 协议按区间流类型判定（UDP 流带 packets 统计字段）
                let protocol = if interval.streams.iter().any(|s| s.packets.is_some()) {
                    IperfProtocol::Udp
                } else {
                    IperfProtocol::Tcp
                };
                emit_interval_event(
                    &app_c,
                    &sid,
                    "server",
                    protocol,
                    &interval_report(&interval),
                );
                emitted.fetch_add(1, Ordering::Relaxed);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // 区间队列暂时为空（每个区间 recv 后立即发出并计数，无在途
                // 项）：此刻应答排水屏障，主线程可安全发 done
                if let Ok(()) = ping_rx.try_recv() {
                    let _ = ack_tx.send(());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    });

    let (tx, rx) = tokio::sync::watch::channel(None);
    let server: Server = ServerBuilder::new()
        .port(Some(config.listen_port))
        // 尊重用户配置的监听地址（此前从未调用，服务端静默绑通配地址，
        // 设 127.0.0.1 期望仅本地时实际全网可达）
        .bind_address(&config.listen_ip)
        .one_off(true)
        .interrupt(rx)
        .interval_channel(interval_tx)
        // fork 静音补丁：console 不打印测试文本；log::info! 仍流向 TauTerm
        // 文件日志，--get-server-output 捕获不受影响
        .quiet(true)
        .build()
        .map_err(|e| format!("iperf3 服务端配置错误: {}", e))?;
    let watcher = spawn_interrupt_watcher(abort_flag, tx);

    // fork bind 补丁：先 bind 后 emit running:true（与 iperf2 一致，端口占用
    // 等绑定失败在 emit 前暴露，无"先绿后红"闪烁）；监听器贯穿全部轮次复用，
    // 消除每轮重新 bind 的窗口
    let listener = rt
        .block_on(server.bind())
        .map_err(|e| format!("iperf3 服务端启动失败: {}", e))?;

    // 以实际绑定地址为准（bind_address 生效后可能与配置形式不同，
    // 如 "::" 的双栈行为），UI 宣传真实监听点
    let listen_addr = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| format!("{}:{}", config.listen_ip, config.listen_port));
    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": true,
            "listen_addr": listen_addr,
        }),
    );
    log::info!(
        "[iperf3] 服务端监听中 (session={}, {})",
        session_id,
        listen_addr
    );

    loop {
        if abort_flag.load(Ordering::Relaxed) {
            break;
        }
        // 接待轮次开始：置 test_running + 通知前端（协议未知——客户端接入前
        // 不可知，前端以客户端参数兜底）
        test_running.store(true, Ordering::Relaxed);
        let _ = app.emit(
            "iperf-test-started",
            serde_json::json!({
                "session_id": session_id,
                "role": "server",
                "direction": "fwd",
                "pending": true,
                "protocol": null,
            }),
        );
        // 本轮区间计数基线（消费线程已排空上一轮——见排水屏障）
        let round_base = emitted_count.load(Ordering::Relaxed);
        match rt.block_on(server.run_once_with_listener(&listener)) {
            Ok(report) => {
                test_running.store(false, Ordering::Relaxed);
                let summary = summary_from_report(&report, IperfRole::Server);
                // 排水屏障：等消费线程排空本轮区间事件后再发 done，保证
                // 前端按序渲染（done 一定落在全部区间之后）
                let _ = ping_tx.send(());
                let _ = ack_rx.recv();
                let mut last = super::lock_or_recover(last_summary, "last_summary");
                *last = Some(summary.clone());
                let _ = app.emit(
                    "iperf-test-done",
                    serde_json::json!({
                        "session_id": session_id,
                        "success": true,
                        "role": "server",
                        "direction": "fwd",
                        "protocol": summary.protocol,
                        "summary": summary,
                    }),
                );
                log::info!(
                    "[iperf3] 服务端接待测试完成 (session={}, avg={:.2} Mbps)",
                    session_id,
                    summary.avg_bandwidth_bps / 1_000_000.0
                );
            }
            Err(_e) if abort_flag.load(Ordering::Relaxed) => {
                // 中止退出：run_once 被 interrupt 打断。若本轮已产生区间
                // （前端已惰性建出 running 记录），排水后补发失败 done，
                // 避免记录永久停留 running
                test_running.store(false, Ordering::Relaxed);
                let _ = ping_tx.send(());
                let _ = ack_rx.recv();
                if emitted_count.load(Ordering::Relaxed) > round_base {
                    let _ = app.emit(
                        "iperf-test-done",
                        serde_json::json!({
                            "session_id": session_id,
                            "success": false,
                            "role": "server",
                            "direction": "fwd",
                            "error": "服务端已停止（测试中止）",
                            "summary": null,
                        }),
                    );
                }
                break;
            }
            Err(e) => {
                // 绑定失败已在 bind 前置步骤暴露（emit 前返回 Err），此处
                // 为接待轮次内的客户端异常（断连等）。若本轮已产生区间
                // （前端已惰性建出 running 记录），排水后补发失败 done，
                // 否则无记录可收尾，直接继续等待下一个客户端
                test_running.store(false, Ordering::Relaxed);
                let err_msg = format!("客户端异常断连: {}", e);
                let _ = ping_tx.send(());
                let _ = ack_rx.recv();
                if emitted_count.load(Ordering::Relaxed) > round_base {
                    let _ = app.emit(
                        "iperf-test-done",
                        serde_json::json!({
                            "session_id": session_id,
                            "success": false,
                            "role": "server",
                            "direction": "fwd",
                            "error": err_msg.clone(),
                            "summary": null,
                        }),
                    );
                }
                log::warn!(
                    "[iperf3] 接待测试异常 (session={}): {}",
                    session_id,
                    err_msg
                );
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }

    // 释放中断接收端（server 持有 rx）与全部 sender 克隆（server + runtime
    // 内 reporter task），随后 join 监视线程与消费线程
    drop(server);
    drop(rt);
    let _ = watcher.join();
    let _ = consumer.join();

    Ok(listen_addr)
}
