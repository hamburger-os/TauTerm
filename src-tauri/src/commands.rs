//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。命令通过会话抽象层操作，
//! 不感知具体连接类型（串口/SSH/Telnet）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use crate::session::{ConnectionType, TermSession, SessionState};
use crate::session::serial::SerialSession;
use crate::AppState;

// ── 数据结构 ────────────────────────────────────────

/// 连接类型信息（返回给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTypeInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
}

/// 会话端点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointItem {
    pub name: String,
    pub description: String,
    pub connection_type: String,
}

/// 会话状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub state: String,
    pub connection_type: String,
    pub endpoint: Option<String>,
}

// ── 命令 ────────────────────────────────────────────

/// 获取所有可用连接类型
#[tauri::command]
pub fn get_connection_types() -> Vec<ConnectionTypeInfo> {
    ConnectionType::all()
        .iter()
        .map(|ct| ConnectionTypeInfo {
            id: serde_json::to_string(ct)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string(),
            label: ct.label().to_string(),
            available: ct.is_available(),
        })
        .collect()
}

/// 枚举指定连接类型的可用端点
#[tauri::command]
pub fn enumerate_endpoints(
    state: State<AppState>,
) -> Result<Vec<EndpointItem>, String> {
    let session = state.session.lock().map_err(|e| e.to_string())?;
    let endpoints = session.enumerate_endpoints()?;
    Ok(endpoints
        .into_iter()
        .map(|ep| EndpointItem {
            name: ep.name,
            description: ep.description,
            connection_type: serde_json::to_string(&ep.connection_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string(),
        })
        .collect())
}

/// 连接到端点
///
/// 参数 params 为 JSON 对象，具体结构取决于连接类型：
/// - 串口：`{ "baud_rate", "data_bits", "parity", "stop_bits", "flow_control" }`
#[tauri::command]
pub fn connect_session(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;

    // 数据回调 → 推送 Tauri 事件
    let app_data = app.clone();
    let on_data: Box<dyn Fn(Vec<u8>) + Send> = Box::new(move |data| {
        let _ = app_data.emit("session-data", data);
    });

    // 断开回调 → 推送 Tauri 事件
    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn() + Send> = Box::new(move || {
        let _ = app_disconnect.emit("session-disconnected", ());
    });

    session.connect(&endpoint, params, on_data, on_disconnect)?;

    let _ = app.emit("session-connected", serde_json::json!({
        "endpoint": endpoint,
        "connection_type": serde_json::to_string(&session.connection_type()).unwrap_or_default(),
    }));

    Ok(())
}

/// 断开当前会话
#[tauri::command]
pub fn disconnect_session(
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;
    session.disconnect()?;
    let _ = app.emit("session-disconnected", ());
    Ok(())
}

/// 获取当前会话状态
#[tauri::command]
pub fn get_session_status(
    state: State<AppState>,
) -> Result<SessionStatus, String> {
    let session = state.session.lock().map_err(|e| e.to_string())?;
    let state_str = match session.state() {
        SessionState::Disconnected => "disconnected",
        SessionState::Connecting => "connecting",
        SessionState::Connected => "connected",
    };
    Ok(SessionStatus {
        state: state_str.to_string(),
        connection_type: serde_json::to_string(&session.connection_type())
            .unwrap_or_default()
            .trim_matches('"')
            .to_string(),
        endpoint: None,
    })
}

/// 向当前会话写入数据
#[tauri::command]
pub fn write_data(
    state: State<AppState>,
    data: Vec<u8>,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;
    session.write(&data)
}

/// YModem 发送文件（通过当前串口会话）
#[tauri::command]
pub fn send_files_ymodem(
    app: AppHandle,
    state: State<AppState>,
    file_paths: Vec<String>,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;

    if session.connection_type() != ConnectionType::Serial {
        return Err("YModem 当前仅支持串口连接".into());
    }

    let port = session.port_handle();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    session.set_cancel_tx(cancel_tx);

    // 释放锁，避免死锁
    drop(session);

    SerialSession::ymodem_send(port, app, file_paths, cancel_rx)
}

/// YModem 接收文件（通过当前串口会话）
#[tauri::command]
pub fn receive_files_ymodem(
    app: AppHandle,
    state: State<AppState>,
    download_dir: String,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;

    if session.connection_type() != ConnectionType::Serial {
        return Err("YModem 当前仅支持串口连接".into());
    }

    let port = session.port_handle();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    session.set_cancel_tx(cancel_tx);

    drop(session);

    SerialSession::ymodem_receive(port, app, download_dir, cancel_rx)
}

/// 取消当前传输
#[tauri::command]
pub fn cancel_transfer(
    state: State<AppState>,
) -> Result<(), String> {
    let mut session = state.session.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = session.take_cancel_tx() {
        let _ = tx.send(());
    }
    Ok(())
}
