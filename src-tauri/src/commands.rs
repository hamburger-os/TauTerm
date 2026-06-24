//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。
//! 通过 SerialAdapter + SessionStore + Channel 架构管理会话。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use crate::channel::Channel;
use crate::channel::io_loop::IoLoopCmd;
use crate::kernel::plugin_adapter::{ProtocolAdapter, TransferProtocolType};
use crate::kernel::session_store::{SessionState, SessionStore};
use crate::AppState;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub state: String,
    pub plugin_id: String,
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
}

// ── 命令：连接类型 ──────────────────────────────────

#[tauri::command]
pub fn get_connection_types(
    state: State<AppState>,
) -> Vec<ConnectionTypeInfo> {
    let plugin_host = state.plugin_host.lock().unwrap_or_else(|e| e.into_inner());
    plugin_host.plugins().iter().map(|p| ConnectionTypeInfo {
        id: p.id.clone(),
        label: p.name.clone(),
        available: true,
        description: format!("{} v{}", p.name, p.version),
        icon: p.category.clone(),
        content_type: p.content_type.clone(),
    }).collect()
}

// ── 命令：端点枚举 ──────────────────────────────────

#[tauri::command]
pub fn enumerate_endpoints(
    state: State<AppState>,
    plugin_id: Option<String>,
) -> Result<Vec<EndpointItem>, String> {
    let pid = plugin_id.unwrap_or_else(|| "serial".into());
    match pid.as_str() {
        "serial" => {
            let endpoints = state.serial_adapter.discover_endpoints()
                .map_err(|e| e.to_string())?;
            Ok(endpoints.into_iter().map(|ep| EndpointItem {
                name: ep.name,
                description: ep.description,
                connection_type: "serial".to_string(),
            }).collect())
        }
        other => Err(format!("插件 '{}' 暂不支持端点枚举", other)),
    }
}

// ── 命令：会话连接 ──────────────────────────────────

#[tauri::command]
pub fn connect_session(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    plugin_id: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
) -> Result<String, String> {
    let pid = plugin_id.unwrap_or_else(|| "serial".into());

    // 验证插件存在
    {
        let plugin_host = state.plugin_host.lock().map_err(|e| e.to_string())?;
        if plugin_host.get_plugin(&pid).is_none() {
            return Err(format!("插件 '{}' 未注册", pid));
        }
    }

    match pid.as_str() {
        "serial" => connect_session_serial(app, state, endpoint, params, name, transfer_enabled, transfer_protocol),
        other => Err(format!("插件 '{}' 的连接功能尚未实现", other)),
    }
}

/// 串口会话连接（新架构：SerialAdapter → Channel → SessionStore）
fn connect_session_serial(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
) -> Result<String, String> {
    // 通过 SerialAdapter（ProtocolAdapter trait）创建 Channel
    let channel = state.serial_adapter.connect(&endpoint, &params)
        .map_err(|e| format!("串口连接失败: {}", e))?;

    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state.serial_adapter.content_type();
    let io_strategy = state.serial_adapter.io_strategy();
    let transfer_protocols = state.serial_adapter.transfer_protocols();
    log::info!(
        "串口连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",
        content_type, io_strategy, transfer_protocols
    );

    let params_clone = params.clone();
    let session_name = name.unwrap_or_default();

    let app_data = app.clone();
    let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {
        let _ = app_data.emit("session-data", serde_json::json!({
            "session_id": session_id,
            "data": data,
        }));
    });

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |session_id| {
        let app_state: State<AppState> = app_disconnect.state();
        if let Ok(mut store) = app_state.session_store.lock() {
            store.mark_disconnected(&session_id);
        }
        let _ = app_disconnect.emit("session-disconnected", serde_json::json!({
            "session_id": session_id,
        }));
    });

    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "ymodem".into());

    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    let session_id = store.create_session(
        &session_name, "serial", &endpoint, params, channel,
        on_data, on_disconnect, app.clone(),
        transfer_enabled_val,
        Some(transfer_protocol_val.clone()),
    )?;

    // 自动保存
    let path = SessionStore::sessions_file_path(&app);
    let _ = store.save_to_disk(&path);
    drop(store);

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&session_id)
            .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
            .unwrap_or((session_name, params_clone, None))
    };

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": session_id,
        "endpoint": endpoint,
        "connection_type": "serial",
        "plugin_id": "serial",
        "name": actual_name,
        "params": actual_params,
        "connected_at": connected_at,
        "transfer_enabled": transfer_enabled_val,
        "transfer_protocol": transfer_protocol_val,
    }));

    Ok(session_id)
}

// ── 命令：会话断开 ──────────────────────────────────

#[tauri::command]
pub fn disconnect_session(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    let path = SessionStore::sessions_file_path(&app);
    let _ = store.save_to_disk(&path);
    store.close_session(&session_id)?;

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
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.write(&session_id, &data)
}

/// 切换活跃标签页
#[tauri::command]
pub fn switch_active_session(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.switch_active(&session_id)?;
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
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.rename_session(&session_id, &new_name)?;

    let path = SessionStore::sessions_file_path(&app);
    let _ = store.save_to_disk(&path);

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
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.reorder_tabs(session_ids)?;
    Ok(())
}

/// 获取所有标签页信息
#[tauri::command]
pub fn get_tabs(
    state: State<AppState>,
) -> Result<Vec<TabInfo>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let tabs: Vec<TabInfo> = store.tab_ids().iter().filter_map(|id| {
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
        })
    }).collect();
    Ok(tabs)
}

// ── 会话持久化命令 ─────────────────────────────────

#[tauri::command]
pub fn save_sessions(
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let path = SessionStore::sessions_file_path(&app);
    store.save_to_disk(&path)
}

#[tauri::command]
pub fn load_sessions(
    app: AppHandle,
) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionStore::sessions_file_path(&app);
    let saved = SessionStore::load_from_disk(&path)?;
    Ok(saved.into_iter().map(|s| SavedSessionInfo {
        id: s.id,
        name: s.name,
        connection_type: s.plugin_id.clone(),
        endpoint: s.endpoint,
        params: s.params,
        timestamp: s.timestamp,
        plugin_id: s.plugin_id,
        transfer_enabled: s.transfer_enabled,
        transfer_protocol: s.transfer_protocol.clone(),
    }).collect())
}

// ── 会话配置命令 ─────────────────────────────────────

/// 保存会话配置（不打开端口，仅持久化配置）
#[tauri::command]
pub fn save_session_config(
    app: AppHandle,
    _state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    plugin_id: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
) -> Result<String, String> {
    let pid = plugin_id.unwrap_or_else(|| "serial".into());
    let id = uuid::Uuid::new_v4().to_string();
    let session_name = name.unwrap_or_else(|| format!("{} @ {}", pid, endpoint));

    let now = chrono::Utc::now().timestamp_millis() as u64;

    let saved = crate::kernel::session_store::SavedSession {
        id: id.clone(),
        name: session_name,
        plugin_id: pid,
        endpoint,
        params: params.clone(),
        timestamp: now,
        transfer_enabled: transfer_enabled.unwrap_or(true),
        transfer_protocol: transfer_protocol.clone(),
    };

    SessionStore::save_config_to_disk(&app, saved)?;

    Ok(id)
}

/// 删除会话配置（从 sessions.json 中移除指定会话）
#[tauri::command]
pub fn delete_session_config(
    app: AppHandle,
    _state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    SessionStore::delete_config_from_disk(&app, &session_id)
}

// ── 文件传输命令（统一 X/Y/ZModem）────────────────────

/// X/Y/ZModem 发送文件（带协议选择 + YMODEM 可选配置）
#[tauri::command]
pub fn send_files(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    file_paths: Vec<String>,
    protocol: String,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    let pt: TransferProtocolType = protocol.parse()
        .map_err(|_| format!("不支持的传输协议: {}", protocol))?;
    send_files_internal(app, state, session_id, file_paths, pt, block_size, checksum_mode, streaming)
}

/// 发送文件内部实现
fn send_files_internal(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    file_paths: Vec<String>,
    protocol_type: TransferProtocolType,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    let file_infos: Vec<crate::transfer::types::FileInfo> = file_paths
        .iter()
        .filter_map(|p| {
            match crate::transfer::types::FileInfo::from_path(p) {
                Ok(info) => Some(info),
                Err(e) => {
                    log::warn!("无法获取文件信息 {}: {}", p, e);
                    None
                }
            }
        })
        .collect();

    if file_infos.is_empty() {
        return Err("没有可传输的有效文件".into());
    }

    let pt = protocol_type.clone();
    handoff_and_spawn_transfer(app, state, session_id, &protocol_type, move |port, app_handle, cancel_rx| {
        transfer_send(port, app_handle, file_infos, pt, cancel_rx, block_size, checksum_mode, streaming)
    })
}

/// X/Y/ZModem 接收文件（带协议选择 + YMODEM 可选配置）
#[tauri::command]
pub fn receive_files(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    download_dir: String,
    protocol: String,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    let pt: TransferProtocolType = protocol.parse()
        .map_err(|_| format!("不支持的传输协议: {}", protocol))?;
    receive_files_internal(app, state, session_id, download_dir, pt, block_size, checksum_mode, streaming)
}

/// 接收文件内部实现
fn receive_files_internal(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    download_dir: String,
    protocol_type: TransferProtocolType,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    let pt = protocol_type.clone();
    handoff_and_spawn_transfer(app, state, session_id, &protocol_type, move |port, app_handle, cancel_rx| {
        transfer_receive(port, app_handle, download_dir, pt, cancel_rx, block_size, checksum_mode, streaming)
    })
}

/// Channel 交接 + 后台线程 — send/receive 共享实现
fn handoff_and_spawn_transfer<F>(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    protocol_type: &TransferProtocolType,
    transfer_fn: F,
) -> Result<(), String>
where
    F: FnOnce(&mut Box<dyn serialport::SerialPort>, AppHandle, tokio::sync::oneshot::Receiver<()>) -> Result<(), String> + Send + 'static,
{
    let (give_tx, give_rx) = std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
    let (return_tx, return_rx) = std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store.get_session_mut(&session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

        if handle.state != SessionState::Connected {
            return Err("会话未连接".into());
        }
        handle.state = SessionState::Transferring;
        handle.channel_return_tx = Some(return_tx);
        handle.cancel_transfer_tx = Some(cancel_tx);

        let _ = handle.write_tx.send(IoLoopCmd::HandoffPort { give_tx, return_rx });
    }

    let mut channel = give_rx.recv()
        .map_err(|e| {
            if let Ok(mut store) = state.session_store.lock() {
                if let Some(h) = store.get_session_mut(&session_id) {
                    h.state = SessionState::Connected;
                    h.cancel_transfer_tx = None;
                    h.channel_return_tx = None;
                }
            }
            format!("无法从 I/O 线程获取 Channel: {}", e)
        })?;

    let protocol_str = format!("{:?}", protocol_type).to_lowercase();
    let _ = app.emit("session-transfer-started", serde_json::json!({
        "session_id": session_id,
        "protocol": protocol_str,
    }));

    let port_any = channel.try_handoff()
        .ok_or_else(|| "Channel 不支持端口移交".to_string())?;
    let boxed_port = port_any
        .downcast::<Box<dyn serialport::SerialPort>>()
        .map_err(|_| "端口类型转换失败".to_string())?;
    let mut port = *boxed_port;
    drop(channel);

    let app_clone = app.clone();
    let sid = session_id.clone();
    std::thread::spawn(move || {
        crate::transfer::io::flush_port_buffer(&mut port);
        let result = transfer_fn(&mut port, app_clone.clone(), cancel_rx);

        let app_state: State<AppState> = app_clone.state();
        if let Ok(mut store) = app_state.session_store.lock() {
            if let Some(h) = store.get_session_mut(&sid) {
                h.cancel_transfer_tx = None;
                h.state = SessionState::Connected;
                if let Some(tx) = h.channel_return_tx.take() {
                    use crate::channel::serial_channel::SerialChannel;
                    let new_channel = SerialChannel::new(port);
                    let _ = tx.send(Box::new(new_channel));
                }
            }
        }

        match result {
            Ok(()) => {
                let _ = app_clone.emit("session-transfer-finished", serde_json::json!({
                    "session_id": sid,
                }));
            }
            Err(e) => {
                let _ = app_clone.emit("session-transfer-failed", serde_json::json!({
                    "session_id": sid,
                    "error": e,
                }));
            }
        }
    });

    Ok(())
}

/// 取消当前传输
#[tauri::command]
pub fn cancel_transfer(
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.cancel_transfer(&session_id)
}

// ── 凭据存储命令 ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub account: String,
    pub credential_type: String,
    pub description: String,
}

#[tauri::command]
pub fn store_credential(
    state: State<AppState>,
    account: String,
    credential_type: String,
    value: String,
    description: String,
) -> Result<(), String> {
    use crate::security::credential_store::{CredentialType, CredentialValue};

    let ct = match credential_type.as_str() {
        "password" => CredentialType::Password,
        "ssh_key" => CredentialType::SshKey,
        "certificate" => CredentialType::Certificate,
        "token" => CredentialType::Token,
        other => return Err(format!("未知凭据类型: {}", other)),
    };

    let cv = match ct {
        CredentialType::Password | CredentialType::Token => CredentialValue::Password(value),
        CredentialType::SshKey => CredentialValue::SshKey { private_key: value, passphrase: None },
        CredentialType::Certificate => return Err("证书类型需通过文件导入，暂不支持".into()),
    };

    state.credential_store.store_credential(&account, ct, cv, &description)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_credential(
    state: State<AppState>,
    account: String,
) -> Result<String, String> {
    let cv = state.credential_store.get_credential(&account)
        .map_err(|e| e.to_string())?;

    match cv {
        crate::security::credential_store::CredentialValue::Password(p) |
        crate::security::credential_store::CredentialValue::Token(p) => Ok(p),
        other => Err(format!("不支持的凭据类型: {:?}", std::mem::discriminant(&other))),
    }
}

#[tauri::command]
pub fn list_credentials(
    state: State<AppState>,
) -> Result<Vec<CredentialInfo>, String> {
    let entries = state.credential_store.list_credentials()
        .map_err(|e| e.to_string())?;
    Ok(entries.into_iter().map(|e| CredentialInfo {
        account: e.account,
        credential_type: format!("{:?}", e.credential_type),
        description: e.description,
    }).collect())
}

#[tauri::command]
pub fn delete_credential(
    state: State<AppState>,
    account: String,
) -> Result<(), String> {
    state.credential_store.delete_credential(&account)
        .map_err(|e| e.to_string())
}

// ── ConfigStore 命令 ────────────────────────────────

#[tauri::command]
pub fn get_config(
    state: State<AppState>,
    key: String,
) -> Result<Option<Value>, String> {
    Ok(state.config_store.get::<Value>(&key))
}

#[tauri::command]
pub fn set_config(
    state: State<AppState>,
    key: String,
    value: Value,
) -> Result<(), String> {
    state.config_store.set(&key, &value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_config(
    state: State<AppState>,
    key: String,
) -> Result<(), String> {
    state.config_store.delete(&key)
        .map_err(|e| e.to_string())
}

// ── ThemeEngine 命令 ────────────────────────────────

#[tauri::command]
pub fn get_theme_list(
    state: State<AppState>,
) -> Vec<String> {
    state.theme_engine.theme_names()
}

#[tauri::command]
pub fn get_active_theme(
    state: State<AppState>,
) -> String {
    state.theme_engine.active_name()
}

#[tauri::command]
pub fn set_theme(
    state: State<AppState>,
    name: String,
) -> Result<(), String> {
    state.theme_engine.apply_theme(&name)
        .map_err(|e| e.to_string())
}

// ── 协议无关的传输辅助函数 ──────────────────────────

/// 通过 TransferProtocol trait 发送文件
fn transfer_send(
    port: &mut Box<dyn serialport::SerialPort>,
    app: AppHandle,
    files: Vec<crate::transfer::types::FileInfo>,
    protocol_type: TransferProtocolType,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    block_size: Option<usize>,
    _checksum_mode: Option<String>,  // 保留用于 API 兼容，模式由握手动态检测
    streaming: Option<bool>,
) -> Result<(), String> {
    use crate::transfer::protocol::create_protocol;
    use crate::transfer::types::{FileTransferEvent, TransferProgress};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let protocol_str = format!("{:?}", protocol_type).to_lowercase();
    let protocol: Box<dyn crate::transfer::protocol::TransferProtocol> = match &protocol_type {
        TransferProtocolType::YModem => {
            let mut ymodem = crate::transfer::ymodem::YModem::default();
            if let Some(bs) = block_size {
                ymodem.block_size = bs;
            }
            // checksum_mode 由握手动态检测（'C'=CRC-16, NAK=校验和），
            // 前端参数仅保留用于 API 兼容，不再写入结构体。
            if let Some(s) = streaming {
                ymodem.streaming = s;
            }
            Box::new(ymodem)
        }
        _ => create_protocol(&protocol_type)
            .ok_or_else(|| format!("{} 协议未实现", protocol_str))?,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let c = cancelled.clone();
    std::thread::spawn(move || {
        let _ = cancel_rx.blocking_recv();
        c.store(true, Ordering::SeqCst);
    });
    let cancel_fn = &mut || cancelled.load(Ordering::SeqCst);

    let ac = app.clone();
    let ac2 = app.clone();
    let proto = protocol_str.clone();

    let on_progress: &dyn Fn(TransferProgress) = &move |p: TransferProgress| {
        let _ = ac.emit("transfer-progress", serde_json::json!({
            "file_name": p.file_name,
            "bytes_transferred": p.bytes_transferred,
            "total_bytes": p.total_bytes,
            "file_index": p.file_index,
            "total_files": p.total_files,
            "aggregate_bytes_transferred": p.aggregate_bytes_transferred,
            "aggregate_total_bytes": p.aggregate_total_bytes,
            "direction": "send",
            "protocol": proto,
        }));
    };

    let on_file_event: &dyn Fn(FileTransferEvent) = &move |e: FileTransferEvent| {
        match e {
            FileTransferEvent::FileStart { file_name, file_index, total_files, file_size } => {
                let _ = ac2.emit("transfer-file-start", serde_json::json!({
                    "file_name": file_name,
                    "file_index": file_index,
                    "total_files": total_files,
                    "file_size": file_size,
                }));
            }
            FileTransferEvent::FileComplete { file_name, file_index, total_files, bytes_transferred, success, error } => {
                let _ = ac2.emit("transfer-file-complete", serde_json::json!({
                    "file_name": file_name,
                    "file_index": file_index,
                    "total_files": total_files,
                    "bytes_transferred": bytes_transferred,
                    "success": success,
                    "error": error,
                }));
            }
        }
    };

    let batch_results = protocol
        .send_files(port, &files, on_progress, on_file_event, cancel_fn)
        .map_err(|e| e.to_string())?;

    let completed = batch_results.iter().filter(|r| r.status == "completed").count();
    let failed = batch_results.iter().filter(|r| r.status == "failed").count();
    let skipped = batch_results.iter().filter(|r| r.status == "skipped").count();
    let _ = app.emit("transfer-complete", serde_json::json!({
        "success": failed == 0 && skipped == 0,
        "files_completed": completed,
        "files_failed": failed,
        "files_skipped": skipped,
        "direction": "send",
        "protocol": protocol_str,
        "results": batch_results,
    }));
    Ok(())
}

/// 通过 TransferProtocol trait 接收文件
fn transfer_receive(
    port: &mut Box<dyn serialport::SerialPort>,
    app: AppHandle,
    download_dir: String,
    protocol_type: TransferProtocolType,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    block_size: Option<usize>,
    _checksum_mode: Option<String>,  // 保留用于 API 兼容，模式由握手动态检测
    streaming: Option<bool>,
) -> Result<(), String> {
    use crate::transfer::protocol::create_protocol;
    use crate::transfer::types::{FileTransferEvent, TransferProgress};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let protocol_str = format!("{:?}", protocol_type).to_lowercase();
    let protocol: Box<dyn crate::transfer::protocol::TransferProtocol> = match &protocol_type {
        TransferProtocolType::YModem => {
            let mut ymodem = crate::transfer::ymodem::YModem::default();
            if let Some(bs) = block_size {
                ymodem.block_size = bs;
            }
            // checksum_mode 由握手动态检测（'C'=CRC-16, NAK=校验和），
            // 前端参数仅保留用于 API 兼容，不再写入结构体。
            if let Some(s) = streaming {
                ymodem.streaming = s;
            }
            Box::new(ymodem)
        }
        _ => create_protocol(&protocol_type)
            .ok_or_else(|| format!("{} 协议未实现", protocol_str))?,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let c = cancelled.clone();
    std::thread::spawn(move || {
        let _ = cancel_rx.blocking_recv();
        c.store(true, Ordering::SeqCst);
    });
    let cancel_fn = &mut || cancelled.load(Ordering::SeqCst);

    let ac = app.clone();
    let ac2 = app.clone();
    let proto = protocol_str.clone();

    let on_progress: &dyn Fn(TransferProgress) = &move |p: TransferProgress| {
        let _ = ac.emit("transfer-progress", serde_json::json!({
            "file_name": p.file_name,
            "bytes_transferred": p.bytes_transferred,
            "total_bytes": p.total_bytes,
            "file_index": p.file_index,
            "total_files": p.total_files,
            "aggregate_bytes_transferred": p.aggregate_bytes_transferred,
            "aggregate_total_bytes": p.aggregate_total_bytes,
            "direction": "receive",
            "protocol": proto,
        }));
    };

    let on_file_event: &dyn Fn(FileTransferEvent) = &move |e: FileTransferEvent| {
        match e {
            FileTransferEvent::FileStart { file_name, file_index, total_files, file_size } => {
                let _ = ac2.emit("transfer-file-start", serde_json::json!({
                    "file_name": file_name,
                    "file_index": file_index,
                    "total_files": total_files,
                    "file_size": file_size,
                }));
            }
            FileTransferEvent::FileComplete { file_name, file_index, total_files, bytes_transferred, success, error } => {
                let _ = ac2.emit("transfer-file-complete", serde_json::json!({
                    "file_name": file_name,
                    "file_index": file_index,
                    "total_files": total_files,
                    "bytes_transferred": bytes_transferred,
                    "success": success,
                    "error": error,
                }));
            }
        }
    };

    let batch_results = protocol
        .receive_files(port, &download_dir, on_progress, on_file_event, cancel_fn)
        .map_err(|e| e.to_string())?;

    let completed = batch_results.iter().filter(|r| r.status == "completed").count();
    let failed = batch_results.iter().filter(|r| r.status == "failed").count();
    let skipped = batch_results.iter().filter(|r| r.status == "skipped").count();
    let _ = app.emit("transfer-complete", serde_json::json!({
        "success": failed == 0 && skipped == 0,
        "files_completed": completed,
        "files_failed": failed,
        "files_skipped": skipped,
        "direction": "receive",
        "protocol": protocol_str,
        "results": batch_results,
    }));
    Ok(())
}
