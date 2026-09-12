//! Frontend IPC boundary for transport-sensitive commands.
//!
//! Confirmed transport I/O may wait for the transport actor/physical driver. Tauri synchronous
//! commands execute on the application main thread, so WebView-facing write/resize paths live here
//! as async commands. SessionStore locks are released before awaiting transport acknowledgements.

use crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry};
use crate::session::DisconnectInfo;
use crate::AppState;
use chrono::Local;
use tauri::{AppHandle, Emitter, State};

/// Write bytes to a session and resolve only after the transport actor confirms the physical write.
///
/// The Rust identifier is intentionally distinct from the older synchronous helper in `commands`;
/// Tauri's command rename keeps the stable WebView command name without generating duplicate macro
/// identifiers at crate scope.
///
/// Text payloads are encoded by SessionIo before dispatch. The exact wire bytes are returned so TX
/// rendering and logging remain truthful while the ACK wait stays off the Tauri main thread.
#[tauri::command(rename = "write_data")]
pub async fn write_data_ipc(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    let (encoding, data_mode, io) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
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
        (encoding, data_mode, store.get_io_for(&session_id))
    };

    let io = io.ok_or_else(|| format!("会话 {} 没有可写 I/O 能力", session_id))?;
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

    if let Ok(log_engine) = state.log_engine.lock() {
        try_send_session_log(
            &log_engine.sender(),
            DataLogEntry {
                session_id,
                direction: DataDirection::TX,
                data_mode,
                encoding,
                payload: data_out.clone(),
                timestamp: Local::now(),
            },
        );
    }
    Ok(data_out)
}

/// Resize a terminal-capable DataPlane without blocking the application main thread.
#[tauri::command(rename = "resize_pty")]
pub async fn resize_pty_ipc(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let io = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_io_for(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };
    io.resize_terminal_async(cols, rows)
        .await
        .map_err(|error| error.to_string())
}

/// Close one terminal child. Blocking thread joins are explicitly offloaded after the SessionStore
/// lock is released so teardown cannot occupy a Tauri async-runtime worker.
#[tauri::command(rename = "close_channel")]
pub async fn close_channel_ipc(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    parent_id: Option<String>,
    reset_counter: Option<bool>,
) -> Result<(), String> {
    let (parent_id, is_last, retain_history, cleanup) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store
            .find_parent_of_channel(&session_id)
            .or(parent_id)
            .ok_or_else(|| format!("子连接 {} 未找到", session_id))?;
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
        if last {
            store.close_session(&pid)?;
            if reset_counter.unwrap_or(false) {
                store.reset_child_counter(&pid);
            }
        }
        (pid, last, retain_history, cleanup)
    };

    tauri::async_runtime::spawn_blocking(move || cleanup.join())
        .await
        .map_err(|error| format!("等待终端子连接资源清理失败: {error}"))?;

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
                "reason": "所有终端已关闭",
                "disconnect_info": info,
            }),
        );
    }

    log::info!("终端子连接已关闭: {}", session_id);
    Ok(())
}
