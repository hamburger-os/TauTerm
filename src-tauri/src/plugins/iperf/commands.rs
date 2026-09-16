//! iperf application-layer connector and IPC commands.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::plugin_adapter::ProtocolAdapter;
use crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};
use crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};
use crate::AppState;

pub(crate) fn session_connector(
    app: AppHandle,
    request: ConnectSessionRequest,
) -> SessionConnectFuture {
    Box::pin(async move {
        let state: State<'_, AppState> = app.state();
        connect_session(app.clone(), state, request).await
    })
}

// ═══════════════════════════════════════════════════════════════
// iperf 协议命令（iperf2 + iperf3）
// ═══════════════════════════════════════════════════════════════

use crate::plugins::iperf::{self, IperfDynamicParams, IperfStatus};

/// iperf 客户端测速任务注册表（keyed by session_id）。
///
/// 断连状态下的客户端测速任务注册表条目。
///
/// `abort` 供 `iperf_client_stop` 中止；`running` 是重跑守卫的事实源
/// （client 角色事件无 seq，两轮并发会在前端错配，必须串行）。
/// 条目按会话存续：任务结束不删除（运行标志跨 run 复用），会话重连
/// （侧通道接管）时由 `iperf_client_run` 清除。
#[derive(Clone)]
struct RegisteredClientRun {
    abort: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

static IPERF_CLIENT_REGISTRY: LazyLock<Mutex<HashMap<String, RegisteredClientRun>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// iperf 会话连接
///
/// 创建容器会话（无终端 I/O loop）。侧通道 `IperfRuntime` 持有
/// 服务端监听线程句柄与测试状态。
/// 对齐 TFTP：连接即自动启动服务端（配置于 ConnectDialog 表单），
/// 断开自动停止；服务端生命周期跟随会话生命周期。
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        session_id,
        ..
    } = request;
    let old_runtime = session_id.as_deref().and_then(|id| {
        state
            .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
            .runtime(id)
    });
    let prev_params = old_runtime.as_ref().map(|runtime| runtime.get_params());
    if let Some(runtime) = old_runtime {
        let handle = runtime.server_handle.clone();
        let joined = tokio::task::spawn_blocking(move || {
            iperf::join_server_handle(&handle, std::time::Duration::from_secs(10))
        })
        .await
        .unwrap_or(false);
        if !joined {
            log::warn!("[iperf] 重连时旧服务端线程 join 超时");
        }
    }
    let config: iperf::IperfConfig =
        serde_json::from_value(params.clone()).map_err(|e| format!("iperf 配置解析失败: {}", e))?;
    let resolved_params = serde_json::to_value(&config).unwrap_or_else(|_| params.clone());
    let conn = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;
    let session_name = name.unwrap_or_else(|| format!("iperf :{}", endpoint));
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "iperf".into(),
                endpoint: endpoint.clone(),
                params: resolved_params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: false,
                id_override: session_id,
            },
            ContainerSessionRuntime {
                service: conn.service,
                file_transfer: conn.file_transfer,
                channel_factory: conn.channel_factory,
                io: None,
                attachment: conn.on_attached,
                teardown_delay: conn.teardown_delay,
            },
        )?
    };
    let runtime = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&sid)
        .ok_or_else(|| "iperf runtime 注册失败".to_string())?;
    if let Some(prev) = prev_params {
        let current = runtime.get_params();
        let merged = iperf::IperfDynamicParams {
            version: current.version,
            listen_ip: current.listen_ip,
            listen_port: current.listen_port,
            port: if prev.port == iperf::default_client_port(prev.version) {
                current.listen_port
            } else {
                prev.port
            },
            ..prev
        };
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = merged;
    }
    if let Err(error) = iperf::try_start_server(&app, &runtime, &sid).await {
        log::warn!("[iperf] 服务端自动启动失败 (session={}): {}", sid, error);
        let _ = app.emit(
            "iperf-server-status",
            serde_json::json!({
                "session_id": sid, "running": false, "error": error,
            }),
        );
    }
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid, "plugin_id": "iperf", "content_type": "custom",
            "endpoint": endpoint, "name": session_name, "connection_type": "iperf",
            "params": resolved_params, "send_bar_enabled": false, "transfer_enabled": false,
        }),
    );
    Ok(sid)
}

/// 启动 iperf 服务端
///
/// 不乐观置 running——真实状态由服务端线程绑定成功后自行 emit
///（iperf2/iperf3 引擎均在线程内发 running:true + listen_addr；
/// 失败时线程侧 emit running=false + error，避免"先绿后红"闪烁）。
#[tauri::command]
pub async fn iperf_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let runtime = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 iperf runtime", session_id))?;
    iperf::try_start_server(&app, &runtime, &session_id).await?;

    Ok(())
}

/// 停止 iperf 服务端
#[tauri::command]
pub async fn iperf_server_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let iperf_sc = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 iperf runtime", session_id))?;

    // 与 try_start_server 互斥：Stop 不会落在 start 的 join/复位窗口内被
    // 覆盖（start 先完成则线程循环感知 abort；stop 先完成则 start 入口检查放弃）
    let _lifecycle = iperf_sc.lifecycle.lock().await;

    iperf_sc
        .server_abort_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    iperf_sc
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": false,
        }),
    );

    Ok(())
}

/// 运行 iperf 客户端测速（瞬态任务）
///
/// 配置 → 运行 → 实时出结果 → 结束。不建立常驻连接。
/// **fire-and-forget**：invoke 立即返回，进度/结果完全由事件驱动
/// （iperf-test-started → iperf-interval-report × N → iperf-test-done）。
#[tauri::command]
pub async fn iperf_client_run(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    target_host: String,
    params: Value,
) -> Result<(), String> {
    let mut params: IperfDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut params);

    // 客户端自给自足（对齐 TFTP）：侧通道存在时复用其状态（停止按钮可中断）；
    // 会话未连接（无已连接 runtime）时命令内自建一次性状态，测速照常可用。
    // 注意：客户端中止标志独立于服务端监听标志（client_abort_flag vs
    // server_abort_flag）——客户端测速结束/被停止不得杀死会话内的服务端。
    let (client_abort_flag, client_test_running, last_summary) = {
        match state
            .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
            .runtime(&session_id)
        {
            Some(iperf_sc) => {
                if let Ok(mut reg) = IPERF_CLIENT_REGISTRY.lock() {
                    reg.remove(&session_id);
                }
                (
                    iperf_sc.client_abort_flag.clone(),
                    iperf_sc.client_test_running.clone(),
                    iperf_sc.last_summary.clone(),
                )
            }
            None => {
                // 注册/复用断连任务条目：运行标志跨 run 存续，重跑守卫据此
                // 生效（此前每次新建标志导致守卫恒 false、两轮并发错配事件）
                if let Ok(mut reg) = IPERF_CLIENT_REGISTRY.lock() {
                    if !reg.contains_key(&session_id) {
                        reg.insert(
                            session_id.clone(),
                            RegisteredClientRun {
                                abort: Arc::new(AtomicBool::new(false)),
                                running: Arc::new(AtomicBool::new(false)),
                            },
                        );
                    }
                    let entry = reg.get(&session_id).expect("注册表条目已存在");
                    (
                        entry.abort.clone(),
                        entry.running.clone(),
                        Arc::new(Mutex::new(None)),
                    )
                } else {
                    // 注册表锁中毒等极端情况：退化为一次性独立状态（守卫失效但可用）
                    (
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(Mutex::new(None)),
                    )
                }
            }
        }
    };

    // 重复 run 防护：上一轮测速未结束时先中止并等待其收尾（有界）——client
    // 角色事件无 seq，两轮并发交错发事件时前端无法区分（服务端角色已用 seq 配对）
    if client_test_running.load(Ordering::Relaxed) {
        client_abort_flag.store(true, Ordering::Relaxed);
        log::info!(
            "[iperf] 中止上一轮客户端测速后重跑 (session={})",
            session_id
        );
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while client_test_running.load(Ordering::Relaxed) && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        if client_test_running.load(Ordering::Relaxed) {
            // 等待超时：上一轮仍未收尾。强行重跑会让两轮无 seq 事件在前端
            // 错配（旧 done 标失败新记录），故拒绝本次 run
            log::warn!(
                "[iperf] 上一轮客户端测速未在 10s 内收尾，拒绝重跑 (session={})",
                session_id
            );
            return Err("上一轮客户端测速仍在收尾，请稍后重试".into());
        }
    }
    // 到达此处时上一轮已收尾（done 已发出，或从未运行）：复位中止标志安全。
    // 同步置位运行标志：闭合"守卫检查与任务置位之间"的 TOCTOU 窗口
    // （此前在 run_iperf_client 内部置位，双击可穿透守卫）
    client_abort_flag.store(false, Ordering::Relaxed);
    client_test_running.store(true, Ordering::Relaxed);

    // 同步动态参数到 typed runtime（服务端与客户端共享，含版本与监听参数）；
    // 会话未连接时静默跳过（sync_iperf_params 已容忍）
    sync_iperf_params(&state, &session_id, &params);

    // fire-and-forget：后台任务，invoke 立即返回。
    // run_iperf_client 内部保证 iperf-test-done 一定发出（含 panic 兜底），
    // 并在 done 之后复位运行标志。注册表条目跨 run 存续（运行标志是守卫
    // 的事实源），不在此清理——会话重连时由侧通道分支清除。
    tokio::spawn(async move {
        let result = iperf::client::run_iperf_client(
            app,
            session_id,
            target_host,
            params,
            client_abort_flag,
            client_test_running,
            last_summary,
        )
        .await;
        if let Err(e) = result {
            log::warn!("[iperf] 客户端测速任务失败: {}", e);
        }
    });

    Ok(())
}

/// 参数防御性 clamp：并行流数决定线程/task 数、时长决定 force-end 窗口——
/// 上限防本地资源耗尽（1e9 流会炸线程）与恶意客户端滞留
fn sanitize_iperf_params(params: &mut IperfDynamicParams) {
    params.parallel_streams = params.parallel_streams.clamp(1, 64);
    params.duration_secs = params.duration_secs.clamp(1, 86_400);
    params.report_interval_secs = params.report_interval_secs.clamp(1, 60);
}

/// 同步 iperf 动态参数到 typed runtime（客户端测速前调用）。
/// 若 runtime 不存在（会话未连接），静默跳过。
fn sync_iperf_params(state: &AppState, session_id: &str, params: &IperfDynamicParams) {
    if let Some(runtime) = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(session_id)
    {
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = params.clone();
        log::info!(
            "[iperf] 动态参数已同步 (session={}, duration={}s, port={})",
            session_id,
            params.duration_secs,
            params.port
        );
    }
}

/// 中止进行中的客户端测速
///
/// 会话已连接时置位侧通道中止标志；会话未连接时查任务注册表
///（`iperf_client_run` 无已连接 runtime 时注册的一次性任务）。
/// 两者皆无则静默返回——任务已完成或从未启动。
#[tauri::command]
pub async fn iperf_client_stop(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    if let Some(runtime) = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&session_id)
    {
        runtime.client_abort_flag.store(true, Ordering::Relaxed);
        return Ok(());
    }
    if let Ok(reg) = IPERF_CLIENT_REGISTRY.lock() {
        if let Some(entry) = reg.get(&session_id) {
            entry.abort.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// 更新 iperf 动态参数（服务端与客户端共享）
#[tauri::command]
pub async fn iperf_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let mut new_params: IperfDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut new_params);
    if let Some(runtime) = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&session_id)
    {
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = new_params;
    }
    Ok(())
}

/// 获取 iperf 状态
///
/// 若会话已连接（runtime 已注册），从 typed runtime 读取实时状态。
/// 若会话未连接，返回默认值。
#[tauri::command]
pub async fn iperf_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IperfStatus, String> {
    // 全局锁只用于取 Arc：dynamic_params/last_summary 在侧通道自有锁下克隆，
    // 长摘要克隆不占用 session_store 锁（其他会话命令无谓排队）
    if let Some(iperf_sc) = state
        .plugin::<crate::plugins::iperf::IperfAdapter>(crate::plugins::iperf::PLUGIN_ID)
        .runtime(&session_id)
    {
        let server_running = iperf_sc
            .server_running
            .load(std::sync::atomic::Ordering::Relaxed);
        let test_running = iperf_sc
            .test_running
            .load(std::sync::atomic::Ordering::Relaxed);
        let client_test_running = iperf_sc
            .client_test_running
            .load(std::sync::atomic::Ordering::Relaxed);
        // 动态参数为准：版本/监听可在会话内实时修改（config 为创建时不可变快照，
        // 读取它会导致状态报告与用户当前选择不一致）
        let dynamic_params = iperf_sc.get_params();
        let listen_addr = Some(dynamic_params.listen_ip.clone());
        let listen_port = Some(dynamic_params.listen_port);
        let version = dynamic_params.version;
        let last_summary = iperf::lock_or_recover(&iperf_sc.last_summary, "last_summary").clone();
        return Ok(IperfStatus {
            server_running,
            test_running,
            client_test_running,
            listen_addr,
            listen_port,
            version,
            dynamic_params,
            last_summary,
        });
    }

    // 会话未连接（无已连接 runtime），返回默认值
    log::debug!("iperf get_status: 会话 {} 未连接，返回默认状态", session_id);
    Ok(IperfStatus {
        server_running: false,
        test_running: false,
        client_test_running: false,
        listen_addr: None,
        listen_port: None,
        version: iperf::IperfVersion::Iperf2,
        dynamic_params: IperfDynamicParams::default(),
        last_summary: None,
    })
}

pub(crate) fn session_disconnected(app: &AppHandle, session_id: &str) {
    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({ "session_id": session_id, "running": false }),
    );
}
