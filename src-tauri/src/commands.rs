//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。
//! 通过协议 Adapter + SessionStore + DataPlane/SessionIo 架构管理会话。

pub(crate) mod config;
pub(crate) mod files;
pub(crate) mod platform;

use crate::kernel::log_engine::{
    try_send_system_event, LogConfigResponse, LogConfigUpdate, LogEntry, LogHealth, LogStatus,
};
use crate::kernel::plugin_adapter::{ChannelOpenMode, PluginId, TransferProtocolType};
use crate::kernel::script_engine::codegen::{hex_to_bytes, interpret_escape_sequences};
use crate::kernel::script_engine::sandbox::create_sandboxed_lua;
use crate::kernel::session_store::{SessionState, SessionStore};
use crate::plugin_application::{
    ConnectSessionRequest, SessionConfigHandler, SessionConfigServices, SessionConnectHandler,
    SessionDisconnectedHook,
};
use crate::session::DisconnectInfo;
use crate::AppState;
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

// ── 数据结构 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTypeInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub description: String,
    pub icon: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointItem {
    pub name: String,
    pub description: String,
    pub connection_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub state: String,
    pub plugin_id: String,
    pub send_bar_enabled: bool,
    pub transfer_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_index: Option<u32>,
    #[serde(default)]
    pub elevated: bool,
    #[serde(default)]
    pub is_container: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSessionInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub params: Value,
    pub timestamp: u64,
    pub plugin_id: String,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSessionConfigRequest {
    pub endpoint: String,
    pub params: Value,
    pub name: Option<String>,
    pub plugin_id: String,
    pub transfer_enabled: Option<bool>,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: Option<bool>,
    pub session_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferSendRequest {
    pub session_id: String,
    pub protocol_options: crate::transfer::config::SendProtocolOptions,
    pub file_paths: Vec<String>,
    pub remote_dir: Option<String>,
    pub overwrite_policy: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferReceiveRequest {
    pub session_id: String,
    pub protocol_options: crate::transfer::config::ReceiveProtocolOptions,
    pub download_dir: String,
    pub remote_paths: Vec<String>,
    pub destination_paths: Option<Vec<String>>,
    pub overwrite_policy: Option<String>,
}
// ── 命令：连接类型 ──────────────────────────────────

#[tauri::command]
pub fn get_connection_types(state: State<'_, AppState>) -> Vec<ConnectionTypeInfo> {
    state
        .plugins
        .manifests()
        .into_iter()
        .map(|plugin| ConnectionTypeInfo {
            id: plugin.id.to_string(),
            label: plugin.name.clone(),
            available: true,
            description: format!("{} v{}", plugin.name, plugin.version),
            icon: plugin.category.clone(),
            content_type: plugin.content_type.clone(),
        })
        .collect()
}

// ── 命令：端点枚举 ──────────────────────────────────

#[tauri::command]
pub async fn enumerate_endpoints(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<EndpointItem>, String> {
    let plugin_id = PluginId::parse(plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;
    let adapter = state
        .plugins
        .adapter(&plugin_id)
        .ok_or_else(|| format!("插件 '{plugin_id}' 不提供 ProtocolAdapter"))?;

    // 端点发现可能触发驱动枚举、平台命令或未来的网络发现；统一放入 blocking worker，
    // 公共命令不再猜测哪些具体协议会阻塞。
    let endpoints = tauri::async_runtime::spawn_blocking(move || adapter.discover_endpoints())
        .await
        .map_err(|error| format!("插件端点发现任务失败: {error}"))?
        .map_err(|error| error.to_string())?;

    Ok(endpoints
        .into_iter()
        .map(|endpoint| EndpointItem {
            name: endpoint.name,
            description: endpoint.description,
            connection_type: plugin_id.to_string(),
            params: endpoint.params,
        })
        .collect())
}

// ── 命令：会话连接 ──────────────────────────────────

/// 连接会话。前端参数收束为请求结构体，保持 IPC 契约清晰。
#[tauri::command]
pub async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let plugin_id = PluginId::parse(request.plugin_id.clone())
        .map_err(|error| format!("无效插件 ID: {error}"))?;
    let handler = state
        .plugins
        .contribution::<SessionConnectHandler>(&plugin_id)
        .ok_or_else(|| format!("插件 '{plugin_id}' 未注册 Session 连接 contribution"))?;

    (handler.as_ref())(app, request).await
}
// ── 命令：会话断开 ──────────────────────────────────

#[tauri::command]
pub async fn disconnect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let (session_name, plugin_id) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?;
        let snapshot = (handle.name.clone(), handle.plugin_id.clone());
        store.close_session(&session_id)?;
        store.reset_child_counter(&session_id);
        snapshot
    };

    log::info!("会话已断开: {} ({})", session_name, plugin_id);
    if let Ok(plugin_id) = PluginId::parse(plugin_id) {
        if let Some(hook) = state
            .plugins
            .contribution::<SessionDisconnectedHook>(&plugin_id)
        {
            (hook.as_ref())(&app, &session_id);
        }
    }
    let info = DisconnectInfo::user_requested();
    let _ = app.emit(
        "session-disconnected",
        serde_json::json!({
            "session_id": session_id,
            "reason": &info.reason,
            "disconnect_info": &info,
        }),
    );
    Ok(())
}

/// 切换活跃标签页
#[tauri::command]
pub fn switch_active_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.switch_active(&session_id)?;
    let _ = app.emit(
        "session-switched",
        serde_json::json!({
            "session_id": session_id,
        }),
    );
    Ok(())
}

/// 重命名会话
#[tauri::command]
pub async fn rename_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    new_name: String,
) -> Result<(), String> {
    SessionStore::rename_config_on_disk_transactional(&app, &session_id, &new_name, || {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        if store.get_session(&session_id).is_some() {
            store.rename_session(&session_id, &new_name)?;
        }
        Ok(())
    })?;

    let _ = app.emit(
        "session-renamed",
        serde_json::json!({
            "session_id": session_id,
            "name": new_name,
        }),
    );
    Ok(())
}

/// 标签页重排序
#[tauri::command]
pub fn reorder_tabs(state: State<'_, AppState>, session_ids: Vec<String>) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.reorder_tabs(session_ids)?;
    Ok(())
}

/// 获取所有标签页信息（含子连接）
#[tauri::command]
pub fn get_tabs(state: State<'_, AppState>) -> Result<Vec<TabInfo>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let mut tabs: Vec<TabInfo> = store
        .tab_ids()
        .iter()
        .filter_map(|id| {
            store.get_session(id).map(|h| TabInfo {
                id: id.clone(),
                name: h.name.clone(),
                connection_type: h.plugin_id.clone(),
                endpoint: h.endpoint.clone(),
                state: match h.state {
                    SessionState::Connected => "connected".into(),
                    SessionState::Connecting => "connecting".into(),
                    SessionState::Disconnected => "disconnected".into(),
                    SessionState::Transferring => "transferring".into(),
                },
                plugin_id: h.plugin_id.clone(),
                send_bar_enabled: h.send_bar_enabled,
                transfer_enabled: h.transfer_enabled,
                parent_id: None,
                channel_index: None,
                elevated: false,
                is_container: h.channel_factory.is_some(),
            })
        })
        .collect();
    // 添加子连接
    for id in store.tab_ids() {
        if let Some(h) = store.get_session(&id) {
            for sub in &h.sub_connections {
                if sub.state == SessionState::Disconnected {
                    continue; // 跳过已断开的子连接，等待 channel-closed 事件触发 REMOVE_CHILD
                }
                if !sub.visible_in_workspace {
                    continue; // 插件后台子连接不直接占用 Workspace 标签页
                }
                tabs.push(TabInfo {
                    id: sub.id.clone(),
                    name: sub.name.clone(),
                    connection_type: h.plugin_id.clone(),
                    endpoint: h.endpoint.clone(),
                    state: match sub.state {
                        SessionState::Connected => "connected".into(),
                        SessionState::Connecting => "connecting".into(),
                        SessionState::Disconnected => "disconnected".into(),
                        SessionState::Transferring => "transferring".into(),
                    },
                    plugin_id: h.plugin_id.clone(),
                    send_bar_enabled: h.send_bar_enabled,
                    transfer_enabled: h.transfer_enabled,
                    parent_id: Some(h.id.clone()),
                    channel_index: Some(sub.channel_index),
                    elevated: sub.elevated,
                    is_container: false,
                });
            }
        }
    }
    Ok(tabs)
}

// ── SSH 子通道创建（共享逻辑）───────────────────────
// ── 多终端通道命令 ─────────────────────────────────

/// 在支持子终端工厂的父会话上打开新的终端 channel。
#[tauri::command]
pub async fn open_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    elevated: Option<bool>,
) -> Result<String, String> {
    let mode = if elevated.unwrap_or(false) {
        ChannelOpenMode::Elevated
    } else {
        ChannelOpenMode::Standard
    };
    let factory = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
        if handle.state != SessionState::Connected {
            return Err("父会话未连接".into());
        }
        let factory = handle
            .channel_factory
            .as_ref()
            .ok_or("此会话不支持多个终端")?
            .clone();
        if !factory.supports_mode(mode) {
            return Err("此 Shell 不支持管理员连接".into());
        }
        factory
    };

    let channel = factory
        .open_channel(mode)
        .await
        .map_err(|error| format!("打开新终端失败: {error}"))?;
    let channel_id = crate::plugin_application::create_terminal_sub_channel(
        &app,
        &state,
        &session_id,
        channel,
        mode == ChannelOpenMode::Elevated,
        true,
    )
    .await?;

    log::info!("子终端已打开: {} (parent: {})", channel_id, session_id);
    Ok(channel_id)
}

// ── 会话持久化命令 ─────────────────────────────────

#[tauri::command]
pub async fn load_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionStore::sessions_file_path(&app)?;
    let mut saved = SessionStore::load_from_disk(&path)?;
    let mut changed = false;
    for session in &mut saved {
        if let Some(handler) = state
            .plugins
            .contribution_by_str::<SessionConfigHandler>(&session.plugin_id)
        {
            if let Some(sanitize) = handler.sanitize_saved {
                changed |= sanitize(session)?;
            }
        }
    }
    if changed {
        SessionStore::replace_saved_sessions(&path, &saved)?;
    }

    Ok(saved
        .into_iter()
        .map(|s| SavedSessionInfo {
            id: s.id,
            name: s.name,
            connection_type: s.plugin_id.clone(),
            endpoint: s.endpoint,
            params: s.params,
            timestamp: s.timestamp,
            plugin_id: s.plugin_id,
            transfer_enabled: s.transfer_enabled,
            transfer_protocol: s.transfer_protocol,
            send_bar_enabled: s.send_bar_enabled,
        })
        .collect())
}

// ── 会话配置命令 ─────────────────────────────────────

#[tauri::command]
pub async fn save_session_config(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SaveSessionConfigRequest,
) -> Result<String, String> {
    let SaveSessionConfigRequest {
        endpoint,
        mut params,
        name,
        plugin_id,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
    } = request;
    let pid = plugin_id;
    let id = if let Some(ref raw) = session_id {
        if uuid::Uuid::parse_str(raw).is_err() {
            return Err(format!("无效的 session_id 格式: {}", raw));
        }
        raw.clone()
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let handler = state
        .plugins
        .contribution_by_str::<SessionConfigHandler>(&pid);
    if let Some(validate) = handler.as_deref().and_then(|handler| handler.validate) {
        validate(&params)?;
    }
    let services = SessionConfigServices {
        credential_store: &state.credential_store,
        session_store: &state.session_store,
    };
    let prepared = if let Some(handler) = handler.as_deref() {
        (handler.prepare)(&services, &id, &mut params)?
    } else {
        crate::plugin_application::unchanged_session_config(&services, &id, &mut params)?
    };

    let session_name = if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
        name
    } else if let Some(default_name) = handler.as_deref().and_then(|handler| handler.default_name) {
        default_name(&params, &endpoint)?
    } else {
        format!("{} @ {}", pid, endpoint)
    };
    let saved = crate::kernel::session_store::SavedSession {
        id: id.clone(),
        name: session_name,
        plugin_id: pid,
        endpoint,
        params,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        transfer_enabled: transfer_enabled.unwrap_or(true),
        transfer_protocol,
        send_bar_enabled: send_bar_enabled.unwrap_or(true),
    };
    SessionStore::save_config_to_disk_transactional(&app, saved, || {
        prepared.commit(&state.credential_store)
    })?;
    Ok(id)
}

/// 删除会话配置（从 sessions.json 中移除指定会话）
#[tauri::command]
pub async fn delete_session_config(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    SessionStore::delete_config_from_disk_transactional(&app, &session_id, |deleted_session| {
        if let Some(saved) = deleted_session {
            if let Some(handler) = state
                .plugins
                .contribution_by_str::<SessionConfigHandler>(&saved.plugin_id)
            {
                if let Some(delete) = handler.delete {
                    delete(&state.credential_store, &session_id)?;
                }
            }
        }
        Ok(())
    })
}

// ── 凭据存储状态 ────────────────────────────────────

#[tauri::command]
pub async fn credential_storage_status(
    state: State<'_, AppState>,
) -> Result<crate::security::credential_store::CredentialStorageStatus, String> {
    Ok(state.credential_store.status())
}

#[tauri::command]
pub async fn unlock_credential_vault(
    state: State<'_, AppState>,
    master_password: String,
) -> Result<(), String> {
    let master_password = zeroize::Zeroizing::new(master_password);
    state
        .credential_store
        .unlock_fallback(&master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lock_credential_vault(state: State<'_, AppState>) -> Result<(), String> {
    state.credential_store.lock_fallback();
    Ok(())
}

// ── 日志引擎命令 ────────────────────────────────────

/// 启动会话数据日志记录
///
/// 锁顺序：session_store → log_engine（与 write_data 保持一致，避免死锁）
#[tauri::command]
pub async fn start_session_log(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    // 先锁定 session_store 读取会话信息（锁在块结束时释放）
    let (session_name, port_name, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        // 解析子连接 ID → 父会话（子连接不在 HashMap 中）
        let resolved_id = store
            .resolve_parent_id(&session_id)
            .unwrap_or(session_id.clone());
        let handle = store
            .get_session(&resolved_id)
            .ok_or_else(|| store.session_not_found(&resolved_id))?;
        (
            handle.name.clone(),
            handle.endpoint.clone(),
            handle
                .params
                .get("data_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("text")
                .to_string(),
        )
    };

    // 只在短临界区读取配置并克隆 sender；ACK 等待不得持有 LogEngine 锁。
    let log_sender = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        if !log_engine.get_config()?.session_enabled {
            return Err("Session Data Log is disabled in Settings".to_string());
        }
        log_engine.sender()
    };

    let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
    let cmd = LogEntry::Command(crate::kernel::log_engine::LogCommand::StartSession {
        session_id: session_id.clone(),
        session_name,
        port_name,
        data_mode,
        response: response_tx,
    });

    log_sender
        .try_send(cmd)
        .map_err(|e| format!("日志控制队列繁忙，启动请求未入队: {}", e))?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        response_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|error| format!("等待日志启动确认失败: {}", error))
    })
    .await
    .map_err(|error| format!("日志启动确认任务失败: {}", error))??;

    match result {
        Ok(_status) => Ok(session_id),
        Err(error) => Err(error),
    }
}

/// 停止会话数据日志记录
#[tauri::command]
pub fn stop_session_log(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;

    let cmd = LogEntry::Command(crate::kernel::log_engine::LogCommand::StopSession { session_id });

    log_engine
        .sender()
        .try_send(cmd)
        .map_err(|e| format!("日志控制队列繁忙，停止请求未入队: {}", e))?;

    Ok(())
}

/// 前端用户操作/事件日志
#[tauri::command]
pub fn log_event(state: State<'_, AppState>, level: String, message: String) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;

    try_send_system_event(&log_engine.sender(), level, message, Local::now());

    Ok(())
}

/// 获取当前活跃日志状态
#[tauri::command]
pub fn get_log_status(state: State<'_, AppState>) -> Result<Vec<LogStatus>, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_active_logs())
}

#[tauri::command]
pub fn get_log_health(state: State<'_, AppState>) -> Result<LogHealth, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_health())
}

/// 更新系统日志配置（启用/禁用 + 最低日志级别）
#[tauri::command]
pub async fn set_system_log_config(
    state: State<'_, AppState>,
    enabled: bool,
    level: String,
) -> Result<(), String> {
    let _log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let (previous_enabled, previous_level) =
        crate::kernel::log_engine::system_log_config_checked()?;
    state
        .config_store
        .set_batch(&[
            ("logging.system_enabled", serde_json::json!(enabled)),
            ("logging.system_level", serde_json::json!(level.clone())),
        ])
        .map_err(|e| e.to_string())?;

    if let Err(apply_error) = crate::kernel::log_engine::set_system_log_config(enabled, &level) {
        let rollback = state.config_store.set_batch(&[
            (
                "logging.system_enabled",
                serde_json::json!(previous_enabled),
            ),
            ("logging.system_level", serde_json::json!(previous_level)),
        ]);
        return match rollback {
            Ok(()) => Err(apply_error),
            Err(rollback_error) => Err(format!(
                "{}; ConfigStore rollback failed: {}",
                apply_error, rollback_error
            )),
        };
    }
    Ok(())
}

/// 获取日志目录路径
#[tauri::command]
pub fn get_log_dir(state: State<'_, AppState>) -> Result<String, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let config = log_engine.get_config()?;
    Ok(config.log_dir.to_string_lossy().to_string())
}

/// 获取完整日志配置（供前端设置页面初始加载）
///
/// 返回前端友好的 `LogConfigResponse`（PathBuf 已转为字符串）。
/// 前端调用此命令获取 Rust 端的当前配置，确保 UI 显示与后端一致。
#[tauri::command]
pub fn get_log_config(state: State<'_, AppState>) -> Result<LogConfigResponse, String> {
    if !state.config_store.persistence_ready() {
        return Err("ConfigStore persistence is unavailable".to_string());
    }
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    log_engine.get_config_response()
}

/// 在系统文件管理器中打开日志目录
#[tauri::command]
pub async fn open_log_dir(state: State<'_, AppState>) -> Result<(), String> {
    let path = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.get_config()?.log_dir
    };
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("创建日志目录失败 {:?}: {}", path, error))?;

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("打开目录失败: {}", e))?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("打开目录失败: {}", e))?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("打开目录失败: {}", e))?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("打开日志目录任务失败: {error}"))?
}

/// 更新日志引擎运行时配置（由前端设置页调用）
///
/// 消费者线程下次循环自动读取新配置，无需重启。
#[tauri::command]
pub async fn update_log_config(
    state: State<'_, AppState>,
    config: LogConfigUpdate,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let current = log_engine.get_config()?;
    let next_session_enabled = config.session_enabled.unwrap_or(current.session_enabled);
    let next_file_max_size = config.file_max_size.unwrap_or(current.file_max_size);
    let next_buffer_size = config.buffer_size.unwrap_or(current.buffer_size);
    let next_flush_interval_ms = config
        .flush_interval_ms
        .unwrap_or(current.flush_interval_ms);
    let next_retention_days = config.retention_days.unwrap_or(current.retention_days);

    state
        .config_store
        .set_batch(&[
            (
                "logging.session_enabled",
                serde_json::json!(next_session_enabled),
            ),
            (
                "logging.file_max_size",
                serde_json::json!(next_file_max_size),
            ),
            ("logging.buffer_size", serde_json::json!(next_buffer_size)),
            (
                "logging.flush_interval_ms",
                serde_json::json!(next_flush_interval_ms),
            ),
            (
                "logging.retention_days",
                serde_json::json!(next_retention_days),
            ),
        ])
        .map_err(|e| e.to_string())?;

    if let Err(apply_error) = log_engine.update_config(config) {
        let rollback = state.config_store.set_batch(&[
            (
                "logging.session_enabled",
                serde_json::json!(current.session_enabled),
            ),
            (
                "logging.file_max_size",
                serde_json::json!(current.file_max_size),
            ),
            (
                "logging.buffer_size",
                serde_json::json!(current.buffer_size),
            ),
            (
                "logging.flush_interval_ms",
                serde_json::json!(current.flush_interval_ms),
            ),
            (
                "logging.retention_days",
                serde_json::json!(current.retention_days),
            ),
        ]);
        return match rollback {
            Ok(()) => Err(apply_error),
            Err(rollback_error) => Err(format!(
                "{}; ConfigStore rollback failed: {}",
                apply_error, rollback_error
            )),
        };
    }
    Ok(())
}

/// 清除所有日志文件
#[tauri::command]
pub async fn clear_all_logs(state: State<'_, AppState>) -> Result<(), String> {
    let log_sender = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };
    let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
    log_sender
        .try_send(LogEntry::Command(
            crate::kernel::log_engine::LogCommand::ClearAll {
                response: response_tx,
            },
        ))
        .map_err(|e| format!("日志控制队列繁忙，清理请求未入队: {}", e))?;

    tauri::async_runtime::spawn_blocking(move || {
        response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| format!("等待日志清理确认失败: {}", error))?
    })
    .await
    .map_err(|error| format!("日志清理确认任务失败: {}", error))?
}

// ── 脚本引擎命令 ────────────────────────────────────

/// 启动会话的脚本引擎
///
/// 首次调用创建 Lua VM 线程，后续调用热加载新脚本代码。
#[tauri::command]
pub fn start_script_engine(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    code: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.start_script(&session_id, &code, app)
}

/// 停止会话的脚本引擎
#[tauri::command]
pub fn stop_script_engine(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.stop_script(&session_id)
}

/// 将自动应答规则列表编译为 Lua 脚本代码
#[tauri::command]
pub fn rules_to_script(
    rules: Vec<crate::kernel::script_engine::codegen::AutoReplyRule>,
    name: String,
    match_strategy: String,
) -> String {
    crate::kernel::script_engine::codegen::rules_to_lua_script(&rules, &name, &match_strategy)
}

/// 测试匹配表达式
///
/// 对应前端 MatchTester 组件，支持全部 5 种匹配模式。
/// `match_format` 为 "hex" 时，pattern 被视为十六进制字符串进行字节级匹配。
#[tauri::command]
pub fn test_match(
    pattern: String,
    mode: String,
    test_data: String,
    case_sensitive: bool,
    match_format: Option<String>,
) -> Result<serde_json::Value, String> {
    let is_hex = match_format.as_deref() == Some("hex");
    // 解释测试数据中的转义序列（\r \n \t \0 \\），保持与脚本引擎行为一致
    let test_data = if is_hex {
        hex_to_bytes(&test_data)?
    } else {
        interpret_escape_sequences(&test_data).into_bytes()
    };
    let test_data_str = if is_hex {
        None
    } else {
        Some(String::from_utf8_lossy(&test_data).to_string())
    };
    match mode.as_str() {
        "regex" => test_match_regex(&pattern, &test_data, test_data_str.as_deref(), is_hex),
        "lua_pattern" => test_match_lua_pattern(&pattern, test_data_str.as_deref()),
        _ => test_match_text(&pattern, mode.as_str(), &test_data, case_sensitive, is_hex),
    }
}

/// 正则匹配测试
fn test_match_regex(
    pattern: &str,
    test_data: &[u8],
    test_data_str: Option<&str>,
    is_hex: bool,
) -> Result<serde_json::Value, String> {
    let regex_data = if is_hex {
        String::from_utf8_lossy(test_data).to_string()
    } else {
        test_data_str.unwrap_or("").to_string()
    };
    let re = regex::Regex::new(pattern).map_err(|e| format!("正则语法错误: {}", e))?;
    let matched = if regex_data.is_empty() {
        None
    } else {
        Some(re.is_match(&regex_data))
    };
    let groups: Vec<String> = if !regex_data.is_empty() {
        re.captures(&regex_data)
            .map(|caps| {
                caps.iter()
                    .map(|c| c.map(|m| m.as_str().to_string()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };
    Ok(serde_json::json!({
        "valid": true,
        "matched": matched,
        "groups": groups,
    }))
}

/// 文本/HEX 匹配测试（contains / equals / starts_with）
fn test_match_text(
    pattern: &str,
    mode: &str,
    test_data: &[u8],
    case_sensitive: bool,
    is_hex: bool,
) -> Result<serde_json::Value, String> {
    let pat_bytes = if is_hex {
        hex_to_bytes(pattern)?
    } else {
        interpret_escape_sequences(pattern).into_bytes()
    };
    if is_hex {
        let matched = if test_data.is_empty() {
            None
        } else {
            Some(match mode {
                "contains" => test_data
                    .windows(pat_bytes.len())
                    .any(|w| w == pat_bytes.as_slice()),
                "equals" => *test_data == pat_bytes,
                "starts_with" => test_data.starts_with(&pat_bytes),
                _ => return Err(format!("未知匹配模式: {}", mode)),
            })
        };
        Ok(serde_json::json!({
            "valid": true,
            "matched": matched,
            "groups": [],
        }))
    } else {
        let pat_str = String::from_utf8_lossy(&pat_bytes).to_string();
        let data_str = String::from_utf8_lossy(test_data).to_string();
        let (data, pat) = if case_sensitive {
            (data_str.clone(), pat_str.clone())
        } else {
            (data_str.to_lowercase(), pat_str.to_lowercase())
        };
        let matched = if data_str.is_empty() {
            None
        } else {
            Some(match mode {
                "contains" => data.contains(&pat),
                "equals" => data == pat,
                "starts_with" => data.starts_with(&pat),
                _ => return Err(format!("未知匹配模式: {}", mode)),
            })
        };
        Ok(serde_json::json!({
            "valid": true,
            "matched": matched,
            "groups": [],
        }))
    }
}

/// Lua pattern 匹配测试
///
/// 使用沙箱化 Lua VM + 安全传值（create_string / globals.set），
/// 避免字符串插值注入的代码执行风险。VM 已移除 os/io/require 等危险模块。
fn test_match_lua_pattern(
    pattern: &str,
    test_data_str: Option<&str>,
) -> Result<serde_json::Value, String> {
    let data_str = test_data_str.unwrap_or("");
    if data_str.is_empty() {
        return Ok(serde_json::json!({
            "valid": true,
            "matched": null,
            "groups": [],
        }));
    }
    let lua = create_sandboxed_lua().map_err(|e| format!("创建测试 VM 失败: {}", e))?;
    lua.globals()
        .set(
            "__test_data",
            lua.create_string(data_str.as_bytes())
                .map_err(|e| format!("Lua 传值失败: {}", e))?,
        )
        .map_err(|e| format!("Lua 传值失败: {}", e))?;
    lua.globals()
        .set(
            "__test_pattern",
            lua.create_string(pattern.as_bytes())
                .map_err(|e| format!("Lua 传值失败: {}", e))?,
        )
        .map_err(|e| format!("Lua 传值失败: {}", e))?;
    let matched: bool = lua
        .load(r#"return string.find(__test_data, __test_pattern) ~= nil"#)
        .eval()
        .unwrap_or(false);
    Ok(serde_json::json!({
        "valid": true,
        "matched": Some(matched),
        "groups": [],
    }))
}

// ── 统一文件传输命令（协议无关）────────────────────────────

/// 统一文件传输发送命令（协议无关）
///
/// 前端统一入口。通过 TransferOrchestrator 策略模式分发到
/// Inline（串口 X/Y/ZModem）或辅助传输（SSH SFTP）策略。
#[tauri::command]
pub async fn file_transfer_send(
    app: AppHandle,
    state: State<'_, AppState>,
    request: FileTransferSendRequest,
) -> Result<crate::transfer::orchestrator::TransferStartAck, String> {
    let FileTransferSendRequest {
        session_id,
        protocol_options,
        file_paths,
        remote_dir,
        overwrite_policy,
    } = request;
    protocol_options.validate()?;
    let protocol = protocol_options.protocol().to_string();
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）。
    let internal_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .resolve_parent_id(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };

    let pt: TransferProtocolType = protocol
        .parse()
        .map_err(|_| format!("无效的传输协议: {}", protocol))?;

    // 构建 FileInfo 列表。任一输入无效就拒绝整个批次，避免用户选择的目录/
    // 文件被静默丢弃后仍启动“部分上传”。
    let files: Vec<crate::transfer::types::FileInfo> = file_paths
        .iter()
        .map(|path| {
            crate::transfer::types::FileInfo::from_path(path)
                .map_err(|e| format!("无法读取传输源 '{}': {}", path, e))
        })
        .collect::<Result<_, _>>()?;
    if files.is_empty() {
        return Err("没有可传输的有效文件".into());
    }

    // 创建进度通道
    let (progress_tx, progress_rx) = mpsc::unbounded_channel();

    log::info!(
        "文件传输发送: protocol={}, client={}→internal={}, files={}",
        pt,
        session_id,
        internal_id,
        files.len()
    );

    let overwrite_policy =
        crate::kernel::file_transfer::OverwritePolicy::parse(overwrite_policy.as_deref())?;
    let orch = crate::transfer::orchestrator::create_orchestrator(&pt)?;
    orch.execute_send(
        app,
        crate::transfer::orchestrator::SendContext {
            session_id: internal_id,
            files,
            remote_dir,
            options: crate::kernel::file_transfer::FileTransferOptions {
                overwrite_policy,
                destination_paths: Vec::new(),
            },
            progress_tx,
            progress_rx,
            protocol_options,
        },
        session_id, // client_session_id — 前端原始 ID，用于事件回传
    )
    .await
}

/// 统一文件传输接收命令（协议无关）
///
/// 通过 TransferOrchestrator 策略模式分发到
/// Inline（串口 X/Y/ZModem）或辅助传输（SSH SFTP）策略。
#[tauri::command]
pub async fn file_transfer_receive(
    app: AppHandle,
    state: State<'_, AppState>,
    request: FileTransferReceiveRequest,
) -> Result<crate::transfer::orchestrator::TransferStartAck, String> {
    let FileTransferReceiveRequest {
        session_id,
        protocol_options,
        download_dir,
        remote_paths,
        destination_paths,
        overwrite_policy,
    } = request;
    let protocol = protocol_options.protocol().to_string();
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）。
    let internal_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .resolve_parent_id(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };

    let pt: TransferProtocolType = protocol
        .parse()
        .map_err(|_| format!("无效的传输协议: {}", protocol))?;

    let (progress_tx, progress_rx) = mpsc::unbounded_channel();

    log::info!(
        "文件传输接收: protocol={}, client={}→internal={}, download_dir={}, remote_paths=[{}]({} files)",
        pt, session_id, internal_id, download_dir,
        remote_paths.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
        remote_paths.len()
    );

    let overwrite_policy =
        crate::kernel::file_transfer::OverwritePolicy::parse(overwrite_policy.as_deref())?;
    let orch = crate::transfer::orchestrator::create_orchestrator(&pt)?;
    orch.execute_receive(
        app,
        crate::transfer::orchestrator::ReceiveContext {
            session_id: internal_id,
            download_dir,
            remote_paths,
            options: crate::kernel::file_transfer::FileTransferOptions {
                overwrite_policy,
                destination_paths: destination_paths.unwrap_or_default(),
            },
            progress_tx,
            progress_rx,
            protocol_options,
        },
        session_id, // client_session_id — 前端原始 ID，用于事件回传
    )
    .await
}

/// 统一文件传输取消命令（协议无关）
#[tauri::command]
pub fn file_transfer_cancel(
    state: State<'_, AppState>,
    session_id: String,
    transfer_id: Option<String>,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）
    let resolved_id = store
        .resolve_parent_id(&session_id)
        .ok_or_else(|| store.session_not_found(&session_id))?;

    log::info!("请求取消传输: session={}", resolved_id);
    store
        .cancel_scheduled_transfer(&resolved_id, transfer_id.as_deref())
        .map(|_| {
            log::info!("传输取消已接受: session={}", resolved_id);
        })
}

#[cfg(test)]
mod command_security_tests {}
