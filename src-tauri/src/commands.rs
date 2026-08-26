//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。
//! 通过 SerialAdapter + SessionStore + Channel 架构管理会话。

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use crate::channel::io_loop::IoLoopCmd;
use crate::channel::AsyncChannel;
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogConfigResponse, LogConfigUpdate, LogEntry, LogStatus};
use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::kernel::script_engine::codegen::{hex_to_bytes, interpret_escape_sequences};
use crate::kernel::script_engine::sandbox::create_sandboxed_lua;
use tokio::sync::mpsc;
use crate::kernel::plugin_adapter::{ProtocolAdapter, TransferProtocolType};
use crate::kernel::session_store::{SessionState, SessionStore, IoTaskHandle};
use crate::virtual_port::bridge::VirtualPortBridge;
use crate::virtual_port::backend::{contains_elevation_indicator, PortPair, VirtualPortConfig};
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
    pub send_bar_enabled: bool,
    pub transfer_enabled: bool,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
        "ssh" => {
            // 通过适配器调用 discover_endpoints，保持与 serial 一致的插件架构。
            // SSH 当前返回空列表（无硬件端点），但未来可扩展为发现 mDNS/Bonjour SSH 主机等。
            let endpoints = state.ssh_adapter.discover_endpoints()
                .map_err(|e| e.to_string())?;
            Ok(endpoints.into_iter().map(|ep| EndpointItem {
                name: ep.name,
                description: ep.description,
                connection_type: "ssh".to_string(),
            }).collect())
        }
        "telnet" => {
            // 通过适配器调用 discover_endpoints，保持与 serial 一致的插件架构。
            // Telnet 当前返回空列表（无硬件端点），但未来可扩展为发现
            // 已知设备的 Telnet 服务等。
            let endpoints = state.telnet_adapter.discover_endpoints()
                .map_err(|e| e.to_string())?;
            Ok(endpoints.into_iter().map(|ep| EndpointItem {
                name: ep.name,
                description: ep.description,
                connection_type: "telnet".to_string(),
            }).collect())
        }
        other => Err(format!("插件 '{}' 暂不支持端点枚举", other)),
    }
}

// ── 命令：会话连接 ──────────────────────────────────

/// TODO: 升级 Tauri v2 → v3 后，将多个参数收束为请求结构体
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    plugin_id: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    journald_enabled: Option<bool>,
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
        "serial" => connect_session_serial(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, journald_enabled, session_id).await,
        "ssh" => connect_session_ssh(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, journald_enabled, session_id).await,
        "tftp" => connect_session_tftp(app, state, endpoint, params, name, session_id).await,
        "iperf" => connect_session_iperf(app, state, endpoint, params, name, session_id).await,
        "telnet" => connect_session_telnet(app, state, endpoint, params, name, send_bar_enabled, session_id).await,
        "network" => connect_session_network(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, session_id).await,
        other => Err(format!("插件 '{}' 的连接功能尚未实现", other)),
    }
}

/// BridgeChannel = (tx, rx) 类型别名
type BridgeChannel = (std::sync::mpsc::SyncSender<Vec<u8>>, std::sync::mpsc::Receiver<Vec<u8>>);

/// 创建 on_data 回调（含 DataBatcher + 日志记录 + 可选虚拟端口转发）。
///
/// DataBatcher 的所有权被移入回调闭包（通过 `batcher.push()` 消费数据），
/// 因此只返回 `Box<dyn Fn>`；`DataBatcher::Drop` 在会话断开时自动 flush + 清理。
///
/// `bridge_tx` 为可选虚拟端口转发通道（仅串口会话提供）。
/// 全部会话类型共用此函数，消除 ~60 行重复代码。
fn create_on_data_callback(
    app: &AppHandle,
    log_tx: std::sync::mpsc::SyncSender<LogEntry>,
    data_mode: String,
    encoding: String,
    bridge_tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
) -> Box<dyn Fn(String, Vec<u8>) + Send> {
    let app_clone = app.clone();
    let batcher = crate::kernel::data_batcher::DataBatcher::new(move |batched| {
        let _ = app_clone.emit("session-data", serde_json::json!({
            "session_id": batched.session_id,
            "data_b64": batched.data_b64,
        }));
    });

    Box::new(move |session_id, data| {
        // 日志和桥接需克隆数据；主路径（batcher）直接获取所有权，省去一次 clone
        let data_for_log = data.clone();
        let data_for_bridge = bridge_tx.as_ref().map(|_| data.clone());
        batcher.push(session_id.clone(), data);
        let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {
            session_id: session_id.clone(),
            direction: DataDirection::RX,
            data_mode: data_mode.clone(),
            encoding: encoding.clone(),
            payload: data_for_log,
            timestamp: Local::now(),
        }));
        if let (Some(tx), Some(d)) = (bridge_tx.as_ref(), data_for_bridge) {
            let _ = tx.try_send(d);
        }
    })
}

/// 串口会话连接（新架构：SerialAdapter → Channel → SessionStore）
#[allow(clippy::too_many_arguments)]
async fn connect_session_serial(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    _journald_enabled: Option<bool>,
    session_id: Option<String>,
) -> Result<String, String> {
    // 通过 SerialAdapter（ProtocolAdapter trait）创建连接产物
    let conn = state.serial_adapter.connect(&endpoint, &params).await
        .map_err(|e| e.to_string())?;

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
    // 会话字符编码（用于日志按编码解码为 UTF-8）
    let encoding_for_log = params_clone
        .get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("utf-8")
        .to_string();

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
    let log_tx = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };

    // 共享 on_data 回调：DataBatcher + 日志 + 虚拟端口转发
    // 数据推送至脚本引擎由 CommHandle::notify_receive() 统一扇出
    let on_data = create_on_data_callback(
        &app_data, log_tx, data_mode.clone(), encoding_for_log, bridge_tx,
    );

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |session_id| {
        let app_state: State<'_, AppState> = app_disconnect.state();

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
            &session_name, "serial", &endpoint, params, conn,
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

        // 记录虚拟端口创建失败的真实原因，用于 `virtual-port-failed` 事件，避免
        // 用一句写死的 "driver not installed" 掩盖真实问题（如端口耗尽、UAC 被取消）。
        let mut vport_error: Option<String> = None;
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
                vport_error = Some(e);
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
            // 使用真实失败原因，避免用一句写死的 "driver not installed" 掩盖
            // 端口耗尽 / UAC 被取消等真实问题。
            let detail = vport_error.clone().unwrap_or_else(|| {
                "com0com driver not installed. Run TauTerm as administrator once to install the driver."
                    .to_string()
            });
            // 粗略分类，供前端映射到 i18n 文案（而非把英文错误直接展示给用户）
            let detail_lower = detail.to_lowercase();
            let kind = if detail_lower.contains("driver files missing") {
                "files_missing"
            } else if detail_lower.contains("driver not installed") {
                "driver_missing"
            } else if contains_elevation_indicator(&detail) || detail_lower.contains("cancel") {
                "permission"
            } else {
                "create_failed"
            };
            log::warn!("虚拟端口创建失败 (session={}): {}", session_id, detail);
            let _ = app.emit("virtual-port-failed", serde_json::json!({
                "session_id": session_id,
                "kind": kind,
                "reason": detail,
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

/// Telnet 会话连接（TelnetAdapter → Channel → SessionStore）
///
/// 单连接/标签页模式（serial 式 Sync I/O），无文件传输、无容器/子会话。
/// 回显状态事件由通道内回调直接 emit（适配器持有 AppHandle，session_id
/// 经 `Channel::on_session_started` 注入），无需 relay 线程。
#[allow(clippy::too_many_arguments)]
async fn connect_session_telnet(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    send_bar_enabled: Option<bool>,
    session_id: Option<String>,
) -> Result<String, String> {
    let conn = state.telnet_adapter.connect(&endpoint, &params).await
        .map_err(|e| e.to_string())?;

    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state.telnet_adapter.content_type();
    let io_strategy = state.telnet_adapter.io_strategy();
    let transfer_protocols = state.telnet_adapter.transfer_protocols();
    log::info!(
        "Telnet 连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",
        content_type, io_strategy, transfer_protocols
    );

    let session_name = name.unwrap_or_else(|| format!("Telnet {}", endpoint));

    let app_data = app.clone();
    let log_tx = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };
    let data_mode = params
        .get("data_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();
    // 会话字符编码（用于日志按编码解码为 UTF-8）
    let encoding = params
        .get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("utf-8")
        .to_string();

    // 共享 on_data 回调：DataBatcher + 日志（无虚拟端口桥接）
    let on_data = create_on_data_callback(&app_data, log_tx, data_mode, encoding, None);

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |session_id| {
        let app_state: State<'_, AppState> = app_disconnect.state();
        if let Ok(mut store) = app_state.session_store.lock() {
            store.mark_disconnected(&session_id);
            let path = SessionStore::sessions_file_path(&app_disconnect);
            let _ = store.save_to_disk(&path);
        }
        let _ = app_disconnect.emit("session-disconnected", serde_json::json!({
            "session_id": session_id,
        }));
    });

    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);
    // Telnet 无文件传输
    let transfer_enabled_val = false;

    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let sid = store.create_session(
            &session_name, "telnet", &endpoint, params, conn,
            on_data, on_disconnect, app.clone(),
            transfer_enabled_val,
            None,
            send_bar_enabled_val,
            session_id,
        )?;

        // 自动保存
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        sid
    };

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&session_id)
            .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
            .unwrap_or((session_name, Value::Null, None))
    };

    log::info!("Telnet 会话已连接: {} @ {}", actual_name, endpoint);

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": session_id,
        "endpoint": endpoint,
        "connection_type": "telnet",
        "plugin_id": "telnet",
        "name": actual_name,
        "params": actual_params,
        "connected_at": connected_at,
        "transfer_enabled": transfer_enabled_val,
        "send_bar_enabled": send_bar_enabled_val,
    }));

    Ok(session_id)
}

/// SSH 会话连接（新架构：SshAdapter::connect → ProtocolConnection → SessionStore）
#[allow(clippy::too_many_arguments)]
async fn connect_session_ssh(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    mut params: Value,
    name: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    journald_enabled: Option<bool>,
    session_id: Option<String>,
) -> Result<String, String> {
    let params_for_config = params.clone();
    let ssh_config: crate::plugins::ssh::SshConfig = serde_json::from_value(params_for_config)
        .map_err(|e| format!("SSH 配置解析失败: {}", e))?;

    // 将 journald_enabled 提升为 params 的通用字段（不再耦合 SshConfig）。
    // reconfigure/restore 时优先复用 params 中已有值，保证向前向后兼容。
    let journald_enabled_val = if let Some(obj) = params.as_object_mut() {
        if let Some(existing) = obj.get("journald_enabled").and_then(|v| v.as_bool()) {
            existing
        } else {
            let v = journald_enabled.unwrap_or(false);
            obj.insert("journald_enabled".to_string(), serde_json::Value::Bool(v));
            v
        }
    } else {
        journald_enabled.unwrap_or(false)
    };

    // 通过 SshAdapter::connect_with_config 获取 ProtocolConnection，
    // 复用已解析的 SshConfig 实例，避免 connect() 内部二次 JSON 反序列化。
    // 传入 AppHandle 和 HostKeyVerifier 以启用用户确认主机密钥流程。
    let conn = state.ssh_adapter.connect_with_config(
        ssh_config.clone(),
        app.clone(),
        &state.host_key_verifier,
    ).await.map_err(|e| e.to_string())?;

    // 提取主机密钥指纹（供前端展示确认）
    let host_key_fingerprint: Option<String> = conn.side_channel.as_ref()
        .and_then(|sc| sc.as_any().downcast_ref::<crate::plugins::ssh::SshSideChannel>())
        .and_then(|ssc| ssc.host_key_fingerprint.clone());

    if let Some(ref fp) = host_key_fingerprint {
        log::info!("SSH 主机密钥指纹: {}", fp);
    }

    let content_type = state.ssh_adapter.content_type();
    let io_strategy = state.ssh_adapter.io_strategy();
    let transfer_protocols_list = state.ssh_adapter.transfer_protocols();
    log::info!(
        "SSH 连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",
        content_type, io_strategy, transfer_protocols_list
    );

    let session_name = name.unwrap_or_else(|| format!("{}@{}", ssh_config.username, ssh_config.host));
    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "sftp".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    // 分离 side_channel（SSH Handle，供后续子连接复用）
    let side_channel = conn.side_channel;
    // 分离 channel（第一个 PTY，作为通道 0 的 I/O）
    let channel_for_ch0 = match conn.channel {
        Some(crate::kernel::plugin_adapter::ChannelKind::Async(ch)) => ch,
        Some(crate::kernel::plugin_adapter::ChannelKind::Sync(_)) => {
            return Err("SSH 连接期望 Async channel".to_string());
        }
        None => {
            return Err("SSH 连接缺少 I/O channel".to_string());
        }
    };

    // 1. 创建容器父 session（无 I/O loop）
    let parent_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store.create_container_session(
            &session_name, "ssh", &endpoint, params.clone(),
            side_channel,
            None,
            transfer_enabled_val,
            Some(transfer_protocol_val.clone()),
            send_bar_enabled_val,
            session_id.clone(),
        )?;
        pid
    };

    // 2. 通过共享逻辑创建通道 0（名称由 create_ssh_sub_channel 按 channel_index 自动生成）
    let channel0_id = create_ssh_sub_channel(
        &app, &*state, &parent_id, channel_for_ch0,
    ).await
        .inspect_err(|e| {
            // 子通道创建失败 → 回滚清理父容器会话，避免资源泄漏
            log::error!("SSH 通道 0 创建失败，回滚父容器会话 {}: {}", parent_id, e);
            if let Ok(mut store) = state.session_store.lock() {
                let _ = store.close_session(&parent_id);
            }
        })?;

    // 3. 读取父会话信息 + emit 父容器 session-connected（前端不创建额外的根 tab）
    let (actual_name, actual_params) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&parent_id)
            .map(|h| (h.name.clone(), h.params.clone()))
            .unwrap_or((session_name, params.clone()))
    };

    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );

    log::info!("SSH 会话已连接: {} @ {} (parent: {}, channel_0: {})", actual_name, endpoint, parent_id, channel0_id);

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": parent_id,
        "endpoint": endpoint,
        "connection_type": "ssh",
        "plugin_id": "ssh",
        "name": actual_name,
        "params": actual_params,
        "connected_at": connected_at,
        "transfer_enabled": transfer_enabled_val,
        "transfer_protocol": transfer_protocol_val,
        "send_bar_enabled": send_bar_enabled_val,
        "file_service_enabled": ssh_config.file_service_enabled,
        "file_service_protocol": ssh_config.file_service_protocol,
        "journald_enabled": journald_enabled_val,
        "host_key_fingerprint": host_key_fingerprint,
        "is_container": true,
    }));

    Ok(parent_id)
}

// ── 命令：SSH 主机密钥确认 ────────────────────────────

/// 用户确认或拒绝 SSH 主机密钥。
///
/// SSH 连接过程中，`build_connection_with_config` 发现新主机密钥时
/// 通过 `ssh-host-key-verify` 事件将指纹发送到前端。
/// 前端展示确认对话框后调用此命令，由 `HostKeyVerifier` 将用户决策
/// 回传给正在阻塞等待的 `build_connection_with_config`。
#[tauri::command]
pub async fn confirm_host_key(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
    accepted: bool,
) -> Result<(), String> {
    let ok = state.host_key_verifier.respond(&fingerprint, accepted).await;
    if !ok {
        // 指纹未找到：可能已超时、重复确认、或从未发起。
        // 返回错误信息以便前端显示给用户。
        return Err(format!(
            "主机密钥验证请求未找到或已过期（指纹: {}）。可能已超时或重复确认。",
            &fingerprint[..fingerprint.len().min(40)]
        ));
    }
    log::info!(
        "SSH 主机密钥 {}: {}",
        if accepted { "已接受" } else { "已拒绝" },
        &fingerprint[..fingerprint.len().min(40)]
    );
    Ok(())
}

// ── 命令：会话断开 ──────────────────────────────────

#[tauri::command]
pub async fn disconnect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 单次锁获取：读取 → 保存 → 关闭（close_session 内部调用 shutdown() 清理侧通道）
    let (pairs_to_destroy, session_name, is_tftp, is_iperf) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;

        let handle = store.get_session(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?;
        let pairs = handle.virtual_port_pairs.clone();
        let name = handle.name.clone();
        let is_tftp = handle.plugin_id == "tftp";
        let is_iperf = handle.plugin_id == "iperf";
        store.close_session(&session_id)?;
        // 持久化：会话状态已变为 Disconnected，写入磁盘
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        (pairs, name, is_tftp, is_iperf)
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
    // TFTP 会话断开后通知前端服务端已停止
    if is_tftp {
        let _ = app.emit("tftp-server-status", serde_json::json!({
            "session_id": session_id,
            "running": false,
        }));
    }
    // iperf 会话断开 = 服务端生命周期结束（shutdown() 已复位 server_running，
    // 此处显式通知前端刷新右侧面板状态）
    if is_iperf {
        let _ = app.emit("iperf-server-status", serde_json::json!({
            "session_id": session_id,
            "running": false,
        }));
    }
    let _ = app.emit("session-disconnected", serde_json::json!({
        "session_id": session_id,
    }));
    Ok(())
}

/// 向指定会话写入数据
///
/// `transcode` 为 true 时（文本发送路径），将前端 UTF-8 字节委托会话
/// `CommHandle::send_text` 按会话编码转码后写设备；false 时（HEX 发送 /
/// 脚本原始字节路径）原样透传。转码策略只存在于 CommHandle（单一知识源），
/// 未来协议原生 handle 可自行覆盖。
/// 返回实际写入设备的字节（文本路径为转码后字节），供前端 TX 显示与
/// 日志面板使用，保证面板所见与线上字节一致。
#[tauri::command]
pub fn write_data(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    // 锁内仅解析会话参数（O(1)）；转码（encoding_rs 编码）与通道写入在锁外执行，
    // 避免 CPU 密集的转码阻塞其他会话的写入
    let (encoding, data_mode, comm) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        // 解析子连接 ID → 父会话以获取会话参数
        let resolved_id = store.resolve_parent_id(&session_id)
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
        // 克隆 Arc 后释放锁；send_text 内部完成转码（含 UTF-8 短路与未知编码透传）。
        // 对端（网络调试）拥有各自 CommHandle，文本路径按对端编码转码。
        (encoding, data_mode, store.get_comm_handle_for(&session_id))
    };
    // 文本路径：委托会话 CommHandle::send_text（单一转码策略点）；
    // 字节路径或会话无 comm_handle（SSH 容器）：直接写入原样透传
    let data_out = match (transcode, comm) {
        (true, Some(handle)) => handle.send_text(&data).map_err(|e| e.to_string())?,
        (true, None) | (false, _) => {
            let store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.write(&session_id, &data)?;
            data
        }
    };
    // 异步发送 TX 数据日志（非阻塞，best-effort：失败不影响主流程）
    // 日志记录实际写入设备的字节（转码后），text 格式按会话编码解码回 UTF-8
    if let Ok(log_engine) = state.log_engine.lock() {
        let _ = log_engine.sender().try_send(LogEntry::SessionData(DataLogEntry {
            session_id,
            direction: DataDirection::TX,
            data_mode,
            encoding,
            payload: data_out.clone(),
            timestamp: Local::now(),
        }));
    }
    Ok(data_out)
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
    let _ = app.emit("session-switched", serde_json::json!({
        "session_id": session_id,
    }));
    Ok(())
}

/// 重命名会话
#[tauri::command]
pub fn rename_session(
    app: AppHandle,
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    session_ids: Vec<String>,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.reorder_tabs(session_ids)?;
    Ok(())
}

/// 获取所有标签页信息（含子连接）
#[tauri::command]
pub fn get_tabs(
    state: State<'_, AppState>,
) -> Result<Vec<TabInfo>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let mut tabs: Vec<TabInfo> = store.tab_ids().iter().filter_map(|id| {
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
        })
    }).collect();
    // 添加子连接
    for id in store.tab_ids() {
        if let Some(h) = store.get_session(&id) {
            for sub in &h.sub_connections {
                if sub.state == SessionState::Disconnected {
                    continue; // 跳过已断开的子连接，等待 channel-closed 事件触发 REMOVE_CHILD
                }
                if !sub.tabbed {
                    continue; // 会话内对端（网络调试）不占标签页，由自定义视图展示
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
                });
            }
        }
    }
    Ok(tabs)
}

// ── SSH 子通道创建（共享逻辑）───────────────────────

/// 在已有 SSH 父会话上创建子通道。
///
/// 供 [`connect_session_ssh`]（channel-0）和 [`open_channel`]（channel-1+）共用。
/// 所有配置均从父 [`ActiveSessionHandle`] 统一读取，确保所有通道行为完全一致。
/// 通道名称按 `channel_index + 1` 自动生成为 `"Channel N"`。
async fn create_ssh_sub_channel(
    app: &tauri::AppHandle,
    app_state: &AppState,
    parent_id: &str,
    channel: Box<dyn AsyncChannel>,
) -> Result<String, String> {
    // ── 阶段 1: 获取锁 → 检查父存活 + 预留 channel_index + 读取配置 → 释放锁 ──
    let (endpoint, params, data_mode, encoding, channel_index, reserved_name, send_bar_enabled_val, file_service_enabled, journald_enabled, file_service_protocol) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        let dm = handle.params.get("data_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let enc = handle.params.get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8")
            .to_string();
        let idx = handle.sub_connections.len() as u32;
        let ch_name = format!("Channel {}", idx + 1);
        let sbe = handle.send_bar_enabled;
        let fse = handle.params.get("file_service_enabled")
            .and_then(|v| v.as_bool()).unwrap_or(false);
        let jde = handle.params.get("journald_enabled")
            .and_then(|v| v.as_bool()).unwrap_or(false);
        let fsp = handle.params.get("file_service_protocol")
            .and_then(|v| v.as_str()).unwrap_or("sftp").to_string();
        (handle.endpoint.clone(), handle.params.clone(), dm, enc, idx, ch_name, sbe, fse, jde, fsp)
    };

    // ── 阶段 2: 创建 I/O 资源（无锁）──
    let channel_id = uuid::Uuid::new_v4().to_string();
    let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<IoLoopCmd>(256);
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let tx_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let rx_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let tx_clone = tx_bytes.clone();
    let rx_clone = rx_bytes.clone();

    let log_tx = {
        let log_engine = app_state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };
    let on_data = create_on_data_callback(app, log_tx, data_mode, encoding, None);

    let app_disconnect = app.clone();
    let pid = parent_id.to_string();
    let ch_id = channel_id.clone();
    let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |channel_id| {
        let parent_disconnected = {
            if let Ok(mut store) = app_disconnect.state::<AppState>().session_store.lock() {
                store.mark_sub_disconnected(&pid, &channel_id);
                store.get_session(&pid)
                    .map(|h| h.state == SessionState::Disconnected)
                    .unwrap_or(false)
            } else {
                false
            }
        };
        let _ = app_disconnect.emit("channel-closed", serde_json::json!({
            "channel_id": channel_id,
            "parent_id": pid,
        }));
        if parent_disconnected {
            let _ = app_disconnect.emit("session-disconnected", serde_json::json!({
                "session_id": pid,
                "reason": "网络连接丢失",
            }));
        }
    });

    let io_handle = IoTaskHandle::Async(crate::channel::async_io_loop::spawn_async_io_loop(
        channel,
        ch_id.clone(),
        Box::new(on_data),
        on_disconnect,
        write_rx,
        cancel_rx,
        tx_clone,
        rx_clone,
    ));

    let stats_cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );

    crate::kernel::session_store::SessionStore::spawn_stats_collector(
        app.clone(),
        channel_id.clone(),
        tx_bytes.clone(),
        rx_bytes.clone(),
        connected_at,
        stats_cancel_flag.clone(),
    );

    // ── 阶段 3: 获取锁 → 验证父仍存活 → push sub_connection 或回滚清理 ──
    let (actual_index, channel_name) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            // 父会话在阶段 1 和 3 之间被断开 — 回滚清理
            stats_cancel_flag.store(true, Ordering::SeqCst);
            let _ = cancel_tx.send(());
            log::warn!(
                "父会话 {} 在子通道创建中途断开，已清理 I/O 资源: {}",
                parent_id, channel_id
            );
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        // 验证 channel_index 未被其他并发请求抢占（正常不应发生，但防御性检查）
        let actual_idx = handle.sub_connections.len() as u32;
        let actual_name = if actual_idx == channel_index {
            reserved_name
        } else {
            format!("Channel {}", actual_idx + 1)
        };

        let mut sub = crate::kernel::session_store::SubConnection::new(
            channel_id.clone(),
            actual_name.clone(),
            write_tx,
            io_handle,
            actual_idx,
            Some(cancel_tx),
        );
        sub.connected_at = connected_at;
        sub.stats_cancel_flag = Some(stats_cancel_flag);
        handle.sub_connections.push(sub);

        let path = crate::kernel::session_store::SessionStore::sessions_file_path(app);
        let _ = store.save_to_disk(&path);
        (actual_idx, actual_name)
    };

    // 8. Emit session-connected — 所有字段均从父会话继承
    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": channel_id,
        "endpoint": endpoint,
        "connection_type": "ssh",
        "plugin_id": "ssh",
        "name": channel_name,
        "params": params,
        "connected_at": connected_at,
        "transfer_enabled": false,
        "send_bar_enabled": send_bar_enabled_val,
        "parent_id": parent_id,
        "channel_index": actual_index,
        "file_service_enabled": file_service_enabled,
        "file_service_protocol": file_service_protocol,
        "journald_enabled": journald_enabled,
    }));

    log::info!(
        "SSH 子通道已创建: {} (parent: {}, channel_index: {})",
        channel_id, parent_id, actual_index
    );
    Ok(channel_id)
}

// ── SSH 多连接命令 ─────────────────────────────────

/// 在已有 SSH 会话上打开新的 PTY channel（不重复 TCP/握手/认证）。
#[tauri::command]
pub async fn open_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    // 1. 获取 SSH side_channel（russh Handle）
    let ssh_handle = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let side = store.get_side_channel(&session_id)
            .ok_or_else(|| format!("会话 {} 没有 SSH side channel（可能是串口会话）", session_id))?;
        let ssh_side = side.as_any()
            .downcast_ref::<crate::plugins::ssh::SshSideChannel>()
            .ok_or("无法获取 SSH 会话句柄")?;
        ssh_side.handle().clone()
    };

    // 2. 打开新 PTY + shell
    let ssh_channel = crate::plugins::ssh::open_pty_shell_channel(ssh_handle)
        .await
        .map_err(|e| format!("打开新终端失败: {}", e))?;

    // 3. 通过共享逻辑创建子通道（名称由 create_ssh_sub_channel 按 channel_index 自动生成）
    let channel_id = create_ssh_sub_channel(
        &app, &*state, &session_id, Box::new(ssh_channel),
    ).await?;

    log::info!("SSH 子连接已打开: {} (parent: {})", channel_id, session_id);
    Ok(channel_id)
}

/// 关闭单个子连接（若为最后一个则自动断开父会话）。
#[tauri::command]
pub async fn close_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 两段式关闭：锁内发信号并取出 join 句柄，锁外 join I/O 线程
    // （I/O 线程退出路径可能触发 on_disconnect 回调，回调需获取 store 锁）
    let (parent_id, is_last, cleanup) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store.find_parent_of_channel(&session_id)
            .ok_or_else(|| format!("子连接 {} 未找到", session_id))?;
        let (last, cleanup) = store.close_sub_connection(&pid, &session_id)?;
        if last {
            store.close_session(&pid)?;
            // 持久化：父会话已断开
            let path = SessionStore::sessions_file_path(&app);
            let _ = store.save_to_disk(&path);
        }
        (pid, last, cleanup)
    };
    // 锁外等待 I/O 线程与脚本线程真实退出
    cleanup.join();

    // 通知前端（仅 session-disconnected；channel-closed 由 on_disconnect 回调单独发出）
    if is_last {
        let _ = app.emit("session-disconnected", serde_json::json!({
            "session_id": parent_id,
            "reason": "所有终端已关闭",
        }));
    }

    log::info!("SSH 子连接已关闭: {}", session_id);
    Ok(())
}

// ── 网络调试会话命令 ────────────────────────────────

/// 获取网络调试会话的对端列表（自定义视图初始化用）
#[tauri::command]
pub fn list_network_peers(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<crate::kernel::session_store::PeerInfo>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    Ok(store.list_peers(&session_id))
}

/// 关闭单个对端（网络调试）。
///
/// 与 `close_channel` 不同：关闭对端不级联断开父会话（监听器保持监听）。
#[tauri::command]
pub fn close_network_peer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 两段式：锁内信号 + 移除，锁外 join（同 close_channel）
    let cleanup = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store.find_parent_of_channel(&session_id)
            .ok_or_else(|| format!("对端 {} 未找到", session_id))?;
        let (_is_last, cleanup) = store.close_sub_connection(&pid, &session_id)?;
        cleanup
    };
    cleanup.join();
    Ok(())
}

/// 网络调试会话连接（容器会话 + NetworkSideChannel）
#[tauri::command]
pub async fn connect_session_network(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    transfer_enabled: Option<bool>,
    transfer_protocol: Option<String>,
    send_bar_enabled: Option<bool>,
    session_id: Option<String>,
) -> Result<String, String> {
    let conn = state
        .network_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;

    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            name.as_deref().unwrap_or("网络调试"),
            "network",
            &endpoint,
            params.clone(),
            conn.side_channel.clone(),
            conn.comm_handle.clone(),
            transfer_enabled.unwrap_or(false),
            transfer_protocol,
            // 网络调试启用全局发送栏（目标由 TargetBar 选择）
            send_bar_enabled.unwrap_or(true),
            session_id,
        )?
    };

    // 启动监听 / 接收线程（TCP Client 注册对端、TCP Server accept、UDP recv 路由）
    if let Some(sc) = &conn.side_channel {
        if let Some(net) = sc
            .as_any()
            .downcast_ref::<crate::plugins::network::NetworkSideChannel>()
        {
            net.start(app.clone(), &sid).map_err(|e| e.to_string())?;
        }
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
    let udp_local_addr = conn
        .side_channel
        .as_ref()
        .and_then(|sc| sc.as_any().downcast_ref::<crate::plugins::network::NetworkSideChannel>())
        .and_then(|net| net.udp_client_local_addr())
        .map(|a| a.to_string());

    let _ = app.emit("session-connected", serde_json::json!({
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
    }));
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
    let (net, encoding, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let sc = store
            .get_side_channel(&session_id)
            .ok_or("会话无网络侧通道".to_string())?;
        // 校验确为网络调试会话（锁外 downcast 使用）
        sc.as_any()
            .downcast_ref::<crate::plugins::network::NetworkSideChannel>()
            .ok_or("会话不是网络调试会话".to_string())?;
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
        // 锁内借用完成发送所需的全部引用值，Arc clone 保活到锁外
        (sc.clone(), encoding, data_mode)
    };
    let net = net
        .as_any()
        .downcast_ref::<crate::plugins::network::NetworkSideChannel>()
        .ok_or("会话不是网络调试会话".to_string())?;
    // 文本路径：UTF-8 → 会话编码转码（与 write_data 的 CommHandle::send_text 一致）；
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
        let _ = log_engine.sender().try_send(LogEntry::SessionData(DataLogEntry {
            session_id,
            direction: DataDirection::TX,
            data_mode,
            encoding,
            payload: out.clone(),
            timestamp: Local::now(),
        }));
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
    let sc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_side_channel(&session_id)
            .ok_or("会话无网络侧通道".to_string())?
    };
    let net = sc
        .as_any()
        .downcast_ref::<crate::plugins::network::NetworkSideChannel>()
        .ok_or("会话不是网络调试会话".to_string())?;
    net.set_send_target(target);
    Ok(())
}

// ── 会话持久化命令 ─────────────────────────────────

#[tauri::command]
pub fn save_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
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
        // 原样返回 params：会话配置（含 iperf 的 version/listen_ip/listen_port）
        // 持久化记忆——版本在连接对话框中配置，重启后必须保持用户选择
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
    _state: State<'_, AppState>,
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
    _state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    SessionStore::delete_config_from_disk(&app, &session_id)
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    account: String,
) -> Result<(), String> {
    state.credential_store.delete_credential(&account)
        .map_err(|e| e.to_string())
}

// ── ConfigStore 命令 ────────────────────────────────

#[tauri::command]
pub fn get_config(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<Value>, String> {
    Ok(state.config_store.get::<Value>(&key))
}

#[tauri::command]
pub fn set_config(
    state: State<'_, AppState>,
    key: String,
    value: Value,
) -> Result<(), String> {
    state.config_store.set(&key, &value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_config(
    state: State<'_, AppState>,
    key: String,
) -> Result<(), String> {
    state.config_store.delete(&key)
        .map_err(|e| e.to_string())
}

// ── ThemeEngine 命令 ────────────────────────────────

#[tauri::command]
pub fn get_theme_list(
    state: State<'_, AppState>,
) -> Vec<String> {
    state.theme_engine.theme_names()
}

#[tauri::command]
pub fn get_active_theme(
    state: State<'_, AppState>,
) -> String {
    state.theme_engine.active_name()
}

#[tauri::command]
pub fn set_theme(
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    // 先锁定 session_store 读取会话信息（锁在块结束时释放）
    let (session_name, port_name, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        // 解析子连接 ID → 父会话（子连接不在 HashMap 中）
        let resolved_id = store.resolve_parent_id(&session_id)
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
) -> Result<Vec<LogStatus>, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_active_logs())
}

/// 更新系统日志配置（启用/禁用 + 最低日志级别）
#[tauri::command]
pub fn set_system_log_config(
    _state: State<'_, AppState>,
    enabled: bool,
    level: String,
) -> Result<(), String> {
    crate::kernel::log_engine::set_system_log_config(enabled, &level);
    Ok(())
}

/// 获取日志目录路径
#[tauri::command]
pub fn get_log_dir(
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
) -> Result<LogConfigResponse, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    Ok(log_engine.get_config_response())
}

/// 在系统文件管理器中打开日志目录
#[tauri::command]
pub fn open_log_dir(
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    config: LogConfigUpdate,
) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    log_engine.update_config(config);
    Ok(())
}

/// 清除所有日志文件
#[tauri::command]
pub fn clear_all_logs(
    state: State<'_, AppState>,
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

// ── 虚拟串口驱动管理 ────────────────────────────────

/// 查询 com0com 驱动状态（前端主动拉取，解决事件在组件挂载前发射的竞态）
#[tauri::command]
pub fn check_virtual_port_driver(
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    session_id: String,
    code: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.start_script(&session_id, &code, app)
}

/// 停止会话的脚本引擎
#[tauri::command]
pub fn stop_script_engine(
    state: State<'_, AppState>,
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

// ── 命令：SSH 文件服务（SFTP）────────────────────
//
// SFTP 命令组遵循统一模式：get_ssh_side_channel() → 委托函数。
// 每条命令 2 行样板代码，显式优于隐式（macro 会破坏 IDE 导航和重构工具）。
// 如需缩减，可提取 sftp_command!(name, fn, ret_type, arg_pattern) 声明宏。

use crate::transfer::ssh_file_service::{
    sftp_list_dir, sftp_stat, sftp_read_head, sftp_chmod,
    sftp_delete, sftp_delete_recursive,
    sftp_rename, sftp_mkdir, sftp_new_file, sftp_delete_batch,
};
use crate::plugins::ssh::SshSideChannel;

/// 从 SessionStore 获取 SSH 侧通道（含 session 和 sftp 缓存）的共享句柄。
///
/// 通过 `SideChannel::as_any()` + `downcast_ref` 集中处理类型还原，
/// 避免每个 SFTP 命令重复样板代码。
///
/// 返回 `Arc<SshSideChannel>` 的克隆——其内部 `session` 字段为
/// `Arc<russh::client::Handle<SshHandler>>`（russh Handle 内部线程安全），
/// `sftp` 字段为 `Arc<tokio::sync::Mutex<Option<russh_sftp::client::SftpSession>>>`（惰性缓存）。
/// 通过 `Arc::clone` 共享同一底层资源，因此 SFTP 缓存在多次命令调用间保持有效。
fn get_ssh_side_channel(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<std::sync::Arc<SshSideChannel>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    // 支持子连接路由：若 session_id 是子连接，通过父会话获取 side_channel
    let parent_id = store.resolve_parent_id(session_id)
        .ok_or_else(|| store.session_not_found(session_id))?;
    let sc = store.get_side_channel(&parent_id)
        .ok_or_else(|| format!("会话 {} 不包含 SSH 侧通道（可能不是 SSH 连接）", parent_id))?;
    // 检查父会话状态
    if let Some(h) = store.get_session(&parent_id) {
        if h.state == SessionState::Disconnected {
            return Err("会话已断开".to_string());
        }
    }
    let ssh_sc_ref = sc.as_any().downcast_ref::<SshSideChannel>()
        .ok_or_else(|| "侧通道类型不匹配（期望 SshSideChannel）".to_string())?;
    // 通过克隆内部 Arc 字段构造新的 SshSideChannel，
    // 与 SessionStore 中持有的 Arc<dyn SideChannel> 共享同一 session 和 sftp 缓存。
    Ok(std::sync::Arc::new(SshSideChannel {
        session: ssh_sc_ref.session.clone(),
        sftp: ssh_sc_ref.sftp.clone(),
        host_key_fingerprint: ssh_sc_ref.host_key_fingerprint.clone(),
        home_dir: ssh_sc_ref.home_dir.clone(),
    }))
}

/// SFTP 列出远程目录
#[tauri::command]
pub async fn sftp_list_dir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<Vec<crate::transfer::ssh_file_service::SftpEntry>, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_list_dir(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// SFTP 获取文件信息
#[tauri::command]
pub async fn sftp_stat_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<crate::transfer::ssh_file_service::SftpFileInfo, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_stat(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// SFTP 读取文件头（用于预览）
#[derive(serde::Serialize)]
pub struct ReadHeadResult {
    pub data: Vec<u8>,
    pub total_size: u64,
}

#[tauri::command]
pub async fn sftp_read_head_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    max_bytes: u64,
) -> Result<ReadHeadResult, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let (data, total_size) = sftp_read_head(&ssh_sc.session, &ssh_sc.sftp, &remote_path, max_bytes).await?;
    Ok(ReadHeadResult { data, total_size })
}

/// SFTP 修改文件权限
#[tauri::command]
pub async fn sftp_chmod_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    mode: u32,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_chmod(&ssh_sc.session, &ssh_sc.sftp, &remote_path, mode).await
}

/// SFTP 删除文件或目录
#[tauri::command]
pub async fn sftp_delete_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_delete(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// SFTP 重命名/移动文件或目录
#[tauri::command]
pub async fn sftp_rename_cmd(
    state: State<'_, AppState>,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_rename(&ssh_sc.session, &ssh_sc.sftp, &from_path, &to_path).await
}

/// SFTP 创建目录
#[tauri::command]
pub async fn sftp_mkdir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_mkdir(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// SFTP 创建空文件
#[tauri::command]
pub async fn sftp_new_file_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_new_file(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// SFTP 批量删除
#[tauri::command]
pub async fn sftp_delete_batch_cmd(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_delete_batch(&ssh_sc.session, &ssh_sc.sftp, &paths).await
}

/// SFTP 递归删除目录（包括子内容）
#[tauri::command]
pub async fn sftp_delete_recursive_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    sftp_delete_recursive(&ssh_sc.session, &ssh_sc.sftp, &remote_path).await
}

/// 获取 SSH 会话的远程用户 home 目录
///
/// 连接建立阶段通过 `echo $HOME` 解析并缓存于 `SshSideChannel.home_dir`。
/// 若获取失败或值为 None，回退到 `"/"`。
#[tauri::command]
pub fn get_ssh_home_dir(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    Ok(ssh_sc.home_dir.clone().unwrap_or_else(|| "/".to_string()))
}

// ── Journald 日志查看器命令 ──────────────────────────

/// 启动 journald 实时流式追踪
///
/// 在远程 SSH 会话上打开 exec 通道，执行 `journalctl -o json -f`，
/// spawn tokio task 循环读取并为每条日志 emit `journald:entry` 事件。
#[tauri::command]
pub async fn start_journald_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    level: Option<String>,
    keyword: Option<String>,
    unit: Option<String>,
    kernel_only: Option<bool>,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since: None,
        until: None,
    };
    crate::plugins::ssh::journald::start_journald_stream(
        &ssh_sc.session,
        app,
        session_id,
        &filters,
    )
    .await
}

/// 停止 journald 实时追踪
///
/// 设置对应 session 的 cancel 标志，使 tokio 流式循环优雅退出。
#[tauri::command]
pub async fn stop_journald_stream(
    session_id: String,
) -> Result<(), String> {
    // 确认式停止：等待后端任务真正退出并释放注册表，
    // 保证返回后前端可立即重新开始（消除"已在运行中"窗口期）
    crate::plugins::ssh::journald::stop_journald_stream_confirm(&session_id).await;
    Ok(())
}

/// 查询 journald 历史日志（单次请求，支持游标分页）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn journald_query_cmd(
    state: State<'_, AppState>,
    session_id: String,
    level: Option<String>,
    keyword: Option<String>,
    unit: Option<String>,
    kernel_only: Option<bool>,
    since: Option<String>,
    until: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<crate::plugins::ssh::journald::JournaldQueryResponse, String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since,
        until,
    };
    let limit = limit.unwrap_or(100);
    let (entries, next_cursor) = crate::plugins::ssh::journald::journald_query(
        &ssh_sc.session,
        &filters,
        cursor.as_deref(),
        limit,
    )
    .await?;
    let has_more = entries.len() >= limit;
    Ok(crate::plugins::ssh::journald::JournaldQueryResponse {
        entries,
        next_cursor,
        has_more,
    })
}

/// 启动 journald 日志导出
///
/// 在远程 SSH 会话上循环分页拉取所有匹配过滤条件的日志条目，
/// 序列化为 JSON 后写入指定文件路径。spawn tokio task 异步执行，
/// 通过事件 `journald:export-progress` / `journald:export-complete` /
/// `journald:export-error` 向前端报告进度。
#[tauri::command]
pub async fn start_journald_export(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    file_path: String,
    level: Option<String>,
    keyword: Option<String>,
    unit: Option<String>,
    kernel_only: Option<bool>,
    since: Option<String>,
    until: Option<String>,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since,
        until,
    };
    crate::plugins::ssh::journald::start_journald_export(
        &ssh_sc.session,
        app,
        session_id,
        &filters,
        file_path,
    )
    .await
}

/// 停止 journald 日志导出
///
/// 设置对应 session 的 cancel 标志，使导出循循环优雅退出。
#[tauri::command]
pub fn stop_journald_export(
    session_id: String,
) -> Result<(), String> {
    crate::plugins::ssh::journald::stop_journald_export(&session_id);
    Ok(())
}

// ── 统一文件传输命令（协议无关）────────────────────────────

/// 统一文件传输发送命令（协议无关）
///
/// 前端统一入口。通过 TransferOrchestrator 策略模式分发到
/// Inline（串口 X/Y/ZModem）或 SideChannel（SSH SFTP）策略。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn file_transfer_send(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    protocol: String,
    file_paths: Vec<String>,
    remote_dir: Option<String>,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）。
    let internal_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.resolve_parent_id(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };

    let pt: TransferProtocolType = protocol.parse()
        .map_err(|_| format!("无效的传输协议: {}", protocol))?;

    // 构建 FileInfo 列表
    let files: Vec<crate::transfer::types::FileInfo> = file_paths.iter()
        .filter_map(|p| match crate::transfer::types::FileInfo::from_path(p) {
            Ok(info) => Some(info),
            Err(e) => { log::warn!("无法获取文件信息 {}: {}", p, e); None }
        })
        .collect();
    if files.is_empty() {
        return Err("没有可传输的有效文件".into());
    }

    // 创建进度通道
    let (progress_tx, progress_rx) = mpsc::unbounded_channel();

    log::info!("文件传输发送: protocol={}, client={}→internal={}, files={}",
        pt, session_id, internal_id, files.len());

    let orch = crate::transfer::orchestrator::create_orchestrator(&pt)?;
    orch.execute_send(
        app,
        crate::transfer::orchestrator::SendContext {
            session_id: internal_id,
            files,
            remote_dir,
            progress_tx,
            progress_rx,
            block_size,
            checksum_mode,
            streaming,
        },
        session_id, // client_session_id — 前端原始 ID，用于事件回传
    )
    .await
}

/// 统一文件传输接收命令（协议无关）
///
/// 通过 TransferOrchestrator 策略模式分发到
/// Inline（串口 X/Y/ZModem）或 SideChannel（SSH SFTP）策略。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn file_transfer_receive(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    protocol: String,
    download_dir: String,
    remote_paths: Vec<String>,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）。
    let internal_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.resolve_parent_id(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };

    let pt: TransferProtocolType = protocol.parse()
        .map_err(|_| format!("无效的传输协议: {}", protocol))?;

    let (progress_tx, progress_rx) = mpsc::unbounded_channel();

    log::info!(
        "文件传输接收: protocol={}, client={}→internal={}, download_dir={}, remote_paths=[{}]({} files)",
        pt, session_id, internal_id, download_dir,
        remote_paths.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
        remote_paths.len()
    );

    let orch = crate::transfer::orchestrator::create_orchestrator(&pt)?;
    orch.execute_receive(
        app,
        crate::transfer::orchestrator::ReceiveContext {
            session_id: internal_id,
            download_dir,
            remote_paths,
            progress_tx,
            progress_rx,
            block_size,
            checksum_mode,
            streaming,
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
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    // 解析子通道 ID → 父会话 ID（SSH 多连接支持）
    let resolved_id = store.resolve_parent_id(&session_id)
        .ok_or_else(|| store.session_not_found(&session_id))?;

    log::info!("请求取消传输: session={}", resolved_id);
    // 尝试两种取消路径：内联传输和侧通道传输
    let inline_result = store.cancel_transfer(&resolved_id);
    let sc_result = store.cancel_transfer_op(&resolved_id);
    // 只要其中一个成功即可
    if inline_result.is_ok() || sc_result.is_ok() {
        log::info!("传输取消已置位: session={}", resolved_id);
        Ok(())
    } else {
        log::warn!("取消失败：未找到进行中的传输: session={}", resolved_id);
        Err("取消失败：未找到进行中的传输".into())
    }
}

/// 请求 SSH PTY 窗口大小调整
///
/// 前端终端 resize 时调用，通过 IoLoopCmd::ResizePty 转发到 I/O 循环线程，
/// 再由 Channel::resize_pty 发送 window_change 请求到远端。
/// 非 SSH 协议（串口等）的 Channel 默认空实现，调用无副作用。
/// 支持子连接路由：若 session_id 属于 SSH 子通道，命令通过子通道的 write_tx 发送。
#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let tx = store.get_write_tx(&session_id)
        .ok_or_else(|| store.session_not_found(&session_id))?;
    tx.send(IoLoopCmd::ResizePty { cols, rows })
        .map_err(|e| format!("发送 resize 命令失败: {}", e))
}

// ═══════════════════════════════════════════════════════════════
// TFTP 协议命令
// ═══════════════════════════════════════════════════════════════

use crate::plugins::tftp::{
    self, TftpDynamicParams, TftpStatus,
};

/// TFTP 会话连接
///
/// 创建容器会话（无终端 I/O loop），然后自动启动服务端。
/// 侧通道 `TftpSideChannel` 持有 UDP socket，由独立线程处理所有传输。
async fn connect_session_tftp(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    // 通过 TftpAdapter 创建连接产物（channel=None，仅包含 side_channel）
    let conn = state.tftp_adapter.connect(&endpoint, &params).await
        .map_err(|e| e.to_string())?;

    let session_name = name.unwrap_or_else(|| {
        format!("TFTP :{}", endpoint)
    });

    let side_channel = conn.side_channel
        .ok_or_else(|| "TFTP 适配器未返回侧通道".to_string())?;

    // 重连保留上一轮动态参数（blksize/window 等会话内调整不因重连丢失）：
    // 新侧通道 dynamic_params 归默认值且无重同步，UI 显示与后端实际协商
    // 参数会永久分叉。快照须在 create_container_session 替换旧会话之前读取
    let prev_params: Option<tftp::TftpDynamicParams> = match session_id.as_deref() {
        Some(prev_sid) => state.session_store.lock().ok().and_then(|store| {
            store
                .get_session(prev_sid)
                .and_then(|h| h.side_channel.as_ref())
                .and_then(|sc| sc.as_any().downcast_ref::<tftp::TftpSideChannel>())
                .map(|tsc| tsc.get_params())
        }),
        None => None,
    };
    if let Some(prev) = prev_params {
        if let Some(tsc) = side_channel.as_any().downcast_ref::<tftp::TftpSideChannel>() {
            match tsc.dynamic_params.lock() {
                Ok(mut p) => *p = prev,
                Err(poisoned) => {
                    log::warn!("[TFTP] dynamic_params 锁中毒，恢复后写入重连参数");
                    *poisoned.into_inner() = prev;
                }
            }
        }
    }

    // 使用容器会话模式（无 I/O loop — TFTP 无终端数据流）
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let sid = store.create_container_session(
            &session_name, "tftp", &endpoint, params.clone(),
            Some(side_channel.clone()),
            None,  // comm_handle
            false,  // transfer_enabled
            None,   // transfer_protocol
            false,  // send_bar_enabled
            session_id,
        )?;
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        sid
    };

    log::info!("TFTP 会话已创建（容器模式）: {}", sid);

    // 自动启动服务端
    if let Err(e) = tftp::try_start_server(&app, &side_channel, &sid) {
        log::warn!("[TFTP] 服务端自动启动失败 (session={}): {}", sid, e);
    }

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": sid,
        "plugin_id": "tftp",
        "content_type": "custom",
        "endpoint": endpoint,
        "name": session_name,
        "connection_type": "tftp",
        "params": params,
        "send_bar_enabled": false,
        "transfer_enabled": false,
    }));

    // 不再用 200ms 延迟探测：服务端线程进入监听后权威 emit running:true
    //（socket 在连接时已同步绑定，bind 失败直接使连接失败）；
    // 挂载期首次 getStatus 兜底错过事件的情况

    Ok(sid)
}

/// 启动 TFTP 服务端
#[tauri::command]
pub async fn tftp_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let sc_arc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_side_channel(&session_id)
            .ok_or_else(|| format!("会话 {} 不包含侧通道", session_id))?
    };

    tftp::try_start_server(&app, &sc_arc, &session_id)?;

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
    // 全局锁只用于取 Arc（对齐 disconnect_session 先例：emit 期间不得持有
    // session_store 锁）
    let sc_arc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_side_channel(&session_id)
            .ok_or_else(|| format!("会话 {} 不包含侧通道", session_id))?
    };

    let tftp_sc = sc_arc.as_any()
        .downcast_ref::<tftp::TftpSideChannel>()
        .ok_or_else(|| "侧通道不是 TFTP 类型".to_string())?;

    tftp_sc.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    tftp_sc.server_running.store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit("tftp-server-status", serde_json::json!({
        "session_id": session_id,
        "running": false,
    }));

    Ok(())
}

/// TFTP 客户端 GET（下载）——自给自足，不依赖 side_channel
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无 side_channel）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_get(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: String,
    remote_ip: String,
    remote_port: u16,
    remote_filename: String,
    local_path: String,
    params: Value,
) -> Result<String, String> {
    log::info!("[TFTP Client] GET 请求: session={}, file={}, remote={}:{} → {}",
        session_id, remote_filename, remote_ip, remote_port, local_path);
    let params: TftpDynamicParams = serde_json::from_value(params)
        .map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 side_channel（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_get(
        app, session_id, transfer_id.clone(), remote_ip, remote_port, remote_filename,
        std::path::PathBuf::from(local_path), params,
    ).await?;

    Ok(transfer_id)
}

/// TFTP 客户端 PUT（上传）——自给自足，不依赖 side_channel
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无 side_channel）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_put(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: String,
    remote_ip: String,
    remote_port: u16,
    remote_filename: String,
    local_path: String,
    params: Value,
) -> Result<String, String> {
    log::info!("[TFTP Client] PUT 请求: session={}, file={}, remote={}:{} ← {}",
        session_id, remote_filename, remote_ip, remote_port, local_path);
    let params: TftpDynamicParams = serde_json::from_value(params)
        .map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 side_channel（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_put(
        app, session_id, transfer_id.clone(), remote_ip, remote_port, remote_filename,
        std::path::PathBuf::from(local_path), params,
    ).await?;

    Ok(transfer_id)
}

/// 同步 TFTP 服务端参数到 side_channel（客户端 GET/PUT 前调用）。
/// 若 side_channel 不存在（会话未连接），静默跳过。
fn sync_tftp_server_params(state: &AppState, session_id: &str, params: &TftpDynamicParams) {
    if let Ok(store) = state.session_store.lock() {
        if let Some(sc_arc) = store.get_side_channel(session_id) {
            if let Some(tftp_sc) = sc_arc.as_any().downcast_ref::<tftp::TftpSideChannel>() {
                *tftp_sc.dynamic_params.lock().unwrap() = params.clone();
                log::info!("[TFTP] 服务端参数已同步 (session={}, blksize={})", session_id, params.blksize);
            }
        }
    }
}

/// 更新 TFTP 动态参数
///
/// 若会话已连接（side_channel 存在），更新服务端的共享参数。
/// 若会话未连接，仅记录日志后返回 Ok——客户端操作从前端传参，不依赖此处。
#[tauri::command]
pub async fn tftp_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let new_params: TftpDynamicParams = serde_json::from_value(params)
        .map_err(|e| format!("参数解析失败: {}", e))?;

    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    if let Some(sc_arc) = store.get_side_channel(&session_id) {
        if let Some(tftp_sc) = sc_arc.as_any().downcast_ref::<tftp::TftpSideChannel>() {
            *tftp_sc.dynamic_params.lock().unwrap() = new_params;
            log::info!("TFTP 参数已更新 (session={})", session_id);
        }
    } else {
        log::warn!("TFTP 参数更新跳过：会话 {} 未连接（无 side_channel）", session_id);
    }
    Ok(())
}

/// 获取 TFTP 状态
///
/// 若会话已连接（side_channel 存在），从侧通道读取实时状态。
/// 若会话未连接，返回默认值（server_running=false，其余字段为空/默认）。
#[tauri::command]
pub async fn tftp_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<TftpStatus, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;

    if let Some(sc_arc) = store.get_side_channel(&session_id) {
        if let Some(tftp_sc) = sc_arc.as_any().downcast_ref::<tftp::TftpSideChannel>() {
            let server_running = tftp_sc.server_running.load(std::sync::atomic::Ordering::Relaxed);
            let listen_addr = Some(tftp_sc.config.listen_ip.clone());
            let listen_port = Some(tftp_sc.config.listen_port);
            let file_root = tftp_sc.config.file_root.clone();
            let dynamic_params = tftp_sc.get_params();
            drop(store);
            return Ok(TftpStatus {
                server_running,
                listen_addr,
                listen_port,
                file_root,
                dynamic_params,
            });
        }
    }
    drop(store);

    // 会话未连接（无 side_channel），返回默认值
    log::debug!("TFTP get_status: 会话 {} 未连接，返回默认状态", session_id);
    Ok(TftpStatus {
        server_running: false,
        listen_addr: None,
        listen_port: None,
        file_root: String::new(),
        dynamic_params: TftpDynamicParams::default(),
    })
}

// ═══════════════════════════════════════════════════════════════
// iperf 协议命令（iperf2 + iperf3）
// ═══════════════════════════════════════════════════════════════

use crate::plugins::iperf::{
    self, IperfDynamicParams, IperfStatus,
};

/// iperf 客户端测速任务注册表（keyed by session_id）。
///
/// 断连状态下的客户端测速任务注册表条目。
///
/// `abort` 供 `iperf_client_stop` 中止；`running` 是重跑守卫的事实源
/// （client 角色事件无 seq，两轮并发会在前端错配，必须串行）。
/// 条目按会话存续：任务结束不删除（运行标志跨 run 复用），会话重连
/// （侧通道接管）时由 `iperf_client_run` 清除。
#[derive(Clone)]
struct RegisteredClientRun {
    abort: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

static IPERF_CLIENT_REGISTRY: LazyLock<Mutex<HashMap<String, RegisteredClientRun>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// iperf 会话连接
///
/// 创建容器会话（无终端 I/O loop）。侧通道 `IperfSideChannel` 持有
/// 服务端监听线程句柄与测试状态。
/// 对齐 TFTP：连接即自动启动服务端（配置于 ConnectDialog 表单），
/// 断开自动停止；服务端生命周期跟随会话生命周期。
async fn connect_session_iperf(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: String,
    params: Value,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    // 通过 IperfAdapter 创建连接产物（channel=None，仅包含 side_channel）
    let conn = state.iperf_adapter.connect(&endpoint, &params).await
        .map_err(|e| e.to_string())?;

    let session_name = name.unwrap_or_else(|| {
        format!("iperf :{}", endpoint)
    });

    let side_channel = conn.side_channel
        .ok_or_else(|| "iperf 适配器未返回侧通道".to_string())?;

    // 重连保留上一轮动态参数：会话内调整的客户端目标端口/协议/时长等不因
    // 重连丢失（此前新侧通道回落到 config 播种值，用户 -p 编辑静默丢失）。
    // 快照须在 create_container_session 替换旧会话之前读取
    let prev_params: Option<iperf::IperfDynamicParams> =
        match session_id.as_deref() {
            Some(prev_sid) => state
                .session_store
                .lock()
                .ok()
                .and_then(|store| {
                    store
                        .get_session(prev_sid)
                        .and_then(|h| h.side_channel.as_ref())
                        .and_then(|sc| sc.as_any().downcast_ref::<iperf::IperfSideChannel>())
                        .map(|isc| isc.get_params())
                }),
            None => None,
        };
    if let Some(prev) = prev_params {
        if let Some(isc) = side_channel.as_any().downcast_ref::<iperf::IperfSideChannel>() {
            // version/listen_ip/listen_port 属会话配置，以本次连接解析结果
            // 为准（重配置可修改）；其余为会话内可调参数，跨重连保留。
            // 客户端目标端口联动：仅当旧端口仍是其版本的默认值（未自定义）
            // 时跟随新的监听端口——版本切换 5001↔5201 联动；自定义端口保留。
            // 注意必须按"旧版本默认值"判定（不能简单判定 ∈{5001,5201}）：
            // iperf2 下用户特意把 -p 设为 5201 测外部服务器时属于自定义，
            // 重连不得被改回监听端口（D1 修复保护的场景）
            let current = isc.get_params();
            let merged = iperf::IperfDynamicParams {
                version: current.version,
                listen_ip: current.listen_ip,
                listen_port: current.listen_port,
                port: if prev.port == iperf::default_client_port(prev.version) {
                    current.listen_port
                } else {
                    prev.port
                },
                ..prev
            };
            match isc.dynamic_params.lock() {
                Ok(mut p) => *p = merged,
                Err(poisoned) => {
                    log::warn!("[iperf] dynamic_params 锁中毒，恢复后写入重连参数");
                    *poisoned.into_inner() = merged;
                }
            }
        }
    }

    // 重连前有界 join 旧服务端线程：断开置位 abort 后线程 ≤10s 退出（正常
    // 瞬时返回）；不 join 则新线程与仍持有监听端口的僵尸线程抢端口，
    // 自动启动失败"端口被占用"
    if let Some(prev_sid) = session_id.as_deref() {
        let old_handle = state.session_store.lock().ok().and_then(|store| {
            store
                .get_session(prev_sid)
                .and_then(|h| h.side_channel.as_ref())
                .and_then(|sc| sc.as_any().downcast_ref::<iperf::IperfSideChannel>())
                .map(|isc| isc.server_handle.clone())
        });
        if let Some(handle) = old_handle {
            let joined = tokio::task::spawn_blocking(move || {
                iperf::join_server_handle(&handle, std::time::Duration::from_secs(10))
            })
            .await
            .unwrap_or(false);
            if !joined {
                log::warn!(
                    "[iperf] 重连时旧服务端线程 join 超时，端口可能仍被占用 (session={})",
                    prev_sid
                );
            }
        }
    }

    // 解析后的配置作为会话/事件参数（唯一事实源）：前端 store 播种与右键重连
    // 均以此为准，避免空 params 回落到默认 Iperf2（会话参数恒镜像后端解析结果）
    let resolved_params = {
        let iperf_sc = side_channel
            .as_any()
            .downcast_ref::<iperf::IperfSideChannel>()
            .ok_or_else(|| "侧通道不是 iperf 类型".to_string())?;
        serde_json::to_value(&iperf_sc.config).unwrap_or_else(|_| params.clone())
    };

    // 使用容器会话模式（无 I/O loop — iperf 无终端数据流）
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let sid = store.create_container_session(
            &session_name, "iperf", &endpoint, resolved_params.clone(),
            Some(side_channel.clone()),
            None,  // comm_handle
            false,  // transfer_enabled
            None,   // transfer_protocol
            false,  // send_bar_enabled
            session_id,
        )?;
        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        sid
    };

    log::info!("iperf 会话已创建（容器模式）: {}", sid);

    // 自动启动服务端（对齐 TFTP 语义：连接 = 服务端生命周期开始）。
    // 失败不得静默吞掉——emit 错误状态事件（前端已有该事件处理，展示错误
    // 而非误以为服务端在运行）；成功状态由服务端线程绑定后权威 emit
    //（不再用 200ms 延迟探测——与服务端线程事件重复且 bind 超时会先发
    // 假 running:false 造成"先绿后红"）
    if let Err(e) = iperf::try_start_server(&app, &side_channel, &sid).await {
        log::warn!("[iperf] 服务端自动启动失败 (session={}): {}", sid, e);
        let _ = app.emit("iperf-server-status", serde_json::json!({
            "session_id": sid,
            "running": false,
            "error": e,
        }));
    }

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": sid,
        "plugin_id": "iperf",
        "content_type": "custom",
        "endpoint": endpoint,
        "name": session_name,
        "connection_type": "iperf",
        "params": resolved_params,
        "send_bar_enabled": false,
        "transfer_enabled": false,
    }));

    Ok(sid)
}

/// 启动 iperf 服务端
///
/// 不乐观置 running——真实状态由服务端线程绑定成功后自行 emit
///（iperf2/iperf3 引擎均在线程内发 running:true + listen_addr；
/// 失败时线程侧 emit running=false + error，避免"先绿后红"闪烁）。
#[tauri::command]
pub async fn iperf_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let sc_arc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_side_channel(&session_id)
            .ok_or_else(|| format!("会话 {} 不包含侧通道", session_id))?
    };

    iperf::try_start_server(&app, &sc_arc, &session_id).await?;

    Ok(())
}

/// 停止 iperf 服务端
#[tauri::command]
pub async fn iperf_server_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 全局锁只用于取 Arc（对齐 disconnect_session 先例：emit 期间不得持有
    // session_store 锁，webview 卡顿时会阻塞全部会话命令）
    let sc_arc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_side_channel(&session_id)
            .ok_or_else(|| format!("会话 {} 不包含侧通道", session_id))?
    };

    let iperf_sc = sc_arc.as_any()
        .downcast_ref::<iperf::IperfSideChannel>()
        .ok_or_else(|| "侧通道不是 iperf 类型".to_string())?;

    // 与 try_start_server 互斥：Stop 不会落在 start 的 join/复位窗口内被
    // 覆盖（start 先完成则线程循环感知 abort；stop 先完成则 start 入口检查放弃）
    let _lifecycle = iperf_sc.lifecycle.lock().await;

    iperf_sc.server_abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    iperf_sc.server_running.store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit("iperf-server-status", serde_json::json!({
        "session_id": session_id,
        "running": false,
    }));

    Ok(())
}

/// 运行 iperf 客户端测速（瞬态任务）
///
/// 配置 → 运行 → 实时出结果 → 结束。不建立常驻连接。
/// **fire-and-forget**：invoke 立即返回，进度/结果完全由事件驱动
/// （iperf-test-started → iperf-interval-report × N → iperf-test-done）。
#[tauri::command]
pub async fn iperf_client_run(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    target_host: String,
    params: Value,
) -> Result<(), String> {
    let mut params: IperfDynamicParams = serde_json::from_value(params)
        .map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut params);

    // 客户端自给自足（对齐 TFTP）：侧通道存在时复用其状态（停止按钮可中断）；
    // 会话未连接（无 side_channel）时命令内自建一次性状态，测速照常可用。
    // 注意：客户端中止标志独立于服务端监听标志（client_abort_flag vs
    // server_abort_flag）——客户端测速结束/被停止不得杀死会话内的服务端。
    let (client_abort_flag, client_test_running, last_summary) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        match store.get_side_channel(&session_id) {
            Some(sc_arc) => {
                // 已连接：注册表条目（若有）失效，侧通道状态接管
                if let Ok(mut reg) = IPERF_CLIENT_REGISTRY.lock() {
                    reg.remove(&session_id);
                }
                let iperf_sc = sc_arc.as_any()
                    .downcast_ref::<iperf::IperfSideChannel>()
                    .ok_or_else(|| "侧通道不是 iperf 类型".to_string())?;
                (
                    iperf_sc.client_abort_flag.clone(),
                    iperf_sc.client_test_running.clone(),
                    iperf_sc.last_summary.clone(),
                )
            }
            None => {
                // 注册/复用断连任务条目：运行标志跨 run 存续，重跑守卫据此
                // 生效（此前每次新建标志导致守卫恒 false、两轮并发错配事件）
                if let Ok(mut reg) = IPERF_CLIENT_REGISTRY.lock() {
                    if !reg.contains_key(&session_id) {
                        reg.insert(session_id.clone(), RegisteredClientRun {
                            abort: Arc::new(AtomicBool::new(false)),
                            running: Arc::new(AtomicBool::new(false)),
                        });
                    }
                    let entry = reg.get(&session_id).expect("注册表条目已存在");
                    (
                        entry.abort.clone(),
                        entry.running.clone(),
                        Arc::new(Mutex::new(None)),
                    )
                } else {
                    // 注册表锁中毒等极端情况：退化为一次性独立状态（守卫失效但可用）
                    (
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(Mutex::new(None)),
                    )
                }
            }
        }
    };

    // 重复 run 防护：上一轮测速未结束时先中止并等待其收尾（有界）——client
    // 角色事件无 seq，两轮并发交错发事件时前端无法区分（服务端角色已用 seq 配对）
    if client_test_running.load(Ordering::Relaxed) {
        client_abort_flag.store(true, Ordering::Relaxed);
        log::info!("[iperf] 中止上一轮客户端测速后重跑 (session={})", session_id);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while client_test_running.load(Ordering::Relaxed) && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        if client_test_running.load(Ordering::Relaxed) {
            // 等待超时：上一轮仍未收尾。强行重跑会让两轮无 seq 事件在前端
            // 错配（旧 done 标失败新记录），故拒绝本次 run
            log::warn!(
                "[iperf] 上一轮客户端测速未在 10s 内收尾，拒绝重跑 (session={})",
                session_id
            );
            return Err("上一轮客户端测速仍在收尾，请稍后重试".into());
        }
    }
    // 到达此处时上一轮已收尾（done 已发出，或从未运行）：复位中止标志安全。
    // 同步置位运行标志：闭合"守卫检查与任务置位之间"的 TOCTOU 窗口
    // （此前在 run_iperf_client 内部置位，双击可穿透守卫）
    client_abort_flag.store(false, Ordering::Relaxed);
    client_test_running.store(true, Ordering::Relaxed);

    // 同步动态参数到 side_channel（服务端与客户端共享，含版本与监听参数）；
    // 会话未连接时静默跳过（sync_iperf_params 已容忍）
    sync_iperf_params(&state, &session_id, &params);

    // fire-and-forget：后台任务，invoke 立即返回。
    // run_iperf_client 内部保证 iperf-test-done 一定发出（含 panic 兜底），
    // 并在 done 之后复位运行标志。注册表条目跨 run 存续（运行标志是守卫
    // 的事实源），不在此清理——会话重连时由侧通道分支清除。
    tokio::spawn(async move {
        let result = iperf::client::run_iperf_client(
            app, session_id, target_host, params,
            client_abort_flag, client_test_running, last_summary,
        ).await;
        if let Err(e) = result {
            log::warn!("[iperf] 客户端测速任务失败: {}", e);
        }
    });

    Ok(())
}

/// 参数防御性 clamp：并行流数决定线程/task 数、时长决定 force-end 窗口——
/// 上限防本地资源耗尽（1e9 流会炸线程）与恶意客户端滞留
fn sanitize_iperf_params(params: &mut IperfDynamicParams) {
    params.parallel_streams = params.parallel_streams.clamp(1, 64);
    params.duration_secs = params.duration_secs.clamp(1, 86_400);
    params.report_interval_secs = params.report_interval_secs.clamp(1, 60);
}

/// 同步 iperf 动态参数到 side_channel（客户端测速前调用）。
/// 若 side_channel 不存在（会话未连接），静默跳过。
fn sync_iperf_params(state: &AppState, session_id: &str, params: &IperfDynamicParams) {
    if let Ok(store) = state.session_store.lock() {
        if let Some(sc_arc) = store.get_side_channel(session_id) {
            if let Some(iperf_sc) = sc_arc.as_any().downcast_ref::<iperf::IperfSideChannel>() {
                let mut p = iperf::lock_or_recover(&iperf_sc.dynamic_params, "dynamic_params");
                *p = params.clone();
                log::info!("[iperf] 动态参数已同步 (session={}, duration={}s, port={})",
                    session_id, params.duration_secs, params.port);
            }
        }
    }
}

/// 中止进行中的客户端测速
///
/// 会话已连接时置位侧通道中止标志；会话未连接时查任务注册表
///（`iperf_client_run` 无 side_channel 时注册的一次性任务）。
/// 两者皆无则静默返回——任务已完成或从未启动。
#[tauri::command]
pub async fn iperf_client_stop(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let Some(sc_arc) = store.get_side_channel(&session_id) else {
        // 断开状态：查任务注册表（条目跨 run 存续，中止标志可随时置位）
        if let Ok(reg) = IPERF_CLIENT_REGISTRY.lock() {
            if let Some(entry) = reg.get(&session_id) {
                entry.abort.store(true, Ordering::Relaxed);
                log::info!("[iperf] 已通过任务注册表中止客户端测速 (session={})", session_id);
                return Ok(());
            }
        }
        log::debug!("[iperf] 停止跳过：会话 {} 未连接且无注册任务", session_id);
        return Ok(());
    };

    let iperf_sc = sc_arc.as_any()
        .downcast_ref::<iperf::IperfSideChannel>()
        .ok_or_else(|| "侧通道不是 iperf 类型".to_string())?;

    iperf_sc.client_abort_flag.store(true, Ordering::Relaxed);
    log::info!("[iperf] 已请求中止客户端测速 (session={})", session_id);
    Ok(())
}

/// 更新 iperf 动态参数（服务端与客户端共享）
#[tauri::command]
pub async fn iperf_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let mut new_params: IperfDynamicParams = serde_json::from_value(params)
        .map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut new_params);

    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    if let Some(sc_arc) = store.get_side_channel(&session_id) {
        if let Some(iperf_sc) = sc_arc.as_any().downcast_ref::<iperf::IperfSideChannel>() {
            let mut p = iperf::lock_or_recover(&iperf_sc.dynamic_params, "dynamic_params");
            *p = new_params;
            log::info!("iperf 参数已更新 (session={})", session_id);
        }
    } else {
        log::warn!("iperf 参数更新跳过：会话 {} 未连接（无 side_channel）", session_id);
    }
    Ok(())
}

/// 获取 iperf 状态
///
/// 若会话已连接（side_channel 存在），从侧通道读取实时状态。
/// 若会话未连接，返回默认值。
#[tauri::command]
pub async fn iperf_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IperfStatus, String> {
    // 全局锁只用于取 Arc：dynamic_params/last_summary 在侧通道自有锁下克隆，
    // 长摘要克隆不占用 session_store 锁（其他会话命令无谓排队）
    let sc_arc = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_side_channel(&session_id)
    };

    if let Some(sc_arc) = sc_arc {
        if let Some(iperf_sc) = sc_arc.as_any().downcast_ref::<iperf::IperfSideChannel>() {
            let server_running = iperf_sc.server_running.load(std::sync::atomic::Ordering::Relaxed);
            let test_running = iperf_sc.test_running.load(std::sync::atomic::Ordering::Relaxed);
            let client_test_running = iperf_sc
                .client_test_running
                .load(std::sync::atomic::Ordering::Relaxed);
            // 动态参数为准：版本/监听可在会话内实时修改（config 为创建时不可变快照，
            // 读取它会导致状态报告与用户当前选择不一致）
            let dynamic_params = iperf_sc.get_params();
            let listen_addr = Some(dynamic_params.listen_ip.clone());
            let listen_port = Some(dynamic_params.listen_port);
            let version = dynamic_params.version;
            let last_summary =
                iperf::lock_or_recover(&iperf_sc.last_summary, "last_summary").clone();
            return Ok(IperfStatus {
                server_running,
                test_running,
                client_test_running,
                listen_addr,
                listen_port,
                version,
                dynamic_params,
                last_summary,
            });
        }
    }

    // 会话未连接（无 side_channel），返回默认值
    log::debug!("iperf get_status: 会话 {} 未连接，返回默认状态", session_id);
    Ok(IperfStatus {
        server_running: false,
        test_running: false,
        client_test_running: false,
        listen_addr: None,
        listen_port: None,
        version: iperf::IperfVersion::Iperf2,
        dynamic_params: IperfDynamicParams::default(),
        last_summary: None,
    })
}

