//! Application-layer plugin contributions.
//!
//! Kernel contracts deliberately stay unaware of credentials, UI commands, and other host services.
//! This module defines the small host-application extension points that need those services while
//! still being registered through the canonical `PluginRuntime`.

use chrono::Local;
use serde::Deserialize;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry, LogEntry};
use crate::kernel::session_store::{
    SavedSession, SessionCreateOptions, SessionState, SessionStore,
};
use crate::security::credential_store::CredentialStore;
use crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};
use crate::transport::DataPlaneRuntime;
use crate::AppState;

pub(crate) struct SessionConfigServices<'a> {
    pub credential_store: &'a CredentialStore,
    pub session_store: &'a Mutex<SessionStore>,
}

type SessionConfigCommit = Box<dyn FnOnce(&CredentialStore) -> Result<(), String> + Send>;

pub(crate) struct PreparedSessionConfig {
    commit: Option<SessionConfigCommit>,
}

impl PreparedSessionConfig {
    pub(crate) fn unchanged() -> Self {
        Self { commit: None }
    }

    pub(crate) fn with_commit<F>(commit: F) -> Self
    where
        F: FnOnce(&CredentialStore) -> Result<(), String> + Send + 'static,
    {
        Self {
            commit: Some(Box::new(commit)),
        }
    }

    pub(crate) fn commit(self, credential_store: &CredentialStore) -> Result<(), String> {
        match self.commit {
            Some(commit) => commit(credential_store),
            None => Ok(()),
        }
    }
}

pub(crate) type ValidateSessionConfig = fn(&Value) -> Result<(), String>;
pub(crate) type PrepareSessionConfig = fn(
    services: &SessionConfigServices<'_>,
    session_id: &str,
    params: &mut Value,
) -> Result<PreparedSessionConfig, String>;
pub(crate) type DefaultSessionName = fn(&Value, &str) -> Result<String, String>;
pub(crate) type SanitizeSavedSession = fn(&mut SavedSession) -> Result<bool, String>;
pub(crate) type DeleteSessionConfig = fn(&CredentialStore, &str) -> Result<(), String>;

#[derive(Clone, Copy)]
pub(crate) struct SessionConfigHandler {
    pub validate: Option<ValidateSessionConfig>,
    pub prepare: PrepareSessionConfig,
    pub default_name: Option<DefaultSessionName>,
    pub sanitize_saved: Option<SanitizeSavedSession>,
    pub delete: Option<DeleteSessionConfig>,
}

pub(crate) fn unchanged_session_config(
    _services: &SessionConfigServices<'_>,
    _session_id: &str,
    _params: &mut Value,
) -> Result<PreparedSessionConfig, String> {
    Ok(PreparedSessionConfig::unchanged())
}

// ── Session connection application contract ─────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectSessionRequest {
    pub endpoint: String,
    pub params: Value,
    pub name: Option<String>,
    pub plugin_id: String,
    pub transfer_enabled: Option<bool>,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: Option<bool>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub initial_elevated: bool,
}

pub(crate) type SessionConnectFuture =
    Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'static>>;
pub(crate) type SessionConnectHandler =
    fn(AppHandle, ConnectSessionRequest) -> SessionConnectFuture;

pub(crate) type SessionDisconnectedHook = fn(&AppHandle, &str);

/// 创建共享 on_data 回调（DataBatcher + 日志记录）。
///
/// DataBatcher 的所有权被移入回调闭包（通过 `batcher.push()` 消费数据），
/// 因此只返回 `Box<dyn Fn>`；`DataBatcher::Drop` 在会话断开时自动 flush + 清理。
/// 虚拟串口不经过 UI 回调旁路，而是直接订阅 DataPlane。
pub(crate) fn create_on_data_callback(
    app: &AppHandle,
    log_tx: std::sync::mpsc::SyncSender<LogEntry>,
    data_mode: String,
    encoding: String,
) -> Box<dyn Fn(String, Vec<u8>) + Send> {
    let app_clone = app.clone();
    let overflow_app = app.clone();
    let batcher = crate::kernel::data_batcher::DataBatcher::new(move |batched| {
        let _ = app_clone.emit(
            "session-data",
            serde_json::json!({
                "session_id": batched.session_id,
                "data_b64": batched.data_b64,
            }),
        );
    });

    Box::new(move |session_id, data| {
        let data_for_log = data.clone();
        if let Some(total_dropped) = batcher.push(session_id.clone(), data) {
            if total_dropped == 1 || total_dropped.is_power_of_two() {
                let _ = overflow_app.emit(
                    "session-display-overflow",
                    serde_json::json!({
                        "session_id": session_id,
                        "dropped_chunks": total_dropped,
                    }),
                );
            }
        }
        try_send_session_log(
            &log_tx,
            DataLogEntry {
                session_id,
                direction: DataDirection::RX,
                data_mode: data_mode.clone(),
                encoding: encoding.clone(),
                payload: data_for_log,
                timestamp: Local::now(),
            },
        );
    })
}

/// 简单根终端会话的共享连接流程。
///
/// Serial 的虚拟端口、SSH 与 Local Shell 的多终端容器需要专属 orchestration；
/// 当前由 Telnet 复用这里的日志、SessionStore 和事件语义。
pub(crate) fn connect_simple_terminal_session(
    app: AppHandle,
    state: &State<'_, AppState>,
    request: ConnectSessionRequest,
    plugin_id: &'static str,
    display_name: &'static str,
    default_send_bar_enabled: bool,
    conn: crate::kernel::plugin_adapter::ProtocolConnection,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    let session_name = name.unwrap_or_else(|| format!("{display_name} {endpoint}"));
    let log_tx = state.log_engine.lock().map_err(|e| e.to_string())?.sender();
    let data_mode = params
        .get("data_mode")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .to_string();
    let encoding = params
        .get("encoding")
        .and_then(Value::as_str)
        .unwrap_or("utf-8")
        .to_string();
    let on_data = create_on_data_callback(&app, log_tx, data_mode, encoding);

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> =
        Box::new(move |session_id, info| {
            let app_state: State<'_, AppState> = app_disconnect.state();
            if let Ok(mut store) = app_state.session_store.lock() {
                store.mark_disconnected(&session_id);
            }
            let _ = app_disconnect.emit(
                "session-disconnected",
                serde_json::json!({
                    "session_id": session_id,
                    "reason": info.reason,
                    "disconnect_info": info,
                }),
            );
        });

    let send_bar_enabled = send_bar_enabled.unwrap_or(default_send_bar_enabled);
    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_session(
            SessionCreateOptions {
                name: session_name.clone(),
                plugin_id: plugin_id.into(),
                endpoint: endpoint.clone(),
                params,
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled,
                id_override: session_id,
            },
            conn,
            on_data,
            on_disconnect,
            app.clone(),
        )?
    };

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_session(&session_id)
            .map(|handle| {
                (
                    handle.name.clone(),
                    handle.params.clone(),
                    handle.connected_at,
                )
            })
            .unwrap_or((session_name, Value::Null, None))
    };
    log::info!("{display_name} session connected: {actual_name} @ {endpoint}");
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": session_id,
            "endpoint": endpoint,
            "connection_type": plugin_id,
            "plugin_id": plugin_id,
            "name": actual_name,
            "params": actual_params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "send_bar_enabled": send_bar_enabled,
        }),
    );
    state
        .session_store
        .lock()
        .map_err(|e| e.to_string())?
        .activate_data_plane(&session_id)?;
    Ok(session_id)
}

/// 在父配置上注册一个协议无关的终端子会话。
///
/// 供 [`connect_session_ssh`]（channel-0）和 [`open_channel`]（channel-1+）共用。
/// 所有配置均从父 [`ActiveSessionHandle`] 统一读取，确保所有通道行为完全一致。
/// 通道名称按 `channel_index + 1` 自动生成为 `"Channel N"`。
pub(crate) async fn create_terminal_sub_channel(
    app: &tauri::AppHandle,
    app_state: &AppState,
    parent_id: &str,
    runtime: DataPlaneRuntime,
    elevated: bool,
    announce_connected: bool,
) -> Result<String, String> {
    let (endpoint, plugin_id, params, data_mode, encoding, send_bar_enabled_val) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        let active_children = handle
            .sub_connections
            .iter()
            .filter(|child| child.state != SessionState::Disconnected)
            .count();
        if active_children >= 32 {
            return Err("每个父会话最多允许 32 个活动终端".to_string());
        }
        (
            handle.endpoint.clone(),
            handle.plugin_id.clone(),
            handle.params.clone(),
            handle
                .params
                .get("data_mode")
                .and_then(Value::as_str)
                .unwrap_or("text")
                .to_string(),
            handle
                .params
                .get("encoding")
                .and_then(Value::as_str)
                .unwrap_or("utf-8")
                .to_string(),
            handle.send_bar_enabled,
        )
    };

    let channel_id = uuid::Uuid::new_v4().to_string();
    let log_tx = app_state
        .log_engine
        .lock()
        .map_err(|e| e.to_string())?
        .sender();
    let on_data = create_on_data_callback(app, log_tx, data_mode, encoding.clone());

    let app_disconnect = app.clone();
    let pid = parent_id.to_string();
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> =
        Box::new(move |channel_id, info| {
            let (parent_disconnected, retain_history) = {
                if let Ok(mut store) = app_disconnect.state::<AppState>().session_store.lock() {
                    store.mark_sub_disconnected(&pid, &channel_id, info.retain_terminal);
                    let (no_live_children, has_other_history) = store
                        .get_session(&pid)
                        .map(|handle| {
                            let no_live = handle.channel_factory.is_some()
                                && handle
                                    .sub_connections
                                    .iter()
                                    .all(|child| child.state == SessionState::Disconnected);
                            let history = handle.sub_connections.iter().any(|child| {
                                child.id != channel_id
                                    && child.state == SessionState::Disconnected
                                    && child.retain_terminal
                            });
                            (no_live, history)
                        })
                        .unwrap_or((false, false));
                    let retain = info.retain_terminal || has_other_history;
                    if no_live_children {
                        let _ = store.close_session(&pid);
                        if !retain {
                            store.reset_child_counter(&pid);
                        }
                    }
                    (no_live_children, retain)
                } else {
                    (false, info.retain_terminal)
                }
            };
            let _ = app_disconnect.emit(
                "channel-closed",
                serde_json::json!({
                    "channel_id": channel_id,
                    "parent_id": pid,
                    "disconnect_info": &info,
                }),
            );
            if parent_disconnected {
                let parent_info = if retain_history {
                    DisconnectInfo::remote_eof("所有活动终端已关闭")
                } else {
                    info.clone()
                };
                let _ = app_disconnect.emit(
                    "session-disconnected",
                    serde_json::json!({
                        "session_id": pid,
                        "reason": &parent_info.reason,
                        "disconnect_info": &parent_info,
                    }),
                );
            }
        });

    let io = Arc::new(SessionIo::new(Some(runtime.handle.clone()), None, encoding));
    let data_plane =
        SessionDataPlane::attach_paused(runtime, channel_id.clone(), on_data, on_disconnect)
            .map_err(|e| e.to_string())?;
    let stats_cancel_flag = Arc::new(AtomicBool::new(false));
    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    SessionStore::spawn_stats_collector(
        app.clone(),
        channel_id.clone(),
        io.clone(),
        connected_at,
        stats_cancel_flag.clone(),
    );

    let (actual_index, channel_name) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            stats_cancel_flag.store(true, Ordering::SeqCst);
            data_plane.request_shutdown();
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        let active_children = handle
            .sub_connections
            .iter()
            .filter(|child| child.state != SessionState::Disconnected)
            .count();
        if active_children >= 32 {
            stats_cancel_flag.store(true, Ordering::SeqCst);
            data_plane.request_shutdown();
            return Err("每个父会话最多允许 32 个活动终端".to_string());
        }
        let actual_idx = handle.next_child_index;
        handle.next_child_index = handle.next_child_index.saturating_add(1);
        let prefix = handle
            .channel_factory
            .as_ref()
            .map(|factory| factory.child_name_prefix())
            .unwrap_or("Channel");
        let actual_name = format!("{} {}", prefix, actual_idx + 1);
        let mut sub = crate::kernel::session_store::SubConnection::new(
            channel_id.clone(),
            actual_name.clone(),
            data_plane,
            io,
            actual_idx,
            elevated,
        );
        sub.connected_at = connected_at;
        sub.stats_cancel_flag = Some(stats_cancel_flag);
        handle.sub_connections.push(sub);
        (actual_idx, actual_name)
    };

    if announce_connected {
        let _ = app.emit(
            "session-connected",
            serde_json::json!({
                "session_id": channel_id,
                "endpoint": endpoint,
                "connection_type": plugin_id,
                "plugin_id": plugin_id,
                "name": channel_name,
                "params": params,
                "connected_at": connected_at,
                "transfer_enabled": false,
                "send_bar_enabled": send_bar_enabled_val,
                "parent_id": parent_id,
                "channel_index": actual_index,
                "elevated": elevated,
            }),
        );
    }
    if announce_connected {
        app_state
            .session_store
            .lock()
            .map_err(|e| e.to_string())?
            .activate_data_plane(&channel_id)?;
    }
    Ok(channel_id)
}

pub(crate) fn terminal_sub_channel_connected_payload(
    app_state: &AppState,
    parent_id: &str,
    channel_id: &str,
) -> Result<serde_json::Value, String> {
    let (
        endpoint,
        plugin_id,
        params,
        send_bar_enabled,
        channel_name,
        connected_at,
        channel_index,
        elevated,
    ) = {
        let store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let parent = store
            .get_session(parent_id)
            .ok_or_else(|| format!("父会话 {} 已在连接完成前关闭", parent_id))?;
        if parent.state != SessionState::Connected {
            return Err("父会话已在连接完成前断开".to_string());
        }
        let child = parent
            .sub_connections
            .iter()
            .find(|child| child.id == channel_id && child.state == SessionState::Connected)
            .ok_or_else(|| format!("终端子会话 {} 已在连接完成前关闭", channel_id))?;
        (
            parent.endpoint.clone(),
            parent.plugin_id.clone(),
            parent.params.clone(),
            parent.send_bar_enabled,
            child.name.clone(),
            child.connected_at,
            child.channel_index,
            child.elevated,
        )
    };

    Ok(serde_json::json!({
        "session_id": channel_id,
        "endpoint": endpoint,
        "connection_type": plugin_id,
        "plugin_id": plugin_id,
        "name": channel_name,
        "params": params,
        "connected_at": connected_at,
        "transfer_enabled": false,
        "send_bar_enabled": send_bar_enabled,
        "parent_id": parent_id,
        "channel_index": channel_index,
        "elevated": elevated,
    }))
}
