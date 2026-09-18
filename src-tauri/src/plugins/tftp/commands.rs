//! TFTP application-layer connector and IPC commands.

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::plugin_adapter::ProtocolAdapter;
use crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};
use crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TftpClientRequest {
    pub session_id: String,
    pub remote_ip: String,
    pub remote_port: u16,
    pub remote_filename: String,
    pub local_path: String,
    pub params: Value,
}

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
// TFTP 协议命令
// ═══════════════════════════════════════════════════════════════

use crate::plugins::tftp::{self, TftpDynamicParams, TftpStatus};

/// TFTP 会话连接
///
/// 创建容器会话（无终端 I/O loop），然后自动启动服务端。
/// 侧通道 `TftpRuntime` 持有 UDP socket，由独立线程处理所有传输。
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
    let prev_params = session_id
        .as_deref()
        .and_then(|id| {
            state
                .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
                .runtime(id)
        })
        .map(|runtime| runtime.get_params());
    let conn = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;
    let session_name = name.unwrap_or_else(|| format!("TFTP :{}", endpoint));
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "tftp".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
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
                automation_io: None,
                attachment: conn.on_attached,
                teardown_delay: conn.teardown_delay,
            },
        )?
    };
    let runtime = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(&sid)
        .ok_or_else(|| "TFTP runtime 注册失败".to_string())?;
    if let Some(prev) = prev_params {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = prev,
            Err(poisoned) => *poisoned.into_inner() = prev,
        }
    }
    if let Err(error) = tftp::try_start_server(&app, &runtime, &sid) {
        log::warn!("[TFTP] 服务端自动启动失败 (session={}): {}", sid, error);
    }
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid, "plugin_id": "tftp", "content_type": "custom",
            "endpoint": endpoint, "name": session_name, "connection_type": "tftp",
            "params": params, "send_bar_enabled": false, "transfer_enabled": false,
        }),
    );
    Ok(sid)
}

/// 启动 TFTP 服务端
#[tauri::command]
pub async fn tftp_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let runtime = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 TFTP runtime", session_id))?;
    tftp::try_start_server(&app, &runtime, &session_id)?;

    // 状态由服务端线程权威 emit（真实进入监听后 running:true）；此处不再
    // 无条件乐观 emit——Start 与 Stop 交错时线程在启动前 abort 检查处退出，
    // 乐观的 running:true 会永久失真（keepAlive 会话 getStatus 只查一次、
    // 服务端线程无其他状态事件可纠正）
    Ok(())
}

/// 停止 TFTP 服务端
#[tauri::command]
pub async fn tftp_server_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let tftp_sc = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 TFTP runtime", session_id))?;

    tftp_sc
        .abort_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    tftp_sc
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit(
        "tftp-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": false,
        }),
    );

    Ok(())
}

/// TFTP 客户端 GET（下载）——自给自足，不依赖已连接会话 runtime
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无已连接 runtime）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_get(
    state: State<'_, AppState>,
    app: AppHandle,
    request: TftpClientRequest,
) -> Result<String, String> {
    let TftpClientRequest {
        session_id,
        remote_ip,
        remote_port,
        remote_filename,
        local_path,
        params,
    } = request;
    log::info!(
        "[TFTP Client] GET 请求: session={}, file={}, remote={}:{} → {}",
        session_id,
        remote_filename,
        remote_ip,
        remote_port,
        local_path
    );
    let params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 typed runtime（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_get(
        app,
        tftp::client::TftpClientTransferRequest {
            session_id,
            transfer_id: transfer_id.clone(),
            remote_ip,
            remote_port,
            remote_filename,
            local_path: std::path::PathBuf::from(local_path),
            params,
        },
    )
    .await?;

    Ok(transfer_id)
}

/// TFTP 客户端 PUT（上传）——自给自足，不依赖已连接会话 runtime
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无已连接 runtime）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_put(
    state: State<'_, AppState>,
    app: AppHandle,
    request: TftpClientRequest,
) -> Result<String, String> {
    let TftpClientRequest {
        session_id,
        remote_ip,
        remote_port,
        remote_filename,
        local_path,
        params,
    } = request;
    log::info!(
        "[TFTP Client] PUT 请求: session={}, file={}, remote={}:{} ← {}",
        session_id,
        remote_filename,
        remote_ip,
        remote_port,
        local_path
    );
    let params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 typed runtime（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_put(
        app,
        tftp::client::TftpClientTransferRequest {
            session_id,
            transfer_id: transfer_id.clone(),
            remote_ip,
            remote_port,
            remote_filename,
            local_path: std::path::PathBuf::from(local_path),
            params,
        },
    )
    .await?;

    Ok(transfer_id)
}

/// 同步 TFTP 服务端参数到 typed runtime（客户端 GET/PUT 前调用）。
/// 若 runtime 不存在（会话未连接），静默跳过。
fn sync_tftp_server_params(state: &AppState, session_id: &str, params: &TftpDynamicParams) {
    if let Some(runtime) = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(session_id)
    {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = params.clone(),
            Err(poisoned) => *poisoned.into_inner() = params.clone(),
        }
        log::info!(
            "[TFTP] 服务端参数已同步 (session={}, blksize={})",
            session_id,
            params.blksize
        );
    }
}

/// 更新 TFTP 动态参数
///
/// 若会话已连接（runtime 已注册），更新服务端的共享参数。
/// 若会话未连接，仅记录日志后返回 Ok——客户端操作从前端传参，不依赖此处。
#[tauri::command]
pub async fn tftp_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let new_params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    if let Some(runtime) = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(&session_id)
    {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = new_params,
            Err(poisoned) => *poisoned.into_inner() = new_params,
        }
        log::info!("TFTP 参数已更新 (session={})", session_id);
    } else {
        log::warn!("TFTP 参数更新跳过：会话 {} 未连接", session_id);
    }
    Ok(())
}

/// 获取 TFTP 状态
///
/// 若会话已连接（runtime 已注册），从 typed runtime 读取实时状态。
/// 若会话未连接，返回默认值（server_running=false，其余字段为空/默认）。
#[tauri::command]
pub async fn tftp_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<TftpStatus, String> {
    if let Some(runtime) = state
        .plugin::<crate::plugins::tftp::TftpAdapter>(crate::plugins::tftp::PLUGIN_ID)
        .runtime(&session_id)
    {
        let dynamic_params = runtime.get_params();
        return Ok(TftpStatus {
            server_running: runtime
                .server_running
                .load(std::sync::atomic::Ordering::Relaxed),
            listen_addr: Some(runtime.config.listen_ip.clone()),
            listen_port: Some(runtime.config.listen_port),
            file_root: runtime.config.file_root.clone(),
            dynamic_params,
        });
    }
    Ok(TftpStatus {
        server_running: false,
        listen_addr: None,
        listen_port: None,
        file_root: String::new(),
        dynamic_params: TftpDynamicParams::default(),
    })
}

pub(crate) fn session_disconnected(app: &AppHandle, session_id: &str) {
    let _ = app.emit(
        "tftp-server-status",
        serde_json::json!({ "session_id": session_id, "running": false }),
    );
}
