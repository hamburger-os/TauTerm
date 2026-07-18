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
        "serial" => connect_session_serial(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, session_id).await,
        "ssh" => connect_session_ssh(app, state, endpoint, params, name, transfer_enabled, transfer_protocol, send_bar_enabled, session_id).await,
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
    session_id: Option<String>,
) -> Result<String, String> {
    // 通过 SerialAdapter（ProtocolAdapter trait）创建连接产物
    let conn = state.serial_adapter.connect(&endpoint, &params).await
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
    let log_tx = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };

    // 共享 on_data 回调：DataBatcher + 日志 + 虚拟端口转发
    // 数据推送至脚本引擎由 CommHandle::notify_receive() 统一扇出
    let on_data = create_on_data_callback(
        &app_data, log_tx, data_mode.clone(), bridge_tx,
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

/// SSH 会话连接（新架构：SshAdapter::connect → ProtocolConnection → SessionStore）
#[allow(clippy::too_many_arguments)]
async fn connect_session_ssh(
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
    let params_for_config = params.clone();
    let ssh_config: crate::plugins::ssh::SshConfig = serde_json::from_value(params_for_config)
        .map_err(|e| format!("SSH 配置解析失败: {}", e))?;

    // 通过 SshAdapter::connect_with_config 获取 ProtocolConnection，
    // 复用已解析的 SshConfig 实例，避免 connect() 内部二次 JSON 反序列化。
    // 传入 AppHandle 和 HostKeyVerifier 以启用用户确认主机密钥流程。
    let conn = state.ssh_adapter.connect_with_config(
        ssh_config.clone(),
        app.clone(),
        &state.host_key_verifier,
    ).await.map_err(|e| format!("SSH 连接失败: {}", e))?;

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
    let data_mode = ssh_config.data_mode.clone();

    let app_data = app.clone();
    let log_tx = {
        let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
        log_engine.sender()
    };

    // 共享 on_data 回调：DataBatcher + 日志（SSH 无虚拟端口桥接）
    let on_data = create_on_data_callback(
        &app_data, log_tx, data_mode.clone(), None,
    );

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

    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "sftp".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        // conn 携带 side_channel（Arc<russh::client::Handle<SshHandler>> + SftpSession 缓存），由 SessionStore 保存供 SFTP 复用
        let session_id = store.create_session(
            &session_name, "ssh", &endpoint, params.clone(), conn,
            on_data, on_disconnect, app.clone(),
            transfer_enabled_val,
            Some(transfer_protocol_val.clone()),
            send_bar_enabled_val,
            session_id,
        )?;

        let path = SessionStore::sessions_file_path(&app);
        let _ = store.save_to_disk(&path);
        session_id
    };

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&session_id)
            .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
            .unwrap_or((session_name, params, None))
    };

    log::info!("SSH 会话已连接: {} @ {}", actual_name, endpoint);

    let _ = app.emit("session-connected", serde_json::json!({
        "session_id": session_id,
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
        "host_key_fingerprint": host_key_fingerprint,
    }));

    Ok(session_id)
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
    state: State<'_, AppState>,
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

/// 获取所有标签页信息
#[tauri::command]
pub fn get_tabs(
    state: State<'_, AppState>,
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

// ── 文件传输命令（统一 X/Y/ZModem）────────────────────

/// X/Y/ZModem 发送文件（带协议选择 + YMODEM 可选配置）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn send_files(
    app: AppHandle,
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    session_id: String,
    file_paths: Vec<String>,
    protocol_type: TransferProtocolType,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    // 通过 TransferManager 统一路由传输策略
    let strategy = crate::transfer::manager::TransferManager::select_strategy_by_protocol(&protocol_type);
    if strategy != crate::transfer::manager::TransferStrategy::Inline {
        return Err(format!(
            "协议 {:?} 需要通过侧通道传输（SFTP 路径），请使用 sftp_upload_file_cmd",
            protocol_type
        ));
    }

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
    state: State<'_, AppState>,
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
    state: State<'_, AppState>,
    session_id: String,
    download_dir: String,
    protocol_type: TransferProtocolType,
    block_size: Option<usize>,
    checksum_mode: Option<String>,
    streaming: Option<bool>,
) -> Result<(), String> {
    // 通过 TransferManager 统一路由传输策略
    let strategy = crate::transfer::manager::TransferManager::select_strategy_by_protocol(&protocol_type);
    if strategy != crate::transfer::manager::TransferStrategy::Inline {
        return Err(format!(
            "协议 {:?} 需要通过侧通道传输，接收功能暂不支持 SFTP 自动路径",
            protocol_type
        ));
    }

    let pt = protocol_type.clone();
    handoff_and_spawn_transfer(app, state, session_id, &protocol_type, move |port, app_handle, cancel_rx| {
        transfer_receive(port, app_handle, download_dir, pt, cancel_rx, block_size, checksum_mode, streaming)
    })
}

/// 传输初始化失败时的清理：emit `session-transfer-failed` 事件并重置会话状态。
/// `session-transfer-started` emit 之后、后台线程 spawn 之前的所有错误路径都必须调用此函数。
fn emit_transfer_failed_and_cleanup(
    app: &AppHandle,
    state: &State<'_, AppState>,
    session_id: &str,
) {
    let _ = app.emit("session-transfer-failed", serde_json::json!({
        "session_id": session_id,
        "error": "传输初始化失败：端口类型不兼容",
    }));
    if let Ok(mut store) = state.session_store.lock() {
        if let Some(h) = store.get_session_mut(session_id) {
            h.state = SessionState::Connected;
            h.cancel_transfer_tx = None;
            h.channel_return_tx = None;
        }
    }
}

/// Channel 交接 + 后台线程 — send/receive 共享实现
fn handoff_and_spawn_transfer<F>(
    app: AppHandle,
    state: State<'_, AppState>,
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

    let port_any = match channel.try_handoff() {
        Some(pa) => pa,
        None => {
            emit_transfer_failed_and_cleanup(&app, &state, &session_id);
            return Err("Channel 不支持端口移交".into());
        }
    };
    let boxed_port = match port_any.downcast::<Box<dyn serialport::SerialPort>>() {
        Ok(bp) => bp,
        Err(_raw) => {
            emit_transfer_failed_and_cleanup(&app, &state, &session_id);
            return Err("端口类型转换失败".into());
        }
    };
    let mut port = *boxed_port;
    drop(channel);

    let app_clone = app.clone();
    let sid = session_id.clone();
    std::thread::spawn(move || {
        crate::transfer::io::flush_port_buffer(&mut port);
        let result = transfer_fn(&mut port, app_clone.clone(), cancel_rx);

        let app_state: State<'_, AppState> = app_clone.state();
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
    state: State<'_, AppState>,
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
    sftp_list_dir, sftp_stat, sftp_read_head, sftp_chmod, sftp_download, sftp_upload,
    sftp_delete, sftp_delete_recursive, sftp_download_dir,
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
    let handle = store.get_session(session_id)
        .ok_or_else(|| store.session_not_found(session_id))?;
    let sc = handle.side_channel.as_ref()
        .ok_or_else(|| "此会话不包含 SSH 侧通道（可能不是 SSH 连接）".to_string())?;
    let ssh_sc_ref = sc.as_any().downcast_ref::<SshSideChannel>()
        .ok_or_else(|| "侧通道类型不匹配（期望 SshSideChannel）".to_string())?;
    // 通过克隆内部 Arc 字段构造新的 SshSideChannel，
    // 与 SessionStore 中持有的 Arc<dyn SideChannel> 共享同一 session 和 sftp 缓存。
    Ok(std::sync::Arc::new(SshSideChannel {
        session: ssh_sc_ref.session.clone(),
        sftp: ssh_sc_ref.sftp.clone(),
        host_key_fingerprint: ssh_sc_ref.host_key_fingerprint.clone(),
    }))
}

/// SFTP 传输生命周期 RAII 守卫
///
/// 确保传输 tokio task 在**任何**退出路径（包括 panic）下都清理 `sftp_cancel_flag` 并 emit 完成事件，
/// 避免标志残留导致后续传输被永久拒绝（`sftp_transfer_start` 返回"已有传输进行中"）。
///
/// 使用方式：在传输 tokio task 入口处构造，drop 时自动执行清理。
///
/// 已知限制：传输 task 使用 `tokio::spawn`（非守护），若应用在传输进行中退出，
/// task 可能被强制终止而不执行 Drop 的清理逻辑，远端可能残留半成品文件。
/// Tauri 目前不提供 `on_exit` 钩子来 join 所有传输 task，此为平台限制。
struct SftpTransferGuard {
    app: AppHandle,
    session_id: String,
    direction: &'static str,
    file_name: String,
    /// 标记是否已完成清理（手动 emit 时置 true，避免重复 emit）
    done: bool,
}

impl SftpTransferGuard {
    fn new(app: AppHandle, session_id: String, direction: &'static str, file_name: String) -> Self {
        Self { app, session_id, direction, file_name, done: false }
    }

    /// 手触发完成事件并标记已清理（用于在 task 正常退出时携带传输结果）
    fn finish(&mut self, result: &Result<u64, String>) {
        if self.done {
            return;
        }
        self.done = true;
        if let Ok(mut store) = self.app.state::<AppState>().session_store.lock() {
            store.sftp_transfer_done(&self.session_id);
        }
        let _ = self.app.emit("sftp-transfer-finished", serde_json::json!({
            "session_id": &self.session_id,
            "direction": self.direction,
            "file_name": &self.file_name,
            "result": match result {
                Ok(bytes) => serde_json::json!({"bytes": bytes}),
                Err(e) => serde_json::json!({"error": e}),
            },
        }));
    }
}

impl Drop for SftpTransferGuard {
    fn drop(&mut self) {
        if !self.done {
            // panic 或未正常 finish 的兜底清理。
            // 使用 try_state 而非 state：应用关闭期间 AppState 可能已注销，
            // state() 会 panic，try_state() 返回 Option 安全降级。
            if let Some(app_state) = self.app.try_state::<AppState>() {
                if let Ok(mut store) = app_state.session_store.lock() {
                    store.sftp_transfer_done(&self.session_id);
                }
            }
            let _ = self.app.emit("sftp-transfer-finished", serde_json::json!({
                "session_id": &self.session_id,
                "direction": self.direction,
                "file_name": &self.file_name,
                "result": {"error": "传输 task 异常终止"},
            }));
        }
    }
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

/// SFTP 下载文件（带进度事件，可取消）
///
/// 后台 tokio task 执行传输，立即返回。完成/失败/取消时 emit `sftp-transfer-finished` 事件。
/// 此设计避免阻塞 Tauri 命令，使 `cancel_sftp_transfer` 命令可在传输期间执行。
#[tauri::command]
pub async fn sftp_download_file_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let cancel_flag = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.sftp_transfer_start(&session_id)?
    };

    let file_name = std::path::Path::new(&remote_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| remote_path.clone());
    let sid = session_id.clone();
    let app_clone = app.clone();

    // 启动传输 task 并注册 JoinHandle，确保 close_session 可等待其完成
    let handle = tokio::spawn(async move {
        // RAII 守卫：确保任何退出路径（含 panic）都清理 sftp_cancel_flag 并 emit 完成事件
        let mut guard = SftpTransferGuard::new(app_clone, sid.clone(), "download", file_name.clone());

        let result = sftp_download(
            &ssh_sc.session, &ssh_sc.sftp, &remote_path, &local_path,
            Some(&|done, total| {
                let _ = app.emit("sftp-progress", serde_json::json!({
                    "session_id": &sid,
                    "file_name": &file_name,
                    "direction": "download",
                    "bytes_done": done,
                    "bytes_total": total,
                }));
            }),
            Some(&cancel_flag),
        ).await;

        guard.finish(&result);
    });

    // 注册 JoinHandle 以便 close_session 可等待传输完成
    if let Ok(mut store) = state.session_store.lock() {
        let _ = store.register_sftp_handle(&session_id, handle);
    }

    Ok(())
}

/// SFTP 上传文件（带进度事件，可取消）
///
/// 后台 tokio task 执行传输，立即返回。完成/失败/取消时 emit `sftp-transfer-finished` 事件。
#[tauri::command]
pub async fn sftp_upload_file_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    local_path: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let cancel_flag = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.sftp_transfer_start(&session_id)?
    };

    let file_name = std::path::Path::new(&local_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| local_path.clone());
    let sid = session_id.clone();
    let app_clone = app.clone();

    let handle = tokio::spawn(async move {
        let mut guard = SftpTransferGuard::new(app_clone, sid.clone(), "upload", file_name.clone());

        let result = sftp_upload(
            &ssh_sc.session, &ssh_sc.sftp, &local_path, &remote_path,
            Some(&|done, total| {
                let _ = app.emit("sftp-progress", serde_json::json!({
                    "session_id": &sid,
                    "file_name": &file_name,
                    "direction": "upload",
                    "bytes_done": done,
                    "bytes_total": total,
                }));
            }),
            Some(&cancel_flag),
        ).await;

        // 错误路径（非取消）清理远端半成品文件，避免残留不完整文件
        // 导致下次上传同名文件大小不匹配等问题。
        // 取消路径已在 sftp_upload 内部完成清理。
        if let Err(ref e) = result {
            if !crate::transfer::ssh_file_service::is_cancelled_error(e) {
                crate::transfer::ssh_file_service::cleanup_remote_partial(
                    &ssh_sc.session, &ssh_sc.sftp, &remote_path,
                ).await;
            }
        }

        guard.finish(&result);
    });

    if let Ok(mut store) = state.session_store.lock() {
        let _ = store.register_sftp_handle(&session_id, handle);
    }

    Ok(())
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

/// SFTP 递归下载目录（可取消）
///
/// 后台 tokio task 执行传输，立即返回。完成/失败/取消时 emit `sftp-transfer-finished` 事件。
#[tauri::command]
pub async fn sftp_download_dir_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    remote_dir: String,
    local_dir: String,
) -> Result<(), String> {
    let ssh_sc = get_ssh_side_channel(&state, &session_id)?;
    let cancel_flag = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.sftp_transfer_start(&session_id)?
    };

    let sid = session_id.clone();
    let local_path = std::path::PathBuf::from(&local_dir);
    let app_clone = app.clone();
    let dir_name = std::path::Path::new(&remote_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| remote_dir.clone());

    let handle = tokio::spawn(async move {
        let mut guard = SftpTransferGuard::new(app_clone, sid.clone(), "download", dir_name.clone());

        let result = sftp_download_dir(
            &ssh_sc.session, &ssh_sc.sftp, &remote_dir, &local_path,
            Some(&|cur_file: &str, files_done: u64, files_total: u64| {
                let _ = app.emit("sftp-progress", serde_json::json!({
                    "session_id": &sid,
                    "file_name": cur_file,
                    "direction": "download",
                    "bytes_done": files_done,
                    "bytes_total": files_total,
                }));
            }),
            Some(&cancel_flag),
        ).await;

        guard.finish(&result);
    });

    if let Ok(mut store) = state.session_store.lock() {
        let _ = store.register_sftp_handle(&session_id, handle);
    }

    Ok(())
}

/// 取消当前会话的 SFTP 传输
///
/// 置位取消标志，传输循环在下次块检查时退出并返回错误。
/// 传输命令在退出前会自行清理取消状态。
#[tauri::command]
pub fn cancel_sftp_transfer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.cancel_sftp_transfer(&session_id)
}

/// 请求 SSH PTY 窗口大小调整
///
/// 前端终端 resize 时调用，通过 IoLoopCmd::ResizePty 转发到 I/O 循环线程，
/// 再由 Channel::resize_pty 发送 window_change 请求到远端。
/// 非 SSH 协议（串口等）的 Channel 默认空实现，调用无副作用。
#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    let handle = store.get_session(&session_id)
        .ok_or_else(|| store.session_not_found(&session_id))?;
    handle.write_tx.send(IoLoopCmd::ResizePty { cols, rows })
        .map_err(|e| format!("发送 resize 命令失败: {}", e))
}
