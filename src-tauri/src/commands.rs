//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。通过 SessionManager 操作。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use crate::session::{ConnectionType, SessionState};
use crate::session::manager::SessionManager;
use crate::session::serial::SerialSession;
use crate::AppState;

// ── 数据结构 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTypeInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointItem {
    pub name: String,
    pub description: String,
    pub connection_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSessionInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub params: Value,
    pub timestamp: u64,
}

// ── 命令 ────────────────────────────────────────────

#[tauri::command]
pub fn get_connection_types() -> Vec<ConnectionTypeInfo> {
    ConnectionType::all()
        .iter()
        .map(|ct| ConnectionTypeInfo {
            id: serde_json::to_string(ct).unwrap_or_default().trim_matches('"').to_string(),
            label: ct.label().to_string(),
            available: ct.is_available(),
        })
        .collect()
}

#[tauri::command]
pub fn enumerate_endpoints() -> Result<Vec<EndpointItem>, String> {
    let endpoints = SerialSession::enumerate_serial_endpoints()?;
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

/// 创建新会话
///
/// 返回 session_id。前端通过 events 接收连接结果。
#[tauri::command]
pub fn connect_session(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
) -> Result<String, String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;

    let app_data = app.clone();
    let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {
        let _ = app_data.emit("session-data", serde_json::json!({
            "session_id": session_id,
            "data": data,
        }));
    });

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |session_id| {
        // 标记会话为已断开，清理 I/O 线程引用，避免僵尸句柄累积
        let app_state: State<AppState> = app_disconnect.state();
        if let Ok(mut manager) = app_state.manager.lock() {
            manager.mark_disconnected(&session_id);
        }
        let _ = app_disconnect.emit("session-disconnected", serde_json::json!({
            "session_id": session_id,
        }));
    });

    let conn_type = ConnectionType::Serial; // 当前仅串口
    let session_name = name.unwrap_or_default();
    let params_clone = params.clone();
    let session_id = manager.create_session(&session_name, conn_type, &endpoint, params, on_data, on_disconnect, app.clone())?;

    // 自动保存
    let path = SessionManager::sessions_file_path(&app);
    let _ = manager.save_to_disk(&path);

    // 获取会话的实际名称、参数和连接时间戳（回填到前端标签页，用于断开后重连）
    let (actual_name, actual_params, connected_at) = manager.get_session(&session_id)
        .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
        .unwrap_or((session_name, params_clone, None));

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": session_id,
        "endpoint": endpoint,
        "connection_type": "serial",
        "name": actual_name,
        "params": actual_params,
        "connected_at": connected_at,
    }));

    Ok(session_id)
}

/// 断开指定会话
#[tauri::command]
pub fn disconnect_session(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;

    // 先保存：此时会话仍在 HashMap 中，保存后可随应用重启恢复
    let path = SessionManager::sessions_file_path(&app);
    let _ = manager.save_to_disk(&path);

    manager.close_session(&session_id)?;

    let _ = app.emit("session-disconnected", serde_json::json!({
        "session_id": session_id,
    }));
    Ok(())
}

/// 向指定会话写入数据
#[tauri::command]
pub fn write_data(
    state: State<AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let manager = state.manager.lock().map_err(|e| e.to_string())?;
    manager.write(&session_id, &data)
}

/// 切换活跃标签页
#[tauri::command]
pub fn switch_active_session(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    manager.switch_active(&session_id)?;
    let _ = app.emit("session-switched", serde_json::json!({
        "session_id": session_id,
    }));
    Ok(())
}

/// 重命名会话
#[tauri::command]
pub fn rename_session(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    new_name: String,
) -> Result<(), String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    manager.rename_session(&session_id, &new_name)?;

    let path = SessionManager::sessions_file_path(&app);
    let _ = manager.save_to_disk(&path);

    let _ = app.emit("session-renamed", serde_json::json!({
        "session_id": session_id,
        "name": new_name,
    }));
    Ok(())
}

/// 标签页重排序
#[tauri::command]
pub fn reorder_tabs(
    state: State<AppState>,
    session_ids: Vec<String>,
) -> Result<(), String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    manager.reorder_tabs(session_ids)
}

/// 获取所有标签页信息
#[tauri::command]
pub fn get_tabs(
    state: State<AppState>,
) -> Result<Vec<TabInfo>, String> {
    let manager = state.manager.lock().map_err(|e| e.to_string())?;
    let tabs: Vec<TabInfo> = manager.tab_ids().iter().filter_map(|id| {
        manager.get_session(id).map(|h| TabInfo {
            id: id.clone(),
            name: h.name.clone(),
            connection_type: serde_json::to_string(&h.connection_type).unwrap_or_default().trim_matches('"').to_string(),
            endpoint: h.endpoint.clone(),
            state: match h.state {
                SessionState::Connected => "connected".into(),
                SessionState::Connecting => "connecting".into(),
                SessionState::Disconnected => "disconnected".into(),
                SessionState::Transferring => "transferring".into(),
            },
        })
    }).collect();
    Ok(tabs)
}

// ── 会话持久化命令 ─────────────────────────────────

/// 保存会话到磁盘
#[tauri::command]
pub fn save_sessions(
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let manager = state.manager.lock().map_err(|e| e.to_string())?;
    let path = SessionManager::sessions_file_path(&app);
    manager.save_to_disk(&path)
}

/// 从磁盘加载会话配置
#[tauri::command]
pub fn load_sessions(
    app: AppHandle,
) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionManager::sessions_file_path(&app);
    let saved = SessionManager::load_from_disk(&path)?;
    Ok(saved.into_iter().map(|s| SavedSessionInfo {
        id: s.id,
        name: s.name,
        connection_type: s.connection_type,
        endpoint: s.endpoint,
        params: s.params,
        timestamp: s.timestamp,
    }).collect())
}

// ── YModem 文件传输命令 ────────────────────────────

/// YModem 发送文件
///
/// 通过端口句柄转移机制暂停 I/O 线程的读写循环，将串口临时移交给 YModem 传输代码，
/// 传输完成后归还串口，I/O 线程无缝恢复。串口全程不关闭，会话状态保持连接。
#[tauri::command]
pub fn send_files_ymodem(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    file_paths: Vec<String>,
) -> Result<(), String> {
    // 1. 验证会话
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    let handle = manager.get_session_mut(&session_id)
        .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

    if handle.connection_type != ConnectionType::Serial {
        return Err("YModem 当前仅支持串口连接".into());
    }
    if handle.state != SessionState::Connected {
        return Err("会话未连接".into());
    }

    handle.state = SessionState::Transferring;

    // 2. 创建端口交接通道
    let (give_tx, give_rx) = std::sync::mpsc::sync_channel::<Box<dyn serialport::SerialPort>>(1);
    let (return_tx, return_rx) = std::sync::mpsc::sync_channel::<Box<dyn serialport::SerialPort>>(1);
    handle.port_return_tx = Some(return_tx);

    // 3. 创建取消通道
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    handle.cancel_transfer_tx = Some(cancel_tx);

    // 4. 向 I/O 线程发送端口交接命令
    let _ = handle.write_tx.send(crate::session::manager::IoCmd::HandoffPort { give_tx, return_rx });
    drop(manager);

    // 5. 接收端口（I/O 线程阻塞式交出）
    let mut port = give_rx.recv()
        .map_err(|_| "无法从 I/O 线程获取串口端口".to_string())?;

    // 6. 通知前端传输开始（不断开！）
    let _ = app.emit("session-transfer-started", serde_json::json!({
        "session_id": session_id,
    }));

    // 7. 清空缓冲区并执行 YModem 发送
    SerialSession::flush_port_buffer(&mut port);
    let result = SerialSession::ymodem_send(&mut port, app.clone(), file_paths, cancel_rx);

    // 8. 归还端口给 I/O 线程
    {
        let mut mgr = state.manager.lock().map_err(|e| e.to_string())?;
        if let Some(h) = mgr.get_session_mut(&session_id) {
            h.cancel_transfer_tx = None;
            if let Some(tx) = h.port_return_tx.take() {
                let _ = tx.send(port);
            }
            h.state = SessionState::Connected;
        }
    }

    // 9. 通知前端传输完成
    match result {
        Ok(()) => {
            let _ = app.emit("session-transfer-finished", serde_json::json!({
                "session_id": session_id,
            }));
            Ok(())
        }
        Err(e) => {
            // 传输失败（如取消），发出传输失败事件
            let _ = app.emit("session-transfer-failed", serde_json::json!({
                "session_id": session_id,
                "error": e,
            }));
            Err(e)
        }
    }
}

/// YModem 接收文件
///
/// 通过端口句柄转移机制暂停 I/O 线程的读写循环，将串口临时移交给 YModem 传输代码，
/// 传输完成后归还串口，I/O 线程无缝恢复。串口全程不关闭，会话状态保持连接。
#[tauri::command]
pub fn receive_files_ymodem(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    download_dir: String,
) -> Result<(), String> {
    // 1. 验证会话
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    let handle = manager.get_session_mut(&session_id)
        .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

    if handle.connection_type != ConnectionType::Serial {
        return Err("YModem 当前仅支持串口连接".into());
    }
    if handle.state != SessionState::Connected {
        return Err("会话未连接".into());
    }

    handle.state = SessionState::Transferring;

    // 2. 创建端口交接通道
    let (give_tx, give_rx) = std::sync::mpsc::sync_channel::<Box<dyn serialport::SerialPort>>(1);
    let (return_tx, return_rx) = std::sync::mpsc::sync_channel::<Box<dyn serialport::SerialPort>>(1);
    handle.port_return_tx = Some(return_tx);

    // 3. 创建取消通道
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    handle.cancel_transfer_tx = Some(cancel_tx);

    // 4. 向 I/O 线程发送端口交接命令
    let _ = handle.write_tx.send(crate::session::manager::IoCmd::HandoffPort { give_tx, return_rx });
    drop(manager);

    // 5. 接收端口（I/O 线程阻塞式交出）
    let mut port = give_rx.recv()
        .map_err(|_| "无法从 I/O 线程获取串口端口".to_string())?;

    // 6. 通知前端传输开始（不断开！）
    let _ = app.emit("session-transfer-started", serde_json::json!({
        "session_id": session_id,
    }));

    // 7. 清空缓冲区并执行 YModem 接收
    SerialSession::flush_port_buffer(&mut port);
    let result = SerialSession::ymodem_receive(&mut port, app.clone(), download_dir, cancel_rx);

    // 8. 归还端口给 I/O 线程
    {
        let mut mgr = state.manager.lock().map_err(|e| e.to_string())?;
        if let Some(h) = mgr.get_session_mut(&session_id) {
            h.cancel_transfer_tx = None;
            if let Some(tx) = h.port_return_tx.take() {
                let _ = tx.send(port);
            }
            h.state = SessionState::Connected;
        }
    }

    // 9. 通知前端传输完成
    match result {
        Ok(()) => {
            let _ = app.emit("session-transfer-finished", serde_json::json!({
                "session_id": session_id,
            }));
            Ok(())
        }
        Err(e) => {
            let _ = app.emit("session-transfer-failed", serde_json::json!({
                "session_id": session_id,
                "error": e,
            }));
            Err(e)
        }
    }
}

/// 取消当前传输
#[tauri::command]
pub fn cancel_transfer(
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut manager = state.manager.lock().map_err(|e| e.to_string())?;
    manager.cancel_transfer(&session_id)
}
