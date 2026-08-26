//! iperf 服务端监听线程
//!
//! 在独立 `std::thread` 中运行（对齐 TFTP server 模式）：
//! - iperf2：TCP 控制端口监听 + UDP/TCP 数据端口处理
//! - iperf3：内部 tokio runtime 运行 riperf3 server
//!
//! 通过 `abort_flag` 停止——线程在下个循环迭代检测到后退出。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Emitter;

use super::{IperfConfig, IperfDynamicParams, IperfSummary};

/// iperf 服务端线程运行上下文。
pub struct IperfServerContext<R: tauri::Runtime> {
    pub app: tauri::AppHandle<R>,
    pub config: IperfConfig,
    pub dynamic_params: Arc<Mutex<IperfDynamicParams>>,
    pub abort_flag: Arc<AtomicBool>,
    pub server_running: Arc<AtomicBool>,
    pub test_running: Arc<AtomicBool>,
    pub last_summary: Arc<Mutex<Option<IperfSummary>>>,
    pub server_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    pub server_epoch: Arc<AtomicU64>,
    pub my_epoch: u64,
    pub session_id: String,
}

/// 启动 iperf 服务端监听线程。
///
/// 线程启动即置 `server_running = true`（启动占位由 try_start_server 以
/// compare_exchange 完成，此处幂等重写；abort 守卫防 spawn/启动竞态），
/// 引擎绑定成功后 emit `iperf-server-status {running:true}`；
/// 退出（abort 或绑定失败）时置 false 并 emit `running:false`（失败附带 error）。
///
/// 所有退出写回都经代际门控：`my_epoch` 为 spawn 前 try_start_server 递增
/// 后代际；会话关闭/重启换代后，本线程（含 join 超时的僵尸）的迟到写回
/// 一律跳过——不得覆盖新一代服务器的状态。
pub fn spawn_iperf_server<R: tauri::Runtime>(context: IperfServerContext<R>) {
    let IperfServerContext {
        app,
        config,
        dynamic_params,
        abort_flag,
        server_running,
        test_running,
        last_summary,
        server_handle,
        server_epoch,
        my_epoch,
        session_id,
    } = context;
    let handle = std::thread::spawn(move || {
        // 启动前检查 abort 标志，防止 shutdown() 在 spawn 返回后线程尚未开始执行时误判；
        // running 占位已在 try_start_server 置位——取消须复位并 emit，避免卡死状态
        if abort_flag.load(Ordering::Relaxed) {
            if server_epoch.load(Ordering::SeqCst) == my_epoch {
                server_running.store(false, Ordering::Relaxed);
                let _ = app.emit(
                    "iperf-server-status",
                    serde_json::json!({
                        "session_id": session_id,
                        "running": false,
                        "error": "启动前已取消",
                    }),
                );
            }
            return;
        }
        server_running.store(true, Ordering::Relaxed);
        // 引擎按动态参数路由：版本（运行时可切换）+ 监听 IP/端口（会话内可调，
        // 启动时读取一次；运行中改动需重启服务端生效）。
        // 锁中毒经统一恢复出口（super::lock_or_recover），避免静默回退默认参数
        let dyn_params = super::lock_or_recover(&dynamic_params, "dynamic_params").clone();
        let mut cfg = config.clone();
        cfg.listen_ip = dyn_params.listen_ip;
        cfg.listen_port = dyn_params.listen_port;
        let version = dyn_params.version;
        let result = match version {
            super::IperfVersion::Iperf2 => super::iperf2::run_server(
                &app,
                &session_id,
                &cfg,
                &dynamic_params,
                &abort_flag,
                &test_running,
                &last_summary,
            ),
            super::IperfVersion::Iperf3 => super::iperf3::run_server(
                &app,
                &session_id,
                &cfg,
                &abort_flag,
                &test_running,
                &last_summary,
            ),
        };

        if let Err(e) = &result {
            log::error!("[iperf] 服务端线程退出 (session={}): {}", session_id, e);
        }

        // 退出写回仅限当前代际：重连/重启换代后，僵尸线程的迟到
        // running=false 不得翻转新服务器的状态
        if server_epoch.load(Ordering::SeqCst) == my_epoch {
            server_running.store(false, Ordering::Relaxed);
            // 成功路径不携带 error 字段（前端对 error 的存在性做失败分支判定）
            let mut status = serde_json::json!({
                "session_id": session_id,
                "running": false,
                "listen_addr": result.as_ref().ok().cloned(),
            });
            if let Err(e) = &result {
                status["error"] = serde_json::json!(e);
            }
            let _ = app.emit("iperf-server-status", status);
            log::info!("[iperf] 服务端已停止 (session={})", session_id);
        } else {
            log::info!(
                "[iperf] 服务端线程退出（代际已变，跳过状态写回）(session={})",
                session_id
            );
        }
    });

    // 保存线程句柄供停止时 join（避免 JoinHandle 被 drop 时 detach）
    if let Ok(mut h) = server_handle.lock() {
        *h = Some(handle);
    };
}
