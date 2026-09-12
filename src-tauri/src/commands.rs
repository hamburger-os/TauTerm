//! Tauri 命令处理模块
//!
//! 所有面向前端的 Tauri 命令。
//! 通过协议 Adapter + SessionStore + DataPlane/SessionIo 架构管理会话。

pub(crate) mod config;
pub(crate) mod files;
pub(crate) mod platform;

use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::kernel::log_engine::{
    try_send_session_log, try_send_system_event, DataDirection, DataLogEntry, LogConfigResponse,
    LogConfigUpdate, LogEntry, LogHealth, LogStatus,
};
use crate::kernel::plugin_adapter::{ChannelOpenMode, ProtocolAdapter, TransferProtocolType};
use crate::kernel::script_engine::codegen::{hex_to_bytes, interpret_escape_sequences};
use crate::kernel::script_engine::sandbox::create_sandboxed_lua;
use crate::kernel::session_store::{
    ContainerSessionCreateOptions, ContainerSessionRuntime, SessionCreateOptions, SessionState,
    SessionStore,
};
use crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};
use crate::transport::DataPlaneRuntime;
use crate::virtual_port::backend::{
    contains_elevation_indicator, VirtualEndpoint, VirtualPortConfig,
};
use crate::virtual_port::bridge::VirtualPortBridge;
use crate::AppState;
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

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
    pub virtual_port_enabled: bool,
    pub virtual_port_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectSessionRequest {
    pub endpoint: String,
    pub params: Value,
    pub name: Option<String>,
    pub plugin_id: Option<String>,
    pub transfer_enabled: Option<bool>,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: Option<bool>,
    pub journald_enabled: Option<bool>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub initial_elevated: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSessionConfigRequest {
    pub endpoint: String,
    pub params: Value,
    pub name: Option<String>,
    pub plugin_id: Option<String>,
    pub transfer_enabled: Option<bool>,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: Option<bool>,
    pub session_id: Option<String>,
}

const SSH_CREDENTIAL_ACCOUNT_KEY: &str = "credential_account";

fn ssh_credential_account(session_id: &str) -> String {
    format!("ssh-session:{session_id}")
}

fn non_empty_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn ssh_credential_from_params(
    params: &Value,
) -> Result<
    Option<(
        crate::security::credential_store::CredentialType,
        crate::security::credential_store::CredentialValue,
    )>,
    String,
> {
    use crate::security::credential_store::{CredentialType, CredentialValue};

    let auth_method = params
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("password");

    match auth_method {
        "password" => Ok(non_empty_param(params, "password").map(|password| {
            (
                CredentialType::Password,
                CredentialValue::Password(password.to_string()),
            )
        })),
        "key" => Ok(non_empty_param(params, "private_key").map(|private_key| {
            let passphrase = non_empty_param(params, "passphrase").map(str::to_string);
            (
                CredentialType::SshKey,
                CredentialValue::SshKey {
                    private_key: private_key.to_string(),
                    passphrase,
                },
            )
        })),
        other => Err(format!("不支持的 SSH 认证方式: {other}")),
    }
}

fn credential_matches_auth(
    auth_method: &str,
    credential: &crate::security::credential_store::CredentialValue,
) -> bool {
    use crate::security::credential_store::CredentialValue;

    matches!(
        (auth_method, credential),
        ("password", CredentialValue::Password(_)) | ("key", CredentialValue::SshKey { .. })
    )
}

fn strip_ssh_secret_fields(params: &mut Value) -> Result<bool, String> {
    let object = params
        .as_object_mut()
        .ok_or_else(|| "SSH 会话参数必须是 JSON object".to_string())?;
    let mut changed = false;
    changed |= object.remove("password").is_some();
    changed |= object.remove("private_key").is_some();
    changed |= object.remove("passphrase").is_some();
    Ok(changed)
}

fn scrub_ssh_secrets_from_saved_sessions(
    sessions: &mut [crate::kernel::session_store::SavedSession],
) -> Result<bool, String> {
    let mut changed = false;
    for session in sessions {
        if session.plugin_id == "ssh" {
            changed |= strip_ssh_secret_fields(&mut session.params)?;
        }
    }
    Ok(changed)
}

#[derive(Debug)]
struct PendingSshCredential {
    account: String,
    credential_type: crate::security::credential_store::CredentialType,
    value: crate::security::credential_store::CredentialValue,
    description: String,
}

/// Prepare the only supported SSH persistence model without mutating the credential store.
///
/// Plaintext authentication material is accepted only as transient input. Persisted/session params
/// contain only the deterministic credential account reference. A new credential is returned as a
/// pending commit so the Session Library and credential store can be coordinated transactionally.
fn prepare_ssh_session_params(
    state: &AppState,
    session_id: &str,
    params: &mut Value,
) -> Result<Option<PendingSshCredential>, String> {
    use crate::security::credential_store::CredentialStoreError;

    let auth_method = params
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("password")
        .to_string();
    let account = ssh_credential_account(session_id);

    let pending = if let Some((credential_type, value)) = ssh_credential_from_params(params)? {
        let username = params
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let host = params
            .get("host")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Some(PendingSshCredential {
            account: account.clone(),
            credential_type,
            value,
            description: format!("SSH {username}@{host}"),
        })
    } else {
        match state.credential_store.get_credential(&account) {
            Ok(value) if credential_matches_auth(&auth_method, &value) => {}
            Ok(_) => return Err("SSH 认证方式已变更，请重新输入对应凭据".into()),
            Err(CredentialStoreError::NotFound(_)) => {
                return Err("SSH 会话没有可用的安全凭据，请重新输入密码或私钥".into());
            }
            Err(error) => return Err(format!("无法读取 SSH 安全凭据: {error}")),
        }
        None
    };

    let object = params
        .as_object_mut()
        .ok_or_else(|| "SSH 会话参数必须是 JSON object".to_string())?;
    object.insert(
        SSH_CREDENTIAL_ACCOUNT_KEY.to_string(),
        Value::String(account),
    );
    strip_ssh_secret_fields(params)?;
    Ok(pending)
}

fn commit_ssh_credential(state: &AppState, pending: PendingSshCredential) -> Result<(), String> {
    state
        .credential_store
        .store_credential(
            &pending.account,
            pending.credential_type,
            pending.value,
            &pending.description,
        )
        .map_err(|error| format!("无法安全保存 SSH 凭据: {error}"))
}

fn apply_ssh_credential(
    config: &mut crate::plugins::ssh::SshConfig,
    credential: crate::security::credential_store::CredentialValue,
) -> Result<(), String> {
    use crate::security::credential_store::CredentialValue;

    match (config.auth_method.as_str(), credential) {
        ("password", CredentialValue::Password(password)) => {
            config.password = Some(password);
            config.private_key = None;
            config.passphrase = None;
        }
        (
            "key",
            CredentialValue::SshKey {
                private_key,
                passphrase,
            },
        ) => {
            config.password = None;
            config.private_key = Some(private_key);
            config.passphrase = passphrase;
        }
        _ => {
            return Err("SSH 安全凭据类型与当前认证方式不匹配，请重新配置会话".into());
        }
    }
    Ok(())
}

fn hydrate_ssh_config(
    state: &AppState,
    params: &Value,
) -> Result<crate::plugins::ssh::SshConfig, String> {
    let mut config: crate::plugins::ssh::SshConfig =
        serde_json::from_value(params.clone()).map_err(|e| format!("SSH 配置解析失败: {e}"))?;

    let account = params
        .get(SSH_CREDENTIAL_ACCOUNT_KEY)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "SSH 会话缺少安全凭据引用，请重新配置会话".to_string())?;

    let credential = state
        .credential_store
        .get_credential(account)
        .map_err(|error| format!("无法读取 SSH 安全凭据: {error}"))?;

    apply_ssh_credential(&mut config, credential)?;
    Ok(config)
}

fn hydrate_ssh_config_with_pending(
    state: &AppState,
    params: &Value,
    pending: Option<&PendingSshCredential>,
) -> Result<crate::plugins::ssh::SshConfig, String> {
    if let Some(pending) = pending {
        let mut config: crate::plugins::ssh::SshConfig =
            serde_json::from_value(params.clone()).map_err(|e| format!("SSH 配置解析失败: {e}"))?;
        apply_ssh_credential(&mut config, pending.value.clone())?;
        Ok(config)
    } else {
        hydrate_ssh_config(state, params)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournaldQueryRequest {
    pub session_id: String,
    pub level: Option<String>,
    pub keyword: Option<String>,
    pub unit: Option<String>,
    pub kernel_only: Option<bool>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournaldExportRequest {
    pub session_id: String,
    pub file_path: String,
    pub level: Option<String>,
    pub keyword: Option<String>,
    pub unit: Option<String>,
    pub kernel_only: Option<bool>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferSendRequest {
    pub session_id: String,
    pub protocol: String,
    pub file_paths: Vec<String>,
    pub remote_dir: Option<String>,
    pub overwrite_policy: Option<String>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferReceiveRequest {
    pub session_id: String,
    pub protocol: String,
    pub download_dir: String,
    pub remote_paths: Vec<String>,
    pub destination_paths: Option<Vec<String>>,
    pub overwrite_policy: Option<String>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TftpClientRequest {
    pub session_id: String,
    pub remote_ip: String,
    pub remote_port: u16,
    pub remote_filename: String,
    pub local_path: String,
    pub params: Value,
}

// ── 命令：连接类型 ──────────────────────────────────

#[tauri::command]
pub fn get_connection_types(state: State<'_, AppState>) -> Vec<ConnectionTypeInfo> {
    let plugin_host = state.plugin_host.lock().unwrap_or_else(|e| e.into_inner());
    plugin_host
        .plugins()
        .iter()
        .map(|p| ConnectionTypeInfo {
            id: p.id.clone(),
            label: p.name.clone(),
            available: true,
            description: format!("{} v{}", p.name, p.version),
            icon: p.category.clone(),
            content_type: p.content_type.clone(),
        })
        .collect()
}

// ── 命令：端点枚举 ──────────────────────────────────

#[tauri::command]
pub async fn enumerate_endpoints(
    state: State<'_, AppState>,
    plugin_id: Option<String>,
) -> Result<Vec<EndpointItem>, String> {
    let pid = plugin_id.unwrap_or_else(|| "serial".into());
    match pid.as_str() {
        "serial" => {
            // Windows SetupAPI / 第三方串口驱动枚举可能耗时数秒甚至更久。
            // discover_endpoints 是同步 API，必须放到 blocking worker，不能占用
            // Tauri 命令分发线程，否则打开任意会话配置页都会出现 UI 假死。
            let endpoints = tauri::async_runtime::spawn_blocking(|| {
                crate::plugins::serial::SerialAdapter::new().discover_endpoints()
            })
            .await
            .map_err(|e| format!("serial endpoint discovery task failed: {e}"))?
            .map_err(|e| e.to_string())?;
            Ok(endpoints
                .into_iter()
                .map(|ep| EndpointItem {
                    name: ep.name,
                    description: ep.description,
                    connection_type: "serial".to_string(),
                    params: ep.params,
                })
                .collect())
        }
        "ssh" => {
            // 通过适配器调用 discover_endpoints，保持与 serial 一致的插件架构。
            // SSH 当前返回空列表（无硬件端点），但未来可扩展为发现 mDNS/Bonjour SSH 主机等。
            let endpoints = state
                .ssh_adapter
                .discover_endpoints()
                .map_err(|e| e.to_string())?;
            Ok(endpoints
                .into_iter()
                .map(|ep| EndpointItem {
                    name: ep.name,
                    description: ep.description,
                    connection_type: "ssh".to_string(),
                    params: ep.params,
                })
                .collect())
        }
        "telnet" => {
            // 通过适配器调用 discover_endpoints，保持与 serial 一致的插件架构。
            // Telnet 当前返回空列表（无硬件端点），但未来可扩展为发现
            // 已知设备的 Telnet 服务等。
            let endpoints = state
                .telnet_adapter
                .discover_endpoints()
                .map_err(|e| e.to_string())?;
            Ok(endpoints
                .into_iter()
                .map(|ep| EndpointItem {
                    name: ep.name,
                    description: ep.description,
                    connection_type: "telnet".to_string(),
                    params: ep.params,
                })
                .collect())
        }
        "local-shell" => {
            // Shell/WSL 探测会启动平台命令，同样属于不可预测的阻塞 I/O。
            let endpoints = tauri::async_runtime::spawn_blocking(|| {
                crate::plugins::local_shell::LocalShellAdapter::new().discover_endpoints()
            })
            .await
            .map_err(|e| format!("local shell endpoint discovery task failed: {e}"))?
            .map_err(|e| e.to_string())?;
            Ok(endpoints
                .into_iter()
                .map(|ep| EndpointItem {
                    name: ep.name,
                    description: ep.description,
                    connection_type: "local-shell".to_string(),
                    params: ep.params,
                })
                .collect())
        }
        other => Err(format!("插件 '{}' 暂不支持端点枚举", other)),
    }
}

// ── 命令：会话连接 ──────────────────────────────────

/// 连接会话。前端参数收束为请求结构体，保持 IPC 契约清晰。
#[tauri::command]
pub async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let pid = request.plugin_id.clone().unwrap_or_else(|| "serial".into());

    {
        let plugin_host = state.plugin_host.lock().map_err(|e| e.to_string())?;
        if plugin_host.get_plugin(&pid).is_none() {
            return Err(format!("插件 '{}' 未注册", pid));
        }
    }

    match pid.as_str() {
        "serial" => connect_session_serial(app, state, request).await,
        "ssh" => connect_session_ssh(app, state, request).await,
        "tftp" => connect_session_tftp(app, state, request).await,
        "iperf" => connect_session_iperf(app, state, request).await,
        "telnet" => connect_session_telnet(app, state, request).await,
        "local-shell" => connect_session_local_shell(app, state, request).await,
        "network" => connect_session_network(app, state, request).await,
        "trdp" => crate::plugins::trdp::connect_session(app, state, request).await,
        "modbus" => crate::plugins::modbus::connect_session(app, state, request).await,
        other => Err(format!("插件 '{}' 的连接功能尚未实现", other)),
    }
}

/// BridgeChannel = (tx, rx) 类型别名
type BridgeChannel = (
    std::sync::mpsc::SyncSender<Vec<u8>>,
    std::sync::mpsc::Receiver<Vec<u8>>,
);

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
        // 日志和桥接需克隆数据；主路径（batcher）直接获取所有权，省去一次 clone
        let data_for_log = data.clone();
        let data_for_bridge = bridge_tx.as_ref().map(|_| data.clone());
        if let Some(total_dropped) = batcher.push(session_id.clone(), data) {
            // 只在 1 / 2 / 4 / 8 ... 次时通知 UI，避免过载时事件本身形成新的压力。
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
                session_id: session_id.clone(),
                direction: DataDirection::RX,
                data_mode: data_mode.clone(),
                encoding: encoding.clone(),
                payload: data_for_log,
                timestamp: Local::now(),
            },
        );
        if let (Some(tx), Some(d)) = (bridge_tx.as_ref(), data_for_bridge) {
            let _ = tx.try_send(d);
        }
    })
}

/// 串口会话连接（新架构：SerialAdapter → Channel → SessionStore）
async fn connect_session_serial(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    // 通过 SerialAdapter（ProtocolAdapter trait）创建连接产物
    let conn = state
        .serial_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;

    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state.serial_adapter.content_type();
    let transfer_protocols = state.serial_adapter.transfer_protocols();
    log::info!(
        "串口连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
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
        params_clone
            .as_object()
            .map(|o| o.keys().collect::<Vec<_>>())
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
    // 数据推送至脚本引擎由 SessionDataPlane subscription 统一扇出
    let on_data = create_on_data_callback(
        &app_data,
        log_tx,
        data_mode.clone(),
        encoding_for_log,
        bridge_tx,
    );

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> =
        Box::new(move |session_id, info| {
            let app_state: State<'_, AppState> = app_disconnect.state();

            // 1. 在 mark_disconnected 之前读取虚拟端口对
            //    （mark_disconnected 内部关闭桥接线程，但不销毁 pairs）
            let pairs: Vec<VirtualEndpoint> = {
                let store = match app_state.session_store.lock() {
                    Ok(s) => s,
                    Err(e) => e.into_inner(),
                };
                store
                    .get_session(&session_id)
                    .map(|h| h.virtual_endpoints.clone())
                    .unwrap_or_default()
            };

            // 2. 标记运行态断开 — Saved Session Library 由显式配置命令独立持久化
            if let Ok(mut store) = app_state.session_store.lock() {
                store.mark_disconnected(&session_id);
            }

            // 3. 从内核驱动删除端口对 → 外部工具感知 COM 端口消失
            if !pairs.is_empty() {
                if let Ok(mut vpm) = app_state.virtual_port_manager.lock() {
                    for pair in &pairs {
                        let _ = vpm.destroy_endpoint(pair);
                    }

                    // 检查是否有因权限不足而写入 state 文件的残留端口
                    // UAC 弹窗推迟到下次用户主动操作（状态栏 [清理残留端口] 按钮或
                    // 下次连接的 create_endpoints_elevated），避免在断开回调中突然弹窗
                    let orphan_count = vpm.pending_orphan_count();
                    if orphan_count > 0 {
                        log::warn!(
                            "Session {} disconnected: {} port pair(s) need admin cleanup — \
                         deferred to next explicit user action",
                            session_id,
                            orphan_count
                        );
                    }

                    log::info!(
                        "已清理断开会话 {} 的虚拟端口对 ({} 对)",
                        session_id,
                        pairs.len()
                    );
                }
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

    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "ymodem".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    // 在作用域块内创建会话并保存，利用 RAII 自动释放 MutexGuard
    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let session_id = store.create_session(
            SessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "serial".into(),
                endpoint: endpoint.clone(),
                params,
                transfer_enabled: transfer_enabled_val,
                transfer_protocol: Some(transfer_protocol_val.clone()),
                send_bar_enabled: send_bar_enabled_val,
                id_override: session_id,
            },
            conn,
            on_data,
            on_disconnect,
            app.clone(),
        )?;

        // Runtime Session 只消费 Saved Session 配置；连接生命周期不反向覆盖 Library。
        session_id
    };

    // ── 虚拟串口桥接 ──
    // virtual_enabled 已在上面读取，这里只读取 virtual_count
    let virtual_count = params_clone
        .get("virtual_port_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    // vport_endpoints_json declared here so it's in scope for the session-connected emit below
    // (even when virtual ports are disabled)
    let mut vport_endpoints_json: Vec<serde_json::Value> = Vec::new();

    // ── Virtual port pair creation + bridge thread setup ──
    // TODO: Extract into setup_virtual_external_pathridge() helper once the parameter
    // surface stabilizes (currently touches vpm, session_store, app, bridge channel).
    if virtual_enabled && virtual_count > 0 {
        let config = VirtualPortConfig {
            enabled: true,
            count: virtual_count,
        };
        let mut vpm = state
            .virtual_port_manager
            .lock()
            .map_err(|e| e.to_string())?;

        // 记录虚拟端口创建失败的真实原因，用于 `virtual-port-failed` 事件，避免
        // 用一句写死的 "driver not installed" 掩盖真实问题（如端口耗尽、UAC 被取消）。
        let mut vport_error: Option<String> = None;
        let pairs: Vec<VirtualEndpoint> = vpm
            .create_endpoints(&config)
            .or_else(|first_err| {
                log::warn!("直接创建端口对失败: {}；尝试先安装驱动...", first_err);
                vpm.install_driver()
                    .and_then(|_| vpm.create_endpoints(&config))
            })
            .unwrap_or_else(|e| {
                let is_elevation = contains_elevation_indicator(&e);
                if is_elevation && vpm.detect_driver() {
                    log::info!("驱动已安装，尝试通过 UAC 提权创建端口对...");
                    match vpm.create_endpoints_elevated(&config) {
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
        vport_endpoints_json = pairs
            .iter()
            .map(|p| {
                serde_json::json!({
                    "external_path": p.external_path,
                })
            })
            .collect();

        if !pairs.is_empty() {
            let virtual_port_names: Vec<String> =
                pairs.iter().map(|p| p.bridge_path.clone()).collect();
            let (_bridge_tx, bridge_rx) = bridge
                .take()
                .expect("bridge must be Some when virtual_enabled is true");

            // 桥接线程 → 物理端口写线程 channel（容量 128）
            // 使用独立 channel 避免桥接循环内获取 SessionStore Mutex
            let (write_tx, write_rx) =
                std::sync::mpsc::sync_channel::<Vec<u8>>(BRIDGE_WRITEBACK_CHANNEL_CAPACITY);

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
            let vexternal_pathaud_rate = params_clone
                .get("baud_rate")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(115200);

            let vexternal_pathridge = VirtualPortBridge::spawn(
                virtual_port_names,
                vexternal_pathaud_rate,
                bridge_rx,
                write_tx,
            );

            {
                let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
                if let Some(handle) = store.get_session_mut(&session_id) {
                    handle.virtual_external_pathridge = Some(vexternal_pathridge);
                    handle.virtual_endpoints = pairs.clone();
                }
            }

            // 保留独立事件供 reconnect 场景（tab 已存在时更新 VPort 信息）
            let _ = app.emit(
                "virtual-port-created",
                serde_json::json!({
                    "session_id": session_id,
                    "endpoints": &vport_endpoints_json,
                }),
            );
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
            let _ = app.emit(
                "virtual-port-failed",
                serde_json::json!({
                    "session_id": session_id,
                    "kind": kind,
                    "reason": detail,
                }),
            );
        }
    }
    // virtual_enabled=true 时 bridge_rx 被 VirtualPortBridge::spawn() 消费，
    // virtual_enabled=false 时 bridge Option 在此 drop（通道未创建）。
    // bridge_tx 仅在 virtual_enabled=true 时存在，每个 on_data 回调检查并跳过 None 情况。

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_session(&session_id)
            .map(|h| (h.name.clone(), h.params.clone(), h.connected_at))
            .unwrap_or((session_name, params_clone, None))
    };

    log::info!(
        "会话已连接: {} @ {} (data_mode={})",
        actual_name,
        endpoint,
        data_mode_for_log
    );

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
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
            "virtual_endpoints": vport_endpoints_json,
        }),
    );

    Ok(session_id)
}

/// Telnet 会话连接（TelnetAdapter → Channel → SessionStore）
///
/// 单连接/标签页模式（serial 式 Sync I/O），无文件传输、无容器/子会话。
/// 回显状态事件由通道内回调直接 emit（适配器持有 AppHandle，session_id
/// 经 `Channel::on_session_started` 注入），无需 relay 线程。
async fn connect_session_telnet(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let conn = state
        .telnet_adapter
        .connect(&request.endpoint, &request.params)
        .await
        .map_err(|e| e.to_string())?;

    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state.telnet_adapter.content_type();
    let transfer_protocols = state.telnet_adapter.transfer_protocols();
    log::info!(
        "Telnet 连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
    );

    connect_simple_terminal_session(app, &state, request, "telnet", "Telnet", true, conn)
}

/// Local Shell 会话连接（LocalShellAdapter → PTY Channel → SessionStore）。
async fn connect_session_local_shell(
    app: AppHandle,
    state: State<'_, AppState>,
    mut request: ConnectSessionRequest,
) -> Result<String, String> {
    if request
        .name
        .as_deref()
        .is_none_or(|name| name.trim().is_empty())
    {
        request.name = Some(
            crate::plugins::local_shell::LocalShellAdapter::default_session_name(&request.params)?,
        );
    }
    let initial_mode = if request.initial_elevated {
        ChannelOpenMode::Elevated
    } else {
        ChannelOpenMode::Standard
    };
    let mut conn = state
        .local_shell_adapter
        .connect_with_mode(&request.params, initial_mode)
        .await
        .map_err(|e| e.to_string())?;
    let first_channel = conn
        .data_plane
        .take()
        .ok_or("Local Shell 连接缺少 DataPlane")?;
    let factory = conn
        .channel_factory
        .take()
        .ok_or("Local Shell 连接缺少子终端工厂")?;
    let session_name = request.name.clone().unwrap_or_else(|| "Shell".into());
    let parent_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "local-shell".into(),
                endpoint: request.endpoint.clone(),
                params: request.params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: request.send_bar_enabled.unwrap_or(false),
                id_override: request.session_id.clone(),
            },
            ContainerSessionRuntime {
                service: None,
                file_transfer: None,
                channel_factory: Some(factory),
                io: None,
                attachment: None,
                teardown_delay: std::time::Duration::ZERO,
            },
        )?
    };

    let channel_id = create_terminal_sub_channel(
        &app,
        &state,
        &parent_id,
        first_channel,
        initial_mode == ChannelOpenMode::Elevated,
        true,
    )
    .await
    .inspect_err(|error| {
        log::error!("Local Shell 首个子会话创建失败: {error}");
        if let Ok(mut store) = state.session_store.lock() {
            if let Err(cleanup_error) = store.close_session(&parent_id) {
                log::warn!(
                    "Local Shell 首个子会话失败后的父会话清理也失败: {}",
                    cleanup_error
                );
            }
        }
    })?;

    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": parent_id,
            "endpoint": request.endpoint,
            "connection_type": "local-shell",
            "plugin_id": "local-shell",
            "name": session_name,
            "params": request.params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "send_bar_enabled": false,
            "is_container": true,
        }),
    );
    log::info!(
        "Local Shell 父会话已连接: {} (child: {})",
        parent_id,
        channel_id
    );
    Ok(parent_id)
}

/// 简单根终端会话的共享连接流程。
///
/// Serial 的虚拟端口、SSH 与 Local Shell 的多终端容器需要专属 orchestration；
/// 当前由 Telnet 复用这里的日志、SessionStore 和事件语义。
fn connect_simple_terminal_session(
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
    let on_data = create_on_data_callback(&app, log_tx, data_mode, encoding, None);

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
    Ok(session_id)
}

/// SSH 会话连接（新架构：SshAdapter::connect → ProtocolConnection → SessionStore）
async fn connect_session_ssh(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        mut params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        journald_enabled,
        session_id,
        ..
    } = request;

    let effective_session_id = session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let pending_ssh_credential =
        prepare_ssh_session_params(&state, &effective_session_id, &mut params)?;
    let ssh_config =
        hydrate_ssh_config_with_pending(&state, &params, pending_ssh_credential.as_ref())?;

    // 将 journald_enabled 提升为 params 的通用字段（不再耦合 SshConfig）。
    // reconfigure/restore 统一从当前 Session params 读取。
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
    let conn = state
        .ssh_adapter
        .connect_with_config(ssh_config.clone(), app.clone(), &state.host_key_verifier)
        .await
        .map_err(|e| e.to_string())?;

    let content_type = state.ssh_adapter.content_type();
    let transfer_protocols_list = state.ssh_adapter.transfer_protocols();
    log::info!(
        "SSH 连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols_list
    );

    let session_name =
        name.unwrap_or_else(|| format!("{}@{}", ssh_config.username, ssh_config.host));
    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "sftp".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    let teardown_delay = conn.teardown_delay;
    let service = conn.service;
    let file_transfer = conn.file_transfer;
    let channel_factory = conn.channel_factory;
    let attachment = conn.on_attached;
    // 第一个 SSH PTY 已由 transport async bridge 收敛为普通 DataPlaneRuntime。
    let channel_for_ch0 = conn
        .data_plane
        .ok_or_else(|| "SSH 连接缺少 DataPlane".to_string())?;

    // 1. 创建容器父 session（无 I/O loop）
    let parent_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;

        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "ssh".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled_val,
                transfer_protocol: Some(transfer_protocol_val.clone()),
                send_bar_enabled: send_bar_enabled_val,
                id_override: Some(effective_session_id.clone()),
            },
            ContainerSessionRuntime {
                service,
                file_transfer,
                channel_factory,
                io: None,
                attachment,
                teardown_delay,
            },
        )?
    };

    let host_key_fingerprint = state
        .ssh_adapter
        .runtime(&parent_id)
        .and_then(|runtime| runtime.host_key_fingerprint.clone());
    if let Some(ref fp) = host_key_fingerprint {
        log::info!("SSH 主机密钥指纹: {}", fp);
    }

    // 2. 通过共享逻辑创建通道 0（名称由 create_ssh_sub_channel 按 channel_index 自动生成）
    let channel0_id =
        create_terminal_sub_channel(&app, &state, &parent_id, channel_for_ch0, false, false)
            .await
            .inspect_err(|e| {
                // 子通道创建失败 → 回滚清理父容器会话，避免资源泄漏
                log::error!("SSH 通道 0 创建失败，回滚父容器会话 {}: {}", parent_id, e);
                if let Ok(mut store) = state.session_store.lock() {
                    if let Err(cleanup_error) = store.close_session(&parent_id) {
                        log::warn!(
                            "SSH 通道 0 失败后的父会话清理也失败 {}: {}",
                            parent_id,
                            cleanup_error
                        );
                    }
                }
            })?;

    // 3. 在凭据提交前完成全部可失败的运行态校验与事件快照。
    // 这样凭据提交就是连接流程最后一个可失败步骤，不会在“运行态已消失”后留下新凭据。
    let (actual_name, actual_params) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&parent_id)
            .ok_or_else(|| format!("SSH 父会话 {} 已在连接完成前关闭", parent_id))?;
        if handle.state != SessionState::Connected {
            return Err("SSH 父会话已在连接完成前断开".to_string());
        }
        (handle.name.clone(), handle.params.clone())
    };
    let channel0_connected =
        terminal_sub_channel_connected_payload(&state, &parent_id, &channel0_id)?;

    // Persist transient credentials only after the SSH parent and channel 0 are both
    // registered and readable. A credential failure rolls back the newly-created runtime Session.
    if let Some(pending) = pending_ssh_credential {
        if let Err(error) = commit_ssh_credential(&state, pending) {
            if let Ok(mut store) = state.session_store.lock() {
                if let Err(cleanup_error) = store.close_session(&parent_id) {
                    log::warn!(
                        "SSH 凭据提交失败后的父会话清理也失败 {}: {}",
                        parent_id,
                        cleanup_error
                    );
                }
            }
            return Err(error);
        }
    }

    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );

    log::info!(
        "SSH 会话已连接: {} @ {} (parent: {}, channel_0: {})",
        actual_name,
        endpoint,
        parent_id,
        channel0_id
    );

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
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
        }),
    );
    let _ = app.emit("session-connected", channel0_connected);

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
    request_id: String,
    accepted: bool,
) -> Result<(), String> {
    let ok = state
        .host_key_verifier
        .respond(&request_id, accepted)
        .await?;
    if !ok {
        return Err("主机密钥验证请求未找到或已过期".into());
    }
    log::info!(
        "SSH 主机密钥请求 {}: {}",
        if accepted {
            "已接受并记住"
        } else {
            "已拒绝"
        },
        &request_id[..request_id.len().min(16)]
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
    // 单次锁获取：读取 → 关闭（close_session 内部调用 shutdown() 清理侧通道）
    let (pairs_to_destroy, session_name, is_tftp, is_iperf) = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;

        let handle = store
            .get_session(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?;
        let pairs = handle.virtual_endpoints.clone();
        let name = handle.name.clone();
        let is_tftp = handle.plugin_id == "tftp";
        let is_iperf = handle.plugin_id == "iperf";
        store.close_session(&session_id)?;
        store.reset_child_counter(&session_id);
        // Disconnected 属于运行态，不写回 Saved Session Library。
        (pairs, name, is_tftp, is_iperf)
    };
    // 锁已释放 — close_session 内部已关闭桥接

    // 销毁虚拟端口对（从内核驱动移除 → 外部工具感知 COM 端口消失）
    if !pairs_to_destroy.is_empty() {
        if let Ok(mut vpm) = state.virtual_port_manager.lock() {
            for pair in &pairs_to_destroy {
                let _ = vpm.destroy_endpoint(pair);
                // destroy_endpoint 对权限错误返回 Ok(()) 但通过 mark_for_deferred_cleanup
                // 将 bus 号写入 state 文件，后续统一 UAC 清理
            }

            // 检查是否有因权限不足而写入 state 文件的残留端口
            if vpm.pending_orphan_count() > 0 {
                log::info!(
                    "断开连接: {} 个端口对需要管理员权限，通过 UAC 批量清理...",
                    vpm.pending_orphan_count()
                );
                match vpm.cleanup_endpoints_elevated() {
                    Ok(cleaned) => {
                        log::info!("断开连接: 通过 UAC 成功清理 {} 个端口对", cleaned);
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
        let _ = app.emit(
            "tftp-server-status",
            serde_json::json!({
                "session_id": session_id,
                "running": false,
            }),
        );
    }
    // iperf 会话断开 = 服务端生命周期结束（shutdown() 已复位 server_running，
    // 此处显式通知前端刷新右侧面板状态）
    if is_iperf {
        let _ = app.emit(
            "iperf-server-status",
            serde_json::json!({
                "session_id": session_id,
                "running": false,
            }),
        );
    }
    let _ = app.emit(
        "session-disconnected",
        serde_json::json!({
            "session_id": session_id,
            "reason": "User requested disconnect",
            "disconnect_info": DisconnectInfo::user_requested(),
        }),
    );
    Ok(())
}

/// 向指定会话写入数据
///
/// `transcode` 为 true 时（文本发送路径），将前端 UTF-8 字节委托会话
/// `SessionIo::send_text` 按会话编码转码后写设备；false 时（HEX 发送 /
/// 脚本原始字节路径）原样透传。转码策略只存在于 SessionIo（单一知识源），
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
        // 克隆 Arc 后释放锁；send_text 内部完成转码（含 UTF-8 短路与未知编码透传）。
        // 对端（网络调试）拥有各自 SessionIo，文本路径按对端编码转码。
        (encoding, data_mode, store.get_io_for(&session_id))
    };
    let io = comm.ok_or_else(|| format!("会话 {} 没有可写 I/O 能力", session_id))?;
    let data_out = if transcode {
        io.send_text(&data).map_err(|e| e.to_string())?
    } else {
        io.send(&data).map_err(|e| e.to_string())?;
        data
    };
    // 异步发送 TX 数据日志（非阻塞，best-effort：失败不影响主流程）
    // 日志记录实际写入设备的字节（转码后），text 格式按会话编码解码回 UTF-8
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

/// 在父配置上注册一个协议无关的终端子会话。
///
/// 供 [`connect_session_ssh`]（channel-0）和 [`open_channel`]（channel-1+）共用。
/// 所有配置均从父 [`ActiveSessionHandle`] 统一读取，确保所有通道行为完全一致。
/// 通道名称按 `channel_index + 1` 自动生成为 `"Channel N"`。
async fn create_terminal_sub_channel(
    app: &tauri::AppHandle,
    app_state: &AppState,
    parent_id: &str,
    runtime: DataPlaneRuntime,
    elevated: bool,
    announce_connected: bool,
) -> Result<String, String> {
    let (
        endpoint,
        plugin_id,
        params,
        data_mode,
        encoding,
        send_bar_enabled_val,
        file_service_enabled,
        journald_enabled,
        file_service_protocol,
    ) = {
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
            handle
                .params
                .get("file_service_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            handle
                .params
                .get("journald_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            handle
                .params
                .get("file_service_protocol")
                .and_then(Value::as_str)
                .unwrap_or("sftp")
                .to_string(),
        )
    };

    let channel_id = uuid::Uuid::new_v4().to_string();
    let log_tx = app_state
        .log_engine
        .lock()
        .map_err(|e| e.to_string())?
        .sender();
    let on_data = create_on_data_callback(app, log_tx, data_mode, encoding.clone(), None);

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
    let data_plane = SessionDataPlane::attach(runtime, channel_id.clone(), on_data, on_disconnect)
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
                "file_service_enabled": file_service_enabled,
                "file_service_protocol": file_service_protocol,
                "journald_enabled": journald_enabled,
            }),
        );
    }
    Ok(channel_id)
}

fn terminal_sub_channel_connected_payload(
    app_state: &AppState,
    parent_id: &str,
    channel_id: &str,
) -> Result<serde_json::Value, String> {
    let (
        endpoint,
        plugin_id,
        params,
        send_bar_enabled,
        file_service_enabled,
        file_service_protocol,
        journald_enabled,
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
            parent
                .params
                .get("file_service_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            parent
                .params
                .get("file_service_protocol")
                .and_then(Value::as_str)
                .unwrap_or("sftp")
                .to_string(),
            parent
                .params
                .get("journald_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
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
        "file_service_enabled": file_service_enabled,
        "file_service_protocol": file_service_protocol,
        "journald_enabled": journald_enabled,
    }))
}

// ── SSH 多连接命令 ─────────────────────────────────

/// 在已有 SSH 会话上打开新的 PTY channel（不重复 TCP/握手/认证）。
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
    let channel_id = create_terminal_sub_channel(
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

/// 关闭单个子连接（若为最后一个则自动断开父会话）。
#[tauri::command]
pub async fn close_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    parent_id: Option<String>,
    reset_counter: Option<bool>,
) -> Result<(), String> {
    // 两段式关闭：锁内发信号并取出 join 句柄，锁外 join I/O 线程
    // （I/O 线程退出路径可能触发 on_disconnect 回调，回调需获取 store 锁）
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
                // 异常终端现场只驻留在前端内存中；父连接关闭后后端已释放
                // 对应 I/O 资源，此时关闭卡片是幂等的 UI 清理。
                return Ok(());
            }
        };
        let retain_history = store
            .get_session(&pid)
            .map(|handle| {
                handle
                    .sub_connections
                    .iter()
                    .any(|child| child.state == SessionState::Disconnected && child.retain_terminal)
            })
            .unwrap_or(false);
        if last {
            store.close_session(&pid)?;
            if reset_counter.unwrap_or(false) {
                store.reset_child_counter(&pid);
            }
            // 父会话断开只改变运行态，不写回 Saved Session Library。
        }
        (pid, last, retain_history, cleanup)
    };
    // 锁外等待 I/O 线程与脚本线程真实退出
    cleanup.join();

    // 通知前端（仅 session-disconnected；channel-closed 由 on_disconnect 回调单独发出）
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
pub async fn close_network_peer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 两段式：锁内信号 + 移除，锁外 join（同 close_channel）
    let cleanup = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let pid = store
            .find_parent_of_channel(&session_id)
            .ok_or_else(|| format!("对端 {} 未找到", session_id))?;
        let (_is_last, cleanup) = store.close_sub_connection(&pid, &session_id)?;
        cleanup
    };
    tauri::async_runtime::spawn_blocking(move || cleanup.join())
        .await
        .map_err(|error| format!("等待网络对端资源清理失败: {error}"))?;
    Ok(())
}

/// 网络调试会话连接（容器会话 + NetworkRuntime）
#[tauri::command]
pub async fn connect_session_network(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    let conn = state
        .network_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;

    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_session(
            SessionCreateOptions {
                name: name.unwrap_or_else(|| "网络调试".to_string()),
                plugin_id: "network".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled.unwrap_or(false),
                transfer_protocol,
                send_bar_enabled: send_bar_enabled.unwrap_or(true),
                id_override: session_id,
            },
            conn,
            Box::new(|_, _| {}),
            Box::new(|_, _| {}),
            app.clone(),
        )?
    };

    // attach 后从 Network 插件 typed registry 获取 runtime，再启动监听/接收线程。
    let network_runtime = state
        .network_adapter
        .runtime(&sid)
        .ok_or_else(|| "网络调试 runtime 注册失败".to_string())?;
    network_runtime
        .start(app.clone(), &sid)
        .map_err(|e| e.to_string())?;

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
    let udp_local_addr = state
        .network_adapter
        .runtime(&sid)
        .and_then(|net| net.udp_client_local_addr())
        .map(|a| a.to_string());

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
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
        }),
    );
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
    let (encoding, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
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
        (encoding, data_mode)
    };
    let net = state
        .network_adapter
        .runtime(&session_id)
        .ok_or("会话不是网络调试会话".to_string())?;
    // 文本路径：UTF-8 → 会话编码转码（与 write_data 的 SessionIo::send_text 一致）；
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
        try_send_session_log(
            &log_engine.sender(),
            DataLogEntry {
                session_id,
                direction: DataDirection::TX,
                data_mode,
                encoding,
                payload: out.clone(),
                timestamp: Local::now(),
            },
        );
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
    let net = state
        .network_adapter
        .runtime(&session_id)
        .ok_or("会话不是网络调试会话".to_string())?;
    net.set_send_target(target);
    Ok(())
}

// ── 会话持久化命令 ─────────────────────────────────

#[tauri::command]
pub async fn load_sessions(app: AppHandle) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionStore::sessions_file_path(&app)?;
    let mut saved = SessionStore::load_from_disk(&path)?;

    // SSH has one current persistence model only. Any plaintext authentication
    // material found in sessions.json is a stale development artifact: scrub it
    // from disk immediately instead of migrating it. The session card may remain,
    // but reconnect must be explicitly reconfigured if no current credential
    // reference exists.
    if scrub_ssh_secrets_from_saved_sessions(&mut saved)? {
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
            transfer_protocol: s.transfer_protocol.clone(),
            send_bar_enabled: s.send_bar_enabled,
            virtual_port_enabled: s.virtual_port_enabled,
            virtual_port_count: s.virtual_port_count,
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
    let pid = plugin_id.unwrap_or_else(|| "serial".into());
    if pid == "local-shell" {
        crate::plugins::local_shell::LocalShellAdapter::validate_params(&params)?;
    }
    let id = if let Some(ref raw) = session_id {
        if uuid::Uuid::parse_str(raw).is_err() {
            return Err(format!("无效的 session_id 格式: {}", raw));
        }
        raw.clone()
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let pending_ssh_credential = if pid == "ssh" {
        prepare_ssh_session_params(&state, &id, &mut params)?
    } else {
        None
    };

    // TRDP Workspace is edited and persisted by the custom session view rather
    // than the connection form. Reconfiguring a saved/disconnected TRDP
    // session must therefore preserve the latest Workspace even when the form's
    // params snapshot does not contain it.
    if pid == "trdp" && params.get("trdp_workspace").is_none() {
        let active_workspace = state.session_store.lock().ok().and_then(|store| {
            store
                .get_session(&id)
                .and_then(|handle| handle.params.get("trdp_workspace"))
                .cloned()
        });
        let persisted_workspace = if active_workspace.is_some() {
            active_workspace
        } else {
            let path = SessionStore::sessions_file_path(&app)?;
            SessionStore::load_from_disk(&path)?
                .into_iter()
                .find(|saved| saved.id == id)
                .and_then(|saved| saved.params.get("trdp_workspace").cloned())
        };
        if let Some(workspace) = persisted_workspace {
            params
                .as_object_mut()
                .ok_or("TRDP 会话参数必须是 JSON object")?
                .insert("trdp_workspace".to_string(), workspace);
        }
    }
    let session_name = match name.filter(|value| !value.trim().is_empty()) {
        Some(name) => name,
        None if pid == "local-shell" => {
            crate::plugins::local_shell::LocalShellAdapter::default_session_name(&params)?
        }
        None => format!("{} @ {}", pid, endpoint),
    };

    let now = chrono::Utc::now().timestamp_millis() as u64;

    let saved = crate::kernel::session_store::SavedSession {
        id: id.clone(),
        name: session_name,
        plugin_id: pid.clone(),
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

    if pid == "ssh" {
        SessionStore::save_config_to_disk_transactional(&app, saved, || {
            if let Some(pending) = pending_ssh_credential {
                commit_ssh_credential(&state, pending)?;
            }
            Ok(())
        })?;
    } else {
        SessionStore::save_config_to_disk(&app, saved)?;
    }

    Ok(id)
}

#[tauri::command]
pub fn resolve_local_shell_session_name(params: Value) -> Result<String, String> {
    crate::plugins::local_shell::LocalShellAdapter::default_session_name(&params)
}

/// 删除会话配置（从 sessions.json 中移除指定会话）
#[tauri::command]
pub async fn delete_session_config(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    SessionStore::delete_config_from_disk_transactional(&app, &session_id, |deleted_session| {
        if deleted_session.is_some_and(|saved| saved.plugin_id == "ssh") {
            let account = ssh_credential_account(&session_id);
            state
                .credential_store
                .delete_credential(&account)
                .map_err(|error| {
                    format!(
                        "无法删除 SSH 安全凭据；Session 删除已回滚，请重试: {}",
                        error
                    )
                })?;
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

// ── 命令：SSH 文件服务（SFTP）────────────────────

use crate::plugins::ssh::SshRuntime;
use crate::transfer::ssh_file_service::{
    sftp_chmod, sftp_delete, sftp_delete_batch, sftp_delete_recursive, sftp_list_dir, sftp_mkdir,
    sftp_new_file, sftp_read_head, sftp_rename, sftp_stat,
};

/// 解析父 Session 后从 SSH 插件自己的 typed registry 获取 runtime。
fn get_ssh_runtime(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<std::sync::Arc<SshRuntime>, String> {
    let parent_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let parent_id = store
            .resolve_parent_id(session_id)
            .ok_or_else(|| store.session_not_found(session_id))?;
        if store
            .get_session(&parent_id)
            .is_some_and(|h| h.state == SessionState::Disconnected)
        {
            return Err("会话已断开".to_string());
        }
        parent_id
    };
    state
        .ssh_adapter
        .runtime(&parent_id)
        .ok_or_else(|| format!("会话 {} 不包含 SSH runtime（可能不是 SSH 连接）", parent_id))
}

/// SFTP 列出远程目录
#[tauri::command]
pub async fn sftp_list_dir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<Vec<crate::transfer::ssh_file_service::SftpEntry>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_list_dir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 获取文件信息
#[tauri::command]
pub async fn sftp_stat_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<crate::transfer::ssh_file_service::SftpFileInfo, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_stat(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
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
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    // 后端再次收紧上限，不能依赖 WebView 调用方自律。
    let max_bytes = max_bytes.min(1_048_576);
    let (data, total_size) = sftp_read_head(
        &ssh_runtime.session,
        &ssh_runtime.sftp,
        &remote_path,
        max_bytes,
    )
    .await?;
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
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_chmod(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path, mode).await
}

/// SFTP 删除文件或目录
#[tauri::command]
pub async fn sftp_delete_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 重命名/移动文件或目录
#[tauri::command]
pub async fn sftp_rename_cmd(
    state: State<'_, AppState>,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_rename(
        &ssh_runtime.session,
        &ssh_runtime.sftp,
        &from_path,
        &to_path,
    )
    .await
}

/// SFTP 创建目录
#[tauri::command]
pub async fn sftp_mkdir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_mkdir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 创建空文件
#[tauri::command]
pub async fn sftp_new_file_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_new_file(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 批量删除
#[tauri::command]
pub async fn sftp_delete_batch_cmd(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_batch(&ssh_runtime.session, &ssh_runtime.sftp, &paths).await
}

/// SFTP 递归删除目录（包括子内容）
#[tauri::command]
pub async fn sftp_delete_recursive_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_recursive(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// 获取 SSH 会话的远程用户 home 目录
///
/// 连接建立阶段通过 `echo $HOME` 解析并缓存于 `SshRuntime.home_dir`。
/// 若获取失败或值为 None，回退到 `"/"`。
#[tauri::command]
pub fn get_ssh_home_dir(state: State<'_, AppState>, session_id: String) -> Result<String, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    Ok(ssh_runtime
        .home_dir
        .clone()
        .unwrap_or_else(|| "/".to_string()))
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
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since: None,
        until: None,
    };
    crate::plugins::ssh::journald::start_journald_stream(
        &ssh_runtime.session,
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
pub async fn stop_journald_stream(session_id: String) -> Result<(), String> {
    // 确认式停止：等待后端任务真正退出并释放注册表，
    // 保证返回后前端可立即重新开始（消除"已在运行中"窗口期）
    crate::plugins::ssh::journald::stop_journald_stream_confirm(&session_id).await;
    Ok(())
}

/// 查询 journald 历史日志（单次请求，支持游标分页）
#[tauri::command]
pub async fn journald_query_cmd(
    state: State<'_, AppState>,
    request: JournaldQueryRequest,
) -> Result<crate::plugins::ssh::journald::JournaldQueryResponse, String> {
    let JournaldQueryRequest {
        session_id,
        level,
        keyword,
        unit,
        kernel_only,
        since,
        until,
        cursor,
        limit,
    } = request;
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
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
        &ssh_runtime.session,
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
    request: JournaldExportRequest,
) -> Result<(), String> {
    let JournaldExportRequest {
        session_id,
        file_path,
        level,
        keyword,
        unit,
        kernel_only,
        since,
        until,
    } = request;
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since,
        until,
    };
    crate::plugins::ssh::journald::start_journald_export(
        &ssh_runtime.session,
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
pub fn stop_journald_export(session_id: String) -> Result<(), String> {
    crate::plugins::ssh::journald::stop_journald_export(&session_id);
    Ok(())
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
        protocol,
        file_paths,
        remote_dir,
        overwrite_policy,
        block_size,
        checksum_mode,
        streaming,
    } = request;
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
    if pt.is_serial_inline() && files.iter().any(|file| file.is_dir) {
        return Err("X/Y/ZModem 只支持普通文件；目录上传仅适用于 SFTP".into());
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
/// Inline（串口 X/Y/ZModem）或辅助传输（SSH SFTP）策略。
#[tauri::command]
pub async fn file_transfer_receive(
    app: AppHandle,
    state: State<'_, AppState>,
    request: FileTransferReceiveRequest,
) -> Result<crate::transfer::orchestrator::TransferStartAck, String> {
    let FileTransferReceiveRequest {
        session_id,
        protocol,
        download_dir,
        remote_paths,
        destination_paths,
        overwrite_policy,
        block_size,
        checksum_mode,
        streaming,
    } = request;
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

/// 请求 SSH PTY 窗口大小调整
///
/// 前端终端 resize 时调用，通过 SessionIo 的 terminal-control capability 转发到 DataPlane，
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
    let io = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_io_for(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };
    io.resize_terminal(cols, rows).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════
// TFTP 协议命令
// ═══════════════════════════════════════════════════════════════

use crate::plugins::tftp::{self, TftpDynamicParams, TftpStatus};

/// TFTP 会话连接
///
/// 创建容器会话（无终端 I/O loop），然后自动启动服务端。
/// 侧通道 `TftpRuntime` 持有 UDP socket，由独立线程处理所有传输。
async fn connect_session_tftp(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        session_id,
        ..
    } = request;
    let prev_params = session_id
        .as_deref()
        .and_then(|id| state.tftp_adapter.runtime(id))
        .map(|runtime| runtime.get_params());
    let conn = state
        .tftp_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;
    let session_name = name.unwrap_or_else(|| format!("TFTP :{}", endpoint));
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "tftp".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: false,
                id_override: session_id,
            },
            ContainerSessionRuntime {
                service: conn.service,
                file_transfer: conn.file_transfer,
                channel_factory: conn.channel_factory,
                io: None,
                attachment: conn.on_attached,
                teardown_delay: conn.teardown_delay,
            },
        )?
    };
    let runtime = state
        .tftp_adapter
        .runtime(&sid)
        .ok_or_else(|| "TFTP runtime 注册失败".to_string())?;
    if let Some(prev) = prev_params {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = prev,
            Err(poisoned) => *poisoned.into_inner() = prev,
        }
    }
    if let Err(error) = tftp::try_start_server(&app, &runtime, &sid) {
        log::warn!("[TFTP] 服务端自动启动失败 (session={}): {}", sid, error);
    }
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid, "plugin_id": "tftp", "content_type": "custom",
            "endpoint": endpoint, "name": session_name, "connection_type": "tftp",
            "params": params, "send_bar_enabled": false, "transfer_enabled": false,
        }),
    );
    Ok(sid)
}

/// 启动 TFTP 服务端
#[tauri::command]
pub async fn tftp_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let runtime = state
        .tftp_adapter
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 TFTP runtime", session_id))?;
    tftp::try_start_server(&app, &runtime, &session_id)?;

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
    let tftp_sc = state
        .tftp_adapter
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 TFTP runtime", session_id))?;

    tftp_sc
        .abort_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    tftp_sc
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit(
        "tftp-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": false,
        }),
    );

    Ok(())
}

/// TFTP 客户端 GET（下载）——自给自足，不依赖已连接会话 runtime
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无已连接 runtime）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_get(
    state: State<'_, AppState>,
    app: AppHandle,
    request: TftpClientRequest,
) -> Result<String, String> {
    let TftpClientRequest {
        session_id,
        remote_ip,
        remote_port,
        remote_filename,
        local_path,
        params,
    } = request;
    log::info!(
        "[TFTP Client] GET 请求: session={}, file={}, remote={}:{} → {}",
        session_id,
        remote_filename,
        remote_ip,
        remote_port,
        local_path
    );
    let params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 typed runtime（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_get(
        app,
        tftp::client::TftpClientTransferRequest {
            session_id,
            transfer_id: transfer_id.clone(),
            remote_ip,
            remote_port,
            remote_filename,
            local_path: std::path::PathBuf::from(local_path),
            params,
        },
    )
    .await?;

    Ok(transfer_id)
}

/// TFTP 客户端 PUT（上传）——自给自足，不依赖已连接会话 runtime
///
/// 每次调用生成独立的 UUID 作为 transfer_id，绑定临时 UDP socket 完成传输。
/// 在会话未连接（无已连接 runtime）时也能正常工作。
///
/// 调用前先同步服务端的 dynamic_params，消除前端 500ms 防抖导致的竞态窗口。
#[tauri::command]
pub async fn tftp_client_put(
    state: State<'_, AppState>,
    app: AppHandle,
    request: TftpClientRequest,
) -> Result<String, String> {
    let TftpClientRequest {
        session_id,
        remote_ip,
        remote_port,
        remote_filename,
        local_path,
        params,
    } = request;
    log::info!(
        "[TFTP Client] PUT 请求: session={}, file={}, remote={}:{} ← {}",
        session_id,
        remote_filename,
        remote_ip,
        remote_port,
        local_path
    );
    let params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    // 同步服务端参数：避免前端防抖延迟导致服务端使用旧参数协商
    sync_tftp_server_params(&state, &session_id, &params);

    // 客户端操作自给自足：使用 UUID 生成全局唯一 transfer_id，
    // 不依赖会话的 typed runtime（后者在断连后被释放）
    let transfer_id = uuid::Uuid::new_v4().to_string();

    tftp::client::tftp_client_put(
        app,
        tftp::client::TftpClientTransferRequest {
            session_id,
            transfer_id: transfer_id.clone(),
            remote_ip,
            remote_port,
            remote_filename,
            local_path: std::path::PathBuf::from(local_path),
            params,
        },
    )
    .await?;

    Ok(transfer_id)
}

/// 同步 TFTP 服务端参数到 typed runtime（客户端 GET/PUT 前调用）。
/// 若 runtime 不存在（会话未连接），静默跳过。
fn sync_tftp_server_params(state: &AppState, session_id: &str, params: &TftpDynamicParams) {
    if let Some(runtime) = state.tftp_adapter.runtime(session_id) {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = params.clone(),
            Err(poisoned) => *poisoned.into_inner() = params.clone(),
        }
        log::info!(
            "[TFTP] 服务端参数已同步 (session={}, blksize={})",
            session_id,
            params.blksize
        );
    }
}

/// 更新 TFTP 动态参数
///
/// 若会话已连接（runtime 已注册），更新服务端的共享参数。
/// 若会话未连接，仅记录日志后返回 Ok——客户端操作从前端传参，不依赖此处。
#[tauri::command]
pub async fn tftp_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let new_params: TftpDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;

    if let Some(runtime) = state.tftp_adapter.runtime(&session_id) {
        match runtime.dynamic_params.lock() {
            Ok(mut value) => *value = new_params,
            Err(poisoned) => *poisoned.into_inner() = new_params,
        }
        log::info!("TFTP 参数已更新 (session={})", session_id);
    } else {
        log::warn!("TFTP 参数更新跳过：会话 {} 未连接", session_id);
    }
    Ok(())
}

/// 获取 TFTP 状态
///
/// 若会话已连接（runtime 已注册），从 typed runtime 读取实时状态。
/// 若会话未连接，返回默认值（server_running=false，其余字段为空/默认）。
#[tauri::command]
pub async fn tftp_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<TftpStatus, String> {
    if let Some(runtime) = state.tftp_adapter.runtime(&session_id) {
        let dynamic_params = runtime.get_params();
        return Ok(TftpStatus {
            server_running: runtime
                .server_running
                .load(std::sync::atomic::Ordering::Relaxed),
            listen_addr: Some(runtime.config.listen_ip.clone()),
            listen_port: Some(runtime.config.listen_port),
            file_root: runtime.config.file_root.clone(),
            dynamic_params,
        });
    }
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

use crate::plugins::iperf::{self, IperfDynamicParams, IperfStatus};

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
/// 创建容器会话（无终端 I/O loop）。侧通道 `IperfRuntime` 持有
/// 服务端监听线程句柄与测试状态。
/// 对齐 TFTP：连接即自动启动服务端（配置于 ConnectDialog 表单），
/// 断开自动停止；服务端生命周期跟随会话生命周期。
async fn connect_session_iperf(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        session_id,
        ..
    } = request;
    let old_runtime = session_id
        .as_deref()
        .and_then(|id| state.iperf_adapter.runtime(id));
    let prev_params = old_runtime.as_ref().map(|runtime| runtime.get_params());
    if let Some(runtime) = old_runtime {
        let handle = runtime.server_handle.clone();
        let joined = tokio::task::spawn_blocking(move || {
            iperf::join_server_handle(&handle, std::time::Duration::from_secs(10))
        })
        .await
        .unwrap_or(false);
        if !joined {
            log::warn!("[iperf] 重连时旧服务端线程 join 超时");
        }
    }
    let config: iperf::IperfConfig =
        serde_json::from_value(params.clone()).map_err(|e| format!("iperf 配置解析失败: {}", e))?;
    let resolved_params = serde_json::to_value(&config).unwrap_or_else(|_| params.clone());
    let conn = state
        .iperf_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;
    let session_name = name.unwrap_or_else(|| format!("iperf :{}", endpoint));
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "iperf".into(),
                endpoint: endpoint.clone(),
                params: resolved_params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: false,
                id_override: session_id,
            },
            ContainerSessionRuntime {
                service: conn.service,
                file_transfer: conn.file_transfer,
                channel_factory: conn.channel_factory,
                io: None,
                attachment: conn.on_attached,
                teardown_delay: conn.teardown_delay,
            },
        )?
    };
    let runtime = state
        .iperf_adapter
        .runtime(&sid)
        .ok_or_else(|| "iperf runtime 注册失败".to_string())?;
    if let Some(prev) = prev_params {
        let current = runtime.get_params();
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
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = merged;
    }
    if let Err(error) = iperf::try_start_server(&app, &runtime, &sid).await {
        log::warn!("[iperf] 服务端自动启动失败 (session={}): {}", sid, error);
        let _ = app.emit(
            "iperf-server-status",
            serde_json::json!({
                "session_id": sid, "running": false, "error": error,
            }),
        );
    }
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid, "plugin_id": "iperf", "content_type": "custom",
            "endpoint": endpoint, "name": session_name, "connection_type": "iperf",
            "params": resolved_params, "send_bar_enabled": false, "transfer_enabled": false,
        }),
    );
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
    let runtime = state
        .iperf_adapter
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 iperf runtime", session_id))?;
    iperf::try_start_server(&app, &runtime, &session_id).await?;

    Ok(())
}

/// 停止 iperf 服务端
#[tauri::command]
pub async fn iperf_server_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let iperf_sc = state
        .iperf_adapter
        .runtime(&session_id)
        .ok_or_else(|| format!("会话 {} 不包含 iperf runtime", session_id))?;

    // 与 try_start_server 互斥：Stop 不会落在 start 的 join/复位窗口内被
    // 覆盖（start 先完成则线程循环感知 abort；stop 先完成则 start 入口检查放弃）
    let _lifecycle = iperf_sc.lifecycle.lock().await;

    iperf_sc
        .server_abort_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    iperf_sc
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({
            "session_id": session_id,
            "running": false,
        }),
    );

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
    let mut params: IperfDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut params);

    // 客户端自给自足（对齐 TFTP）：侧通道存在时复用其状态（停止按钮可中断）；
    // 会话未连接（无已连接 runtime）时命令内自建一次性状态，测速照常可用。
    // 注意：客户端中止标志独立于服务端监听标志（client_abort_flag vs
    // server_abort_flag）——客户端测速结束/被停止不得杀死会话内的服务端。
    let (client_abort_flag, client_test_running, last_summary) = {
        match state.iperf_adapter.runtime(&session_id) {
            Some(iperf_sc) => {
                if let Ok(mut reg) = IPERF_CLIENT_REGISTRY.lock() {
                    reg.remove(&session_id);
                }
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
                        reg.insert(
                            session_id.clone(),
                            RegisteredClientRun {
                                abort: Arc::new(AtomicBool::new(false)),
                                running: Arc::new(AtomicBool::new(false)),
                            },
                        );
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
        log::info!(
            "[iperf] 中止上一轮客户端测速后重跑 (session={})",
            session_id
        );
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

    // 同步动态参数到 typed runtime（服务端与客户端共享，含版本与监听参数）；
    // 会话未连接时静默跳过（sync_iperf_params 已容忍）
    sync_iperf_params(&state, &session_id, &params);

    // fire-and-forget：后台任务，invoke 立即返回。
    // run_iperf_client 内部保证 iperf-test-done 一定发出（含 panic 兜底），
    // 并在 done 之后复位运行标志。注册表条目跨 run 存续（运行标志是守卫
    // 的事实源），不在此清理——会话重连时由侧通道分支清除。
    tokio::spawn(async move {
        let result = iperf::client::run_iperf_client(
            app,
            session_id,
            target_host,
            params,
            client_abort_flag,
            client_test_running,
            last_summary,
        )
        .await;
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

/// 同步 iperf 动态参数到 typed runtime（客户端测速前调用）。
/// 若 runtime 不存在（会话未连接），静默跳过。
fn sync_iperf_params(state: &AppState, session_id: &str, params: &IperfDynamicParams) {
    if let Some(runtime) = state.iperf_adapter.runtime(session_id) {
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = params.clone();
        log::info!(
            "[iperf] 动态参数已同步 (session={}, duration={}s, port={})",
            session_id,
            params.duration_secs,
            params.port
        );
    }
}

/// 中止进行中的客户端测速
///
/// 会话已连接时置位侧通道中止标志；会话未连接时查任务注册表
///（`iperf_client_run` 无已连接 runtime 时注册的一次性任务）。
/// 两者皆无则静默返回——任务已完成或从未启动。
#[tauri::command]
pub async fn iperf_client_stop(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    if let Some(runtime) = state.iperf_adapter.runtime(&session_id) {
        runtime.client_abort_flag.store(true, Ordering::Relaxed);
        return Ok(());
    }
    if let Ok(reg) = IPERF_CLIENT_REGISTRY.lock() {
        if let Some(entry) = reg.get(&session_id) {
            entry.abort.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// 更新 iperf 动态参数（服务端与客户端共享）
#[tauri::command]
pub async fn iperf_update_params(
    state: State<'_, AppState>,
    session_id: String,
    params: Value,
) -> Result<(), String> {
    let mut new_params: IperfDynamicParams =
        serde_json::from_value(params).map_err(|e| format!("参数解析失败: {}", e))?;
    sanitize_iperf_params(&mut new_params);
    if let Some(runtime) = state.iperf_adapter.runtime(&session_id) {
        *iperf::lock_or_recover(&runtime.dynamic_params, "dynamic_params") = new_params;
    }
    Ok(())
}

/// 获取 iperf 状态
///
/// 若会话已连接（runtime 已注册），从 typed runtime 读取实时状态。
/// 若会话未连接，返回默认值。
#[tauri::command]
pub async fn iperf_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IperfStatus, String> {
    // 全局锁只用于取 Arc：dynamic_params/last_summary 在侧通道自有锁下克隆，
    // 长摘要克隆不占用 session_store 锁（其他会话命令无谓排队）
    if let Some(iperf_sc) = state.iperf_adapter.runtime(&session_id) {
        let server_running = iperf_sc
            .server_running
            .load(std::sync::atomic::Ordering::Relaxed);
        let test_running = iperf_sc
            .test_running
            .load(std::sync::atomic::Ordering::Relaxed);
        let client_test_running = iperf_sc
            .client_test_running
            .load(std::sync::atomic::Ordering::Relaxed);
        // 动态参数为准：版本/监听可在会话内实时修改（config 为创建时不可变快照，
        // 读取它会导致状态报告与用户当前选择不一致）
        let dynamic_params = iperf_sc.get_params();
        let listen_addr = Some(dynamic_params.listen_ip.clone());
        let listen_port = Some(dynamic_params.listen_port);
        let version = dynamic_params.version;
        let last_summary = iperf::lock_or_recover(&iperf_sc.last_summary, "last_summary").clone();
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

    // 会话未连接（无已连接 runtime），返回默认值
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

#[cfg(test)]
mod command_security_tests {
    use super::*;
    use crate::kernel::session_store::SavedSession;
    use crate::security::credential_store::CredentialValue;

    fn saved_session(plugin_id: &str, params: Value) -> SavedSession {
        SavedSession {
            id: uuid::Uuid::new_v4().to_string(),
            name: "test".into(),
            plugin_id: plugin_id.into(),
            endpoint: "test".into(),
            params,
            timestamp: 0,
            transfer_enabled: false,
            transfer_protocol: None,
            send_bar_enabled: false,
            virtual_port_enabled: false,
            virtual_port_count: 0,
        }
    }

    #[test]
    fn ssh_secret_fields_are_removed_from_persisted_params() {
        let mut params = serde_json::json!({
            "host": "example.invalid",
            "username": "tester",
            "auth_method": "key",
            "password": "should-not-persist",
            "private_key": "private-key-material",
            "passphrase": "secret",
            "credential_account": "ssh-session:test"
        });

        assert!(strip_ssh_secret_fields(&mut params).unwrap());
        assert!(params.get("password").is_none());
        assert!(params.get("private_key").is_none());
        assert!(params.get("passphrase").is_none());
        assert_eq!(params["credential_account"], "ssh-session:test");
        assert!(!strip_ssh_secret_fields(&mut params).unwrap());
    }

    #[test]
    fn saved_session_scrub_does_not_touch_other_protocol_params() {
        let mut sessions = vec![
            saved_session(
                "ssh",
                serde_json::json!({
                    "host": "example.invalid",
                    "auth_method": "password",
                    "password": "secret"
                }),
            ),
            saved_session(
                "serial",
                serde_json::json!({
                    "password": "protocol-owned-field"
                }),
            ),
        ];

        assert!(scrub_ssh_secrets_from_saved_sessions(&mut sessions).unwrap());
        assert!(sessions[0].params.get("password").is_none());
        assert_eq!(
            sessions[1].params.get("password").and_then(Value::as_str),
            Some("protocol-owned-field")
        );
        assert!(!scrub_ssh_secrets_from_saved_sessions(&mut sessions).unwrap());
    }

    #[test]
    fn ssh_credential_type_must_match_auth_method() {
        assert!(credential_matches_auth(
            "password",
            &CredentialValue::Password("secret".into())
        ));
        assert!(credential_matches_auth(
            "key",
            &CredentialValue::SshKey {
                private_key: "key".into(),
                passphrase: None,
            }
        ));
        assert!(!credential_matches_auth(
            "password",
            &CredentialValue::SshKey {
                private_key: "key".into(),
                passphrase: None,
            }
        ));
    }

    #[test]
    fn ssh_session_credential_account_is_stable() {
        assert_eq!(
            ssh_credential_account("00000000-0000-0000-0000-000000000001"),
            "ssh-session:00000000-0000-0000-0000-000000000001"
        );
    }
}
