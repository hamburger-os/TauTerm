//! Frontend IPC boundary for transport-sensitive commands.
//!
//! Confirmed transport I/O may wait for the transport actor/physical driver. Tauri synchronous
//! commands execute on the application main thread, so WebView-facing write/resize paths live here
//! as async commands. SessionStore locks are released before awaiting transport acknowledgements.

use crate::kernel::log_engine::{
    session_log_is_active, try_send_session_log, DataDirection, DataLogEntry,
};
use crate::session::{DisconnectInfo, RuntimeId, RuntimeIdentity};
use crate::AppState;
use chrono::Local;
use tauri::{AppHandle, Emitter, State};

/// Write bytes to a session and resolve only after the transport actor confirms the physical write.
///
/// Text payloads are encoded by SessionIo before dispatch. The exact wire bytes are returned so TX
/// rendering and logging remain truthful while the ACK wait stays off the Tauri main thread.
#[tauri::command]
pub async fn write_data(
    state: State<'_, AppState>,
    session_id: String,
    runtime_id: RuntimeId,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    let identity = RuntimeIdentity {
        session_id: session_id.clone(),
        runtime_id,
    };
    let (encoding, data_mode, io) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let io = store.io_for_runtime(&identity)?;
        let resolved_id = store
            .resolve_parent_id(&session_id)
            .unwrap_or_else(|| session_id.clone());
        let handle = store.get_session(&resolved_id);
        let encoding = handle
            .and_then(|h| h.params.get("encoding"))
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8")
            .to_string();
        let data_mode = handle
            .and_then(|h| h.params.get("data_mode"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "text".to_string());
        (encoding, data_mode, io)
    };

    let data_out = if transcode {
        io.send_text_async(&data)
            .await
            .map_err(|error| error.to_string())?
    } else {
        io.send_async(data.clone())
            .await
            .map_err(|error| error.to_string())?;
        data
    };

    // Metadata may come from a parent Session, but log ownership remains the concrete Session/channel
    // id. StartSession and RX logging use the same identity, so TX must not silently switch owners.
    // Skip the payload copy entirely unless this exact Session is actively being recorded.
    let current = state
        .session_store
        .lock()
        .map_err(|e| e.to_string())?
        .runtime_is_connected(&identity);
    if current && session_log_is_active(&session_id) {
        if let Ok(log_engine) = state.log_engine.lock() {
            try_send_session_log(
                &log_engine.sender(),
                DataLogEntry {
                    session_id,
                    direction: DataDirection::TX,
                    stream: None,
                    data_mode,
                    encoding,
                    payload: data_out.clone(),
                    timestamp: Local::now(),
                },
            );
        }
    }
    Ok(data_out)
}

/// Resize a terminal-capable DataPlane without blocking the application main thread.
#[tauri::command]
pub async fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    runtime_id: RuntimeId,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let io = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.io_for_runtime(&RuntimeIdentity {
            session_id,
            runtime_id,
        })?
    };
    io.resize_terminal_async(cols, rows)
        .await
        .map_err(|error| error.to_string())
}

/// Close one terminal child. Blocking thread joins are explicitly offloaded after the SessionStore
/// lock is released so teardown cannot occupy a Tauri async-runtime worker.
#[tauri::command]
pub async fn close_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    runtime_id: crate::session::RuntimeId,
    parent_id: Option<String>,
    reset_counter: Option<bool>,
) -> Result<(), String> {
    let (parent_id, identity, is_last, retain_history, cleanup, parent_cleanup) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store
            .find_parent_of_channel(&session_id)
            .or(parent_id)
            .ok_or_else(|| format!("子连接 {} 未找到", session_id))?;
        let identity = store.runtime_identity(&pid).ok_or("父会话不存在")?;
        if identity.runtime_id != runtime_id {
            return Err("会话运行时已变化，拒绝关闭旧子连接".into());
        }
        let result = store.close_sub_connection(&pid, &session_id);
        let (last, cleanup) = match result {
            Ok(result) => result,
            Err(_) => {
                if reset_counter.unwrap_or(false) {
                    store.reset_child_counter(&pid);
                }
                return Ok(());
            }
        };
        let retain_history = store
            .get_session(&pid)
            .map(|handle| {
                handle.sub_connections.iter().any(|child| {
                    child.state == crate::kernel::session_store::SessionState::Disconnected
                        && child.retain_terminal
                })
            })
            .unwrap_or(false);
        let parent_cleanup = if last {
            let cleanup = store.close_session(&pid)?;
            if reset_counter.unwrap_or(false) {
                store.reset_child_counter(&pid);
            }
            Some(cleanup)
        } else {
            None
        };
        (pid, identity, last, retain_history, cleanup, parent_cleanup)
    };

    tauri::async_runtime::spawn_blocking(move || cleanup.join())
        .await
        .map_err(|error| format!("等待终端子连接资源清理失败: {error}"))?;
    if let Some(cleanup) = parent_cleanup {
        cleanup.finish().await?;
    }

    if is_last {
        let info = if retain_history {
            DisconnectInfo::remote_eof("所有活动终端已关闭")
        } else {
            DisconnectInfo::user_requested()
        };
        let _ = app.emit(
            "session-disconnected",
            serde_json::json!({
                "session_id": parent_id,
                "runtime_id": identity.runtime_id,
                "reason": "所有终端已关闭",
                "disconnect_info": info,
            }),
        );
    }

    log::info!("终端子连接已关闭: {}", session_id);
    Ok(())
}
