//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。
//! 通过 SerialAdapter + SessionStore + Channel 架构管理会话。

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use crate::channel::Channel;
use crate::channel::io_loop::IoLoopCmd;
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogConfigResponse, LogConfigUpdate, LogEntry, LogStatus};
use crate::kernel::script_engine::codegen::{hex_to_bytes, interpret_escape_sequences};
use crate::kernel::script_engine::sandbox::create_sandboxed_lua;
use crate::kernel::plugin_adapter::{ProtocolAdapter, TransferProtocolType};
use crate::kernel::session_store::{SessionState, SessionStore};
use crate::virtual_port::bridge::VirtualPortBridge;
use crate::virtual_port::manager::{contains_elevation_indicator, PortPair, VirtualPortConfig};
use crate::AppState;

// ── 可调参数常量 ──────────────────────────────────────

/// 桥接数据 channel 容量（物理端口 → 虚拟端口广播）
const BRIDGE_DATA_CHANNEL_CAPACITY: usize = 256;
/// 写回 channel 容量（虚拟端口 → 物理端口写入线程）
const BRIDGE_WRITEBACK_CHANNEL_CAPACITY: usize = 128;

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
    pub send_bar_enabled: bool,
    pub virtual_port_enabled: bool,
    pub virtual_port_count: u32,
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

/// TODO: 升级 Tauri v2 → v3 后，将多个参数收束为请求结构体
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn connect_session(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    plugin_id: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    // 可选：传入已有的 session_id 以原地重连（保留 UUID 和 I/O 统计连续性）
    session_id: Option<String>,
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
        "serial" => connect_session_serial(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, session_id),
        other => Err(format!("插件 '{}' 的连接功能尚未实现", other)),
    }
}

/// TODO: 将串口连接参数收束为 ConnectParams 结构体，消除过多参数
/// BridgeChannel = (tx, rx) 类型别名
type BridgeChannel = (std::sync::mpsc::SyncSender<Vec<u8>>, std::sync::mpsc::Receiver<Vec<u8>>);

/// 串口会话连接（新架构：SerialAdapter → Channel → SessionStore）
#[allow(clippy::too_many_arguments)]
fn connect_session_serial(
    app: AppHandle,
    state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    session_id: Option<String>,
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
    // 提前读取虚拟串口开关，决定是否创建桥接数据通道
    let virtual_enabled = params_clone
        .get("virtual_port_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    log::info!(
        "connect_session_serial: virtual_port_enabled={}, params keys={:?}",
        virtual_enabled,
        params_clone.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    // 获取 data_mode 用于日志格式化
    let data_mode = params_clone
        .get("data_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();
    let data_mode_for_log = data_mode.clone(); // clone for use after the closure

    // 桥接数据通道 (容量 256): 物理端口数据 → 虚拟端口桥接线程
    // 仅在虚拟串口启用时创建，避免不必要的通道分配
    let mut bridge: Option<BridgeChannel> = if virtual_enabled {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(BRIDGE_DATA_CHANNEL_CAPACITY);
        Some((tx, rx))
    } else {
        None
    };
    let bridge_tx = bridge.as_ref().map(|(tx, _)| tx.clone());

    let app_data = app.clone();
    // 克隆日志发送器以注入 on_data 回调
    let log_tx = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };
    let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {
        let _ = app_data.emit("session-data", serde_json::json!({
            "session_id": session_id,
            "data": data,
        }));
        // 异步发送 RX 数据日志（非阻塞）
        let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {
            session_id: session_id.clone(),
            direction: DataDirection::RX,
            data_mode: data_mode.clone(),
            payload: data.clone(),
            timestamp: Local::now(),
        }));
        // 转发到虚拟端口桥接（best-effort，虚拟端口未启用时忽略）
        if let Some(ref tx) = bridge_tx {
            let _ = tx.try_send(data);
        }
        // 数据推送至脚本引擎由 CommHandle::notify_receive() 统一扇出，
        // 脚本引擎通过 CommHandle::on_receive() 注册回调接收数据
    });

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |session_id| {
        let app_state: State<AppState> = app_disconnect.state();

        // 1. 在 mark_disconnected 之前读取虚拟端口对
        //    （mark_disconnected 内部关闭桥接线程，但不销毁 pairs）
        let pairs: Vec<PortPair> = {
            let store = match app_state.session_store.lock() {
                Ok(s) => s,
                Err(e) => e.into_inner(),
            };
            store
                .get_session(&session_id)
                .map(|h| h.virtual_port_pairs.clone())
                .unwrap_or_default()
        };

        // 2. 标记断开 — 内部关闭桥接，PlugInMode 使 B 端自动隐藏
        //    同步保存到磁盘，防止后续崩溃导致配置丢失
        if let Ok(mut store) = app_state.session_store.lock() {
            store.mark_disconnected(&session_id);
            let path = SessionStore::sessions_file_path(&app_disconnect);
            let _ = store.save_to_disk(&path);
        }

        // 3. 从内核驱动删除端口对 → 外部工具感知 COM 端口消失
        if !pairs.is_empty() {
            if let Ok(mut vpm) = app_state.virtual_port_manager.lock() {
                for pair in &pairs {
                    let _ = vpm.destroy_pair(pair);
                }

                // 检查是否有因权限不足而写入 state 文件的残留端口
                // UAC 弹窗推迟到下次用户主动操作（状态栏 [清理残留端口] 按钮或
                // 下次连接的 create_pairs_elevated），避免在断开回调中突然弹窗
                let orphan_count = vpm.pending_orphan_count();
                if orphan_count > 0 {
                    log::warn!(
                        "Session {} disconnected: {} port pair(s) need admin cleanup — \
                         deferred to next explicit user action",
                        session_id, orphan_count
                    );
                }

                log::info!(
                    "已清理断开会话 {} 的虚拟端口对 ({} 对)",
                    session_id,
                    pairs.len()
                );
            }
        }

        let _ = app_disconnect.emit("session-disconnected", serde_json::json!({
            "session_id": session_id,
        }));
    });

    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "ymodem".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    // 在作用域块内创建会话并保存，利用 RAII 自动释放 MutexGuard
    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let session_id = store.create_session(
            &session_name, "serial", &endpoint, params, channel,
            on_data, on_disconnect, app.clone(),
            transfer_enabled_val,
            Some(transfer_protocol_val.clone()),
            send_bar_enabled_val,
            session_id,
        )?;

        // 自动保存
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        session_id
    };

    // ── 虚拟串口桥接 ──
    // virtual_enabled 已在上面读取，这里只读取 virtual_count
    let virtual_count = params_clone
        .get("virtual_port_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    // vport_pairs_json declared here so it's in scope for the session-connected emit below
    // (even when virtual ports are disabled)
    let mut vport_pairs_json: Vec<serde_json::Value> = Vec::new();

    // ── Virtual port pair creation + bridge thread setup ──
    // TODO: Extract into setup_virtual_port_bridge() helper once the parameter
    // surface stabilizes (currently touches vpm, session_store, app, bridge channel).
    if virtual_enabled && virtual_count > 0 {
        let config = VirtualPortConfig { enabled: true, count: virtual_count };
        let mut vpm = state.virtual_port_manager.lock().map_err(|e| e.to_string())?;

        let pairs: Vec<PortPair> = vpm.create_pairs(&config)
            .or_else(|first_err| {
                log::warn!("直接创建端口对失败: {}；尝试先安装驱动...", first_err);
                vpm.install_driver()
                    .and_then(|_| vpm.create_pairs(&config))
            })
            .unwrap_or_else(|e| {
                let is_elevation = contains_elevation_indicator(&e);
                if is_elevation && vpm.detect_driver() {
                    log::info!("驱动已安装，尝试通过 UAC 提权创建端口对...");
                    match vpm.create_pairs_elevated(&config) {
                        Ok(pairs) => return pairs,
                        Err(elevated_err) => log::warn!("提权创建端口对也失败: {}", elevated_err),
                    }
                }
                log::warn!("虚拟端口创建失败: {}", e);
                Vec::new()
            });
        drop(vpm);

        // 序列化 pairs 供 session-connected 事件使用
        vport_pairs_json = pairs.iter().map(|p| serde_json::json!({
            "port_a": p.port_a,
            "port_b": p.port_b,
        })).collect();

        if !pairs.is_empty() {
            let virtual_port_names: Vec<String> = pairs.iter().map(|p| p.port_a.clone()).collect();
            let (_bridge_tx, bridge_rx) = bridge
                .take()
                .expect("bridge must be Some when virtual_enabled is true");

            // 桥接线程 → 物理端口写线程 channel（容量 128）
            // 使用独立 channel 避免桥接循环内获取 SessionStore Mutex
            let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(BRIDGE_WRITEBACK_CHANNEL_CAPACITY);

            // 独立写线程：消费桥接线程的虚拟端口数据，写入物理端口
            // 只有此线程持有 SessionStore Mutex，阻塞不影响桥接循环
            let app_for_write = app.clone();
            let sid = session_id.clone();
            std::thread::spawn(move || {
                while let Ok(data) = write_rx.recv() {
                    if let Ok(store) = app_for_write.state::<AppState>().session_store.lock() {
                        let _ = store.write(&sid, &data);
                    }
                }
                log::trace!("桥接写线程退出: session={}", sid);
            });

            // Extract baud rate from serial config for virtual port opening
            let vport_baud_rate = params_clone
                .get("baud_rate")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(115200);

            let vport_bridge = VirtualPortBridge::spawn(virtual_port_names, vport_baud_rate, bridge_rx, write_tx);

            {
                let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
                if let Some(handle) = store.get_session_mut(&session_id) {
                    handle.virtual_port_bridge = Some(vport_bridge);
                    handle.virtual_port_pairs = pairs.clone();
                }
            }

            // 保留独立事件供 reconnect 场景（tab 已存在时更新 VPort 信息）
            let _ = app.emit("virtual-port-created", serde_json::json!({
                "session_id": session_id,
                "pairs": &vport_pairs_json,
            }));
        } else {
            let reason = "com0com driver not installed — run TauTerm as administrator once, or reinstall the application";
            log::warn!("虚拟端口创建失败 (session={}): {}", session_id, reason);
            let _ = app.emit("virtual-port-failed", serde_json::json!({
                "session_id": session_id,
                "reason": reason,
            }));
        }
    }
    // virtual_enabled=true 时 bridge_rx 被 VirtualPortBridge::spawn() 消费，
    // virtual_enabled=false 时 bridge Option 在此 drop（通道未创建）。
    // bridge_tx 仅在 virtual_enabled=true 时存在，每个 on_data 回调检查并跳过 None 情况。

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&session_id)
            .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
            .unwrap_or((session_name, params_clone, None))
    };

    log::info!("会话已连接: {} @ {} (data_mode={})", actual_name, endpoint, data_mode_for_log);

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
        "send_bar_enabled": send_bar_enabled_val,
        // 合并虚拟端口对信息到 session-connected 中，
        // 避免 virtual-port-created 事件先于 session-connected 到达
        // 前端时因 tab 尚未创建而丢失数据
        "virtual_port_pairs": vport_pairs_json,
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
    // 单次锁获取：读取 → 保存 → 关闭（close_session 内部关闭桥接）
    let (pairs_to_destroy, session_name) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);

        let handle = store.get_session(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?;
        let pairs = handle.virtual_port_pairs.clone();
        let name = handle.name.clone();
        store.close_session(&session_id)?;
        (pairs, name)
    };
    // 锁已释放 — close_session 内部已关闭桥接

    // 销毁虚拟端口对（从内核驱动移除 → 外部工具感知 COM 端口消失）
    if !pairs_to_destroy.is_empty() {
        if let Ok(mut vpm) = state.virtual_port_manager.lock() {
            for pair in &pairs_to_destroy {
                let _ = vpm.destroy_pair(pair);
                // destroy_pair 对权限错误返回 Ok(()) 但通过 mark_for_deferred_cleanup
                // 将 bus 号写入 state 文件，后续统一 UAC 清理
            }

            // 检查是否有因权限不足而写入 state 文件的残留端口
            if vpm.pending_orphan_count() > 0 {
                log::info!(
                    "断开连接: {} 个端口对需要管理员权限，通过 UAC 批量清理...",
                    vpm.pending_orphan_count()
                );
                match vpm.cleanup_pairs_elevated() {
                    Ok(cleaned) => {
                        log::info!(
                            "断开连接: 通过 UAC 成功清理 {} 个端口对",
                            cleaned
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "断开连接: UAC 清理失败: {} — 可通过状态栏[清理残留端口]按钮手动清理",
                            e
                        );
                    }
                }
            }
        }
    }

    log::info!("会话已断开: {} (虚拟端口已清理)", session_name);
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
    // 单次锁定 session_store：写入数据并获取 data_mode
    let data_mode = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.write(&session_id, &data)?;
        store
            .get_session(&session_id)
            .and_then(|h| h.params.get("data_mode"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "text".to_string())
    };
    // 异步发送 TX 数据日志（非阻塞，best-effort：失败不影响主流程）
    if let Ok(log_engine) = state.log_engine.lock() {
        let _ = log_engine.sender().try_send(LogEntry::SessionData(DataLogEntry {
            session_id,
            direction: DataDirection::TX,
            data_mode,
            payload: data,
            timestamp: Local::now(),
        }));
    }
    Ok(())
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
        send_bar_enabled: s.send_bar_enabled,
        virtual_port_enabled: s.virtual_port_enabled,
        virtual_port_count: s.virtual_port_count,
    }).collect())
}

// ── 会话配置命令 ─────────────────────────────────────

/// TODO: 升级 Tauri v2 → v3 后，将多个参数收束为请求结构体
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn save_session_config(
    app: AppHandle,
    _state: State<AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    plugin_id: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    // 可选：传入已有的 session_id 以原地更新配置（保留 UUID 和 I/O 统计连续性）
    session_id: Option<String>,
) -> Result<String, String> {
    let pid = plugin_id.unwrap_or_else(|| "serial".into());
    let id = if let Some(ref raw) = session_id {
        if uuid::Uuid::parse_str(raw).is_err() {
            return Err(format!("无效的 session_id 格式: {}", raw));
        }
        raw.clone()
    } else {
        uuid::Uuid::new_v4().to_string()
    };
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
        send_bar_enabled: send_bar_enabled.unwrap_or(true),
        virtual_port_enabled: params
            .get("virtual_port_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        virtual_port_count: params
            .get("virtual_port_count")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0),
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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
        let not_found = store.session_not_found(&session_id);
        let handle = store.get_session_mut(&session_id)
            .ok_or(not_found)?;

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

// ── 日志引擎命令 ────────────────────────────────────

/// 启动会话数据日志记录
///
/// 锁顺序：session_store → log_engine（与 write_data 保持一致，避免死锁）
#[tauri::command]
pub fn start_session_log(
    state: State<AppState>,
    session_id: String,
) -> Result<String, String> {
    // 先锁定 session_store 读取会话信息（锁在块结束时释放）
    let (session_name, port_name, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?;
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

    // 再锁定 log_engine 发送启动命令
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;

    let cmd = LogEntry::Command(crate::kernel::log_engine::LogCommand::StartSession {
        session_id: session_id.clone(),
        session_name,
        port_name,
        data_mode,
    });

    log_engine
        .sender()
        .send(cmd)
        .map_err(|e| format!("发送日志启动命令失败: {}", e))?;

    Ok(session_id)
}

/// 停止会话数据日志记录
#[tauri::command]
pub fn stop_session_log(
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;

    let cmd = LogEntry::Command(crate::kernel::log_engine::LogCommand::StopSession {
        session_id,
    });

    log_engine
        .sender()
        .send(cmd)
        .map_err(|e| format!("发送日志停止命令失败: {}", e))?;

    Ok(())
}

/// 前端用户操作/事件日志
#[tauri::command]
pub fn log_event(
    state: State<AppState>,
    level: String,
    message: String,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;

    let _ = log_engine.sender().try_send(LogEntry::SystemEvent {
        level,
        message,
        timestamp: Local::now(),
    });

    Ok(())
}

/// 获取当前活跃日志状态
#[tauri::command]
pub fn get_log_status(
    state: State<AppState>,
) -> Result<Vec<LogStatus>, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_active_logs())
}

/// 更新系统日志配置（启用/禁用 + 最低日志级别）
#[tauri::command]
pub fn set_system_log_config(
    _state: State<AppState>,
    enabled: bool,
    level: String,
) -> Result<(), String> {
    crate::kernel::log_engine::set_system_log_config(enabled, &level);
    Ok(())
}

/// 获取日志目录路径
#[tauri::command]
pub fn get_log_dir(
    state: State<AppState>,
) -> Result<String, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let config = log_engine.get_config();
    Ok(config.log_dir.to_string_lossy().to_string())
}

/// 获取完整日志配置（供前端设置页面初始加载）
///
/// 返回前端友好的 `LogConfigResponse`（PathBuf 已转为字符串）。
/// 前端调用此命令获取 Rust 端的当前配置，确保 UI 显示与后端一致。
#[tauri::command]
pub fn get_log_config(
    state: State<AppState>,
) -> Result<LogConfigResponse, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_config_response())
}

/// 在系统文件管理器中打开日志目录
#[tauri::command]
pub fn open_log_dir(
    state: State<AppState>,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let config = log_engine.get_config();
    let path = config.log_dir.clone();
    let _ = std::fs::create_dir_all(&path);

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
    Ok(())
}

/// 更新日志引擎运行时配置（由前端设置页调用）
///
/// 消费者线程下次循环自动读取新配置，无需重启。
#[tauri::command]
pub fn update_log_config(
    state: State<AppState>,
    config: LogConfigUpdate,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    log_engine.update_config(config);
    Ok(())
}

/// 清除所有日志文件
#[tauri::command]
pub fn clear_all_logs(
    state: State<AppState>,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let config = log_engine.get_config();

    // 1. 删除磁盘上的旧日志文件
    match std::fs::read_dir(&config.log_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "log") {
                    continue;
                }
                let _ = std::fs::remove_file(&path);
            }
            log::info!("所有日志文件已清除");
        }
        Err(e) => {
            // 目录不存在不算错误
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("清除日志失败: {}", e));
            }
        }
    }

    // 2. 通知消费者线程关闭旧文件句柄并创建新文件
    //    必须在删除之后发送：消费者收到此命令后会 flush 旧句柄
    //    并通过 rotate_file() 创建带递增序号的新文件
    let _ = log_engine.sender().send(LogEntry::Command(
        crate::kernel::log_engine::LogCommand::ReopenAfterClear,
    ));

    Ok(())
}

// ── 协议无关的传输辅助函数 ──────────────────────────

/// 通过 TransferProtocol trait 发送文件
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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

// ── 虚拟串口驱动管理 ────────────────────────────────

/// 查询 com0com 驱动状态（前端主动拉取，解决事件在组件挂载前发射的竞态）
#[tauri::command]
pub fn check_virtual_port_driver(
    state: State<AppState>,
) -> Result<serde_json::Value, String> {
    let vpm = state.virtual_port_manager.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "files_present": vpm.are_files_present(),
        "driver_installed": vpm.detect_driver(),
        "orphan_count": vpm.pending_orphan_count(),
    }))
}

/// 尝试安装 com0com 虚拟串口驱动
///
/// 优先直接安装（当前进程已提权时成功）；普通权限下则在 Windows 上
/// 通过 PowerShell Start-Process -Verb RunAs 触发 UAC 提权安装。
#[tauri::command]
pub fn install_virtual_port_driver(
    app: AppHandle,
    state: State<AppState>,
) -> Result<String, String> {
    let mut vpm = state.virtual_port_manager.lock().map_err(|e| e.to_string())?;

    // 先检测是否已安装
    if vpm.detect_driver() {
        log::info!("com0com 驱动已安装，无需重复操作");
        return Ok("already_installed".into());
    }

    // 检查驱动文件是否存在
    if !vpm.are_files_present() {
        return Err(
            "com0com driver files missing — please reinstall TauTerm".into()
        );
    }

    // 第 1 层: 尝试直接安装（当前进程已提权时成功）
    log::info!("尝试直接安装 com0com 驱动...");
    match vpm.install_driver() {
        Ok(()) => {
            let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
            return Ok("installed".into());
        }
        Err(direct_err) => {
            log::info!("直接安装失败: {}；尝试提权安装...", direct_err);
        }
    }

    // 第 2 层: 通过提权安装（UAC / sudo），逻辑下沉到 VirtualPortManager
    //      避免 commands 层直接依赖 com0com 的 setupc_path/resource_dir
    match vpm.install_driver_elevated() {
        Ok(()) => {
            log::info!("com0com 驱动提权安装成功");
            // 重新检测确认安装成功
            if vpm.detect_driver() {
                let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
                return Ok("installed".into());
            }
            Err("Driver installed but detection failed — please restart TauTerm".into())
        }
        Err(elevated_err) => {
            Err(format!(
                "Driver installation failed.\n\n{}\n\n\
                 Action: Run TauTerm as administrator once to install the driver.",
                elevated_err
            ))
        }
    }
}

/// 手动触发虚拟端口残留清理（通过 UAC 提权，单次弹窗）。
///
/// 收集所有已知的残留 bus 号（active_pairs + com0com_state.json + 驱动真实状态），
/// 通过单个提权的 PowerShell 脚本批量清理。
///
/// 返回 `{ cleaned: N, message: "..." }`。
#[tauri::command]
pub fn cleanup_virtual_ports(
    state: State<AppState>,
) -> Result<serde_json::Value, String> {
    let mut vpm = state.virtual_port_manager.lock().map_err(|e| e.to_string())?;

    // 先尝试直接清理孤儿端口（无需管理员权限的场景）
    let direct_cleaned = vpm.cleanup_orphans();

    // 检查是否还有残留需要 UAC 提权（pending_orphan_count > 0）
    let has_more_work = vpm.pending_orphan_count() > 0;

    if !has_more_work && direct_cleaned > 0 {
        return Ok(serde_json::json!({
            "cleaned": direct_cleaned,
            "message": format!("已清理 {} 个遗留端口对", direct_cleaned),
        }));
    }

    if !has_more_work && direct_cleaned == 0 {
        return Ok(serde_json::json!({
            "cleaned": 0,
            "message": "没有需要清理的端口对",
        }));
    }

    // 有残留且需要 UAC 提权
    log::info!(
        "cleanup_virtual_ports: 直接清理完成 {} 个，剩余端口对需要 UAC 提权",
        direct_cleaned
    );
    match vpm.cleanup_pairs_elevated() {
        Ok(uac_cleaned) => {
            let total = direct_cleaned + uac_cleaned;
            Ok(serde_json::json!({
                "cleaned": total,
                "message": format!("已清理 {} 个端口对（含 UAC 提权清理 {} 个）", total, uac_cleaned),
            }))
        }
        Err(e) => {
            if e.contains("取消") || e.contains("cancel") {
                Err(format!(
                    "用户取消了 UAC 提权弹窗（已直接清理 {} 个，余下将保留至下次操作）",
                    direct_cleaned
                ))
            } else {
                Err(format!("UAC 提权清理失败: {}", e))
            }
        }
    }
}

// ── 脚本引擎命令 ────────────────────────────────────

/// 启动会话的脚本引擎
///
/// 首次调用创建 Lua VM 线程，后续调用热加载新脚本代码。
#[tauri::command]
pub fn start_script_engine(
    app: AppHandle,
    state: State<AppState>,
    session_id: String,
    code: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.start_script(&session_id, &code, app)
}

/// 停止会话的脚本引擎
#[tauri::command]
pub fn stop_script_engine(
    state: State<AppState>,
    session_id: String,
) -> Result<(), String> {
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
    let re = regex::Regex::new(pattern)
        .map_err(|e| format!("正则语法错误: {}", e))?;
    let matched = if regex_data.is_empty() { None } else { Some(re.is_match(&regex_data)) };
    let groups: Vec<String> = if !regex_data.is_empty() {
        re.captures(&regex_data)
            .map(|caps| caps.iter()
                .map(|c| c.map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect())
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
                "contains" => test_data.windows(pat_bytes.len()).any(|w| w == pat_bytes.as_slice()),
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
    let lua = create_sandboxed_lua()
        .map_err(|e| format!("创建测试 VM 失败: {}", e))?;
    lua.globals()
        .set("__test_data", lua.create_string(data_str.as_bytes())
            .map_err(|e| format!("Lua 传值失败: {}", e))?)
        .map_err(|e| format!("Lua 传值失败: {}", e))?;
    lua.globals()
        .set("__test_pattern", lua.create_string(pattern.as_bytes())
            .map_err(|e| format!("Lua 传值失败: {}", e))?)
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
