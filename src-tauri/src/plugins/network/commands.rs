//! Network Debug application-layer connector and IPC commands.

use chrono::Local;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry};
use crate::kernel::plugin_adapter::ProtocolAdapter;
use crate::kernel::session_store::SessionCreateOptions;
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

// ── 网络调试会话命令 ────────────────────────────────

/// 获取网络调试会话的对端列表（自定义视图初始化用）
#[tauri::command]
pub fn list_network_peers(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<crate::plugins::network::NetworkPeerInfo>, String> {
    Ok(state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .list_peers(&session_id))
}

/// 关闭单个对端（网络调试）。
///
/// 与 `close_channel` 不同：关闭对端不级联断开父会话（监听器保持监听）。
#[tauri::command]
pub async fn close_network_peer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 两段式：锁内信号 + 移除，锁外 join（同 close_channel）。
    // Network 插件自己的 peer 历史在 I/O 自然断开后继续保留；只有显式
    // 关闭/清除命令完成资源回收后才删除元数据。
    let (parent_id, cleanup) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let parent_id = store
            .find_parent_of_channel(&session_id)
            .ok_or_else(|| format!("对端 {} 未找到", session_id))?;
        let (_is_last, cleanup) = store.close_sub_connection(&parent_id, &session_id)?;
        (parent_id, cleanup)
    };
    tauri::async_runtime::spawn_blocking(move || cleanup.join())
        .await
        .map_err(|error| format!("等待网络对端资源清理失败: {error}"))?;
    if let Some(runtime) = state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .runtime(&parent_id)
    {
        runtime.remove_peer(&session_id);
    }
    Ok(())
}

fn rollback_startup_session(state: &State<'_, AppState>, session_id: &str, cause: &str) {
    match state.session_store.lock() {
        Ok(mut store) => {
            if let Err(cleanup_error) = store.close_session(session_id) {
                log::warn!(
                    "网络调试启动失败后的会话清理也失败 (session={}): {}；原始错误: {}",
                    session_id,
                    cleanup_error,
                    cause
                );
            }
        }
        Err(error) => {
            log::warn!(
                "网络调试启动失败后无法锁定 SessionStore 清理会话 {}: {}；原始错误: {}",
                session_id,
                error,
                cause
            );
        }
    }
}

/// 网络调试会话连接（根会话 + NetworkRuntime）。
///
/// 根 Session 先以 paused DataPlane 注册，NetworkRuntime 完成监听/peer 启动并发布
/// `session-connected` 后再 activate。任何启动阶段错误都会回滚已创建的 Session 资源，
/// 避免返回连接失败时留下不可见的活动 runtime。
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    let conn = state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;

    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_session(
            SessionCreateOptions {
                name: name.unwrap_or_else(|| "网络调试".to_string()),
                plugin_id: "network".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled.unwrap_or(false),
                transfer_protocol,
                send_bar_enabled: send_bar_enabled.unwrap_or(true),
                id_override: session_id,
            },
            conn,
            Box::new(|_, _| {}),
            Box::new(|_, _| {}),
            app.clone(),
        )?
    };

    // attach 后从 Network 插件 typed registry 获取 runtime，再启动监听/接收线程。
    let network_runtime = match state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .runtime(&sid)
    {
        Some(runtime) => runtime,
        None => {
            let error = "网络调试 runtime 注册失败".to_string();
            rollback_startup_session(&state, &sid, &error);
            return Err(error);
        }
    };
    if let Err(error) = network_runtime.start(app.clone(), &sid) {
        let error = error.to_string();
        rollback_startup_session(&state, &sid, &error);
        return Err(error);
    }

    // 与其它协议一致：emit session-connected（网络调试容器会话本身是根标签页）。
    // 前端据此把 tab 状态从 connecting 置为 connected 并回填配置。
    let (actual_name, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&sid)
            .ok_or_else(|| "网络调试会话创建失败".to_string())?;
        (handle.name.clone(), handle.connected_at)
    };
    // UDP Client 本地绑定地址（前端展示本机 ip:port 用；其它角色为 null）
    let udp_local_addr = network_runtime
        .udp_client_local_addr()
        .map(|address| address.to_string());

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid,
            "endpoint": endpoint,
            "connection_type": "network",
            "plugin_id": "network",
            "name": actual_name,
            "params": params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "transfer_protocol": Value::Null,
            "send_bar_enabled": send_bar_enabled.unwrap_or(true),
            "local_addr": udp_local_addr,
        }),
    );
    state
        .session_store
        .lock()
        .map_err(|e| e.to_string())?
        .activate_data_plane(&sid)?;
    Ok(sid)
}

/// UDP 发送公共实现：解析侧通道 + 文本转码 + 发送 + TX 日志。
/// `target` 为 `Some` 时按目标地址 `send_to`（server 手动/广播/组播），
/// 为 `None` 时按固定远端 `send_to`（client，不 connect）。
/// 返回实际写入的字节（文本路径为按会话编码转码后的字节），供前端 TX 显示。
fn udp_send_impl(
    state: &State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    target: Option<&str>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    // 锁内：取侧通道 Arc + 会话编码/数据模式（转码 + TX 日志用），随后立即释放锁
    let (encoding, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store.get_session(&session_id);
        let encoding = handle
            .and_then(|h| h.params.get("encoding"))
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8")
            .to_string();
        let data_mode = handle
            .and_then(|h| h.params.get("data_mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("dual")
            .to_string();
        (encoding, data_mode)
    };
    let net = state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or("会话不是网络调试会话".to_string())?;
    // 文本路径：UTF-8 → 会话编码转码（与 write_data 的 SessionIo::send_text 一致）；
    // 字节路径（HEX 发送）原样透传
    let out = if transcode {
        if encoding.eq_ignore_ascii_case("utf-8") {
            data
        } else {
            transcode_utf8_to_encoding(&data, &encoding).unwrap_or(data)
        }
    } else {
        data
    };
    match target {
        Some(addr) => net.udp_send_to(addr, &out)?,
        None => net.udp_send(&out)?,
    }
    // TX 数据日志（非阻塞，best-effort：失败不影响主流程），
    // 记录实际写入设备的字节（转码后），与 send_data 命令的 TX 记账保持同一模式
    if let Ok(log_engine) = state.log_engine.lock() {
        try_send_session_log(
            &log_engine.sender(),
            DataLogEntry {
                session_id,
                direction: DataDirection::TX,
                data_mode,
                encoding,
                payload: out.clone(),
                timestamp: Local::now(),
            },
        );
    }
    Ok(out)
}

/// UDP 手动目标发送（指定任意目标地址，含广播地址）
#[tauri::command]
pub fn network_udp_send_to(
    state: State<'_, AppState>,
    session_id: String,
    target_addr: String,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    udp_send_impl(&state, session_id, data, Some(&target_addr), transcode)
}

/// UDP 固定远端发送（client：不 connect，按记录的固定远端 `send_to`）
#[tauri::command]
pub fn network_udp_send(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    udp_send_impl(&state, session_id, data, None, transcode)
}

/// 同步网络调试会话的「当前发送目标」到后端脚本引擎。
///
/// 前端 TargetBar 变化时调用：UDP server 传手动地址字符串；TCP server 传
/// 对端 id 或 `__all__`（全部客户端）；其余场景传 null（引擎走会话自然对端）。
#[tauri::command]
pub fn set_network_send_target(
    state: State<'_, AppState>,
    session_id: String,
    target: Option<String>,
) -> Result<(), String> {
    let net = state
        .plugin::<crate::plugins::network::NetworkAdapter>(crate::plugins::network::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or("会话不是网络调试会话".to_string())?;
    net.set_send_target(target);
    Ok(())
}
