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

type BridgeChannel = (
    std::sync::mpsc::SyncSender<Vec<u8>>,
    std::sync::mpsc::Receiver<Vec<u8>>,
);

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
        let data_for_log = data.clone();
        let data_for_bridge = bridge_tx.as_ref().map(|_| data.clone());
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
    let conn = state
        .serial_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|e| e.to_string())?;

    let content_type = state.serial_adapter.content_type();
    let transfer_protocols = state.serial_adapter.transfer_protocols();
    log::info!(
        "串口连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
    );

    let params_clone = params.clone();
    let session_name = name.unwrap_or_default();
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
    let data_mode = params_clone
        .get("data_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();
    let data_mode_for_log = data_mode.clone();
    let encoding_for_log = params_clone
        .get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("utf-8")
        .to_string();

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

            if let Ok(mut store) = app_state.session_store.lock() {
                store.mark_disconnected(&session_id);
            }

            if !pairs.is_empty() {
                if let Ok(mut vpm) = app_state.virtual_port_manager.lock() {
                    for pair in &pairs {
                        let _ = vpm.destroy_endpoint(pair);
                    }
                    let orphan_count = vpm.pending_orphan_count();
                    if orphan_count > 0 {
                        log::warn!(
                            "Session {} disconnected: {} port pair(s) need admin cleanup — deferred to next explicit user action",
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

    let session_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_session(
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
        )?
    };

    let virtual_count = params_clone
        .get("virtual_port_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    let mut vport_endpoints_json: Vec<serde_json::Value> = Vec::new();

    if virtual_enabled && virtual_count > 0 {
        let config = VirtualPortConfig {
            enabled: true,
            count: virtual_count,
        };
        let mut vpm = state
            .virtual_port_manager
            .lock()
            .map_err(|e| e.to_string())?;

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

        vport_endpoints_json = pairs
            .iter()
            .map(|p| serde_json::json!({ "external_path": p.external_path }))
            .collect();

        if !pairs.is_empty() {
            let virtual_port_names: Vec<String> =
                pairs.iter().map(|p| p.bridge_path.clone()).collect();
            let (_bridge_tx, bridge_rx) = bridge
                .take()
                .expect("bridge must be Some when virtual_enabled is true");
            let (write_tx, write_rx) =
                std::sync::mpsc::sync_channel::<Vec<u8>>(BRIDGE_WRITEBACK_CHANNEL_CAPACITY);

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

            let _ = app.emit(
                "virtual-port-created",
                serde_json::json!({
                    "session_id": session_id,
                    "endpoints": &vport_endpoints_json,
                }),
            );
        } else {
            let detail = vport_error.clone().unwrap_or_else(|| {
                "com0com driver not installed. Run TauTerm as administrator once to install the driver."
                    .to_string()
            });
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
            "virtual_endpoints": vport_endpoints_json,
        }),
    );

    Ok(session_id)
}

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
    let content_type = state.telnet_adapter.content_type();
    let transfer_protocols = state.telnet_adapter.transfer_protocols();
    log::info!(
        "Telnet 连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
    );
    connect_simple_terminal_session(app, &state, request, "telnet", "Telnet", true, conn)
}

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
    let channel_for_ch0 = conn
        .data_plane
        .ok_or_else(|| "SSH 连接缺少 DataPlane".to_string())?;

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

    let channel0_id =
        create_terminal_sub_channel(&app, &state, &parent_id, channel_for_ch0, false, false)
            .await
            .inspect_err(|e| {
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

#[tauri::command]
pub async fn confirm_host_key(
    state: tauri::State<'_, AppState>,
    request_id: String,
    accepted: bool,
) -> Result<(), String> {
    let ok = state.host_key_verifier.respond(&request_id, accepted).await?;
    if !ok {
        return Err("主机密钥验证请求未找到或已过期".into());
    }
    log::info!(
        "SSH 主机密钥请求 {}: {}",
        if accepted { "已接受并记住" } else { "已拒绝" },
        &request_id[..request_id.len().min(16)]
    );
    Ok(())
}

#[tauri::command]
pub async fn disconnect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
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
        (pairs, name, is_tftp, is_iperf)
    };

    if !pairs_to_destroy.is_empty() {
        if let Ok(mut vpm) = state.virtual_port_manager.lock() {
            for pair in &pairs_to_destroy {
                let _ = vpm.destroy_endpoint(pair);
            }
            if vpm.pending_orphan_count() > 0 {
                log::info!(
                    "断开连接: {} 个端口对需要管理员权限，通过 UAC 批量清理...",
                    vpm.pending_orphan_count()
                );
                match vpm.cleanup_endpoints_elevated() {
                    Ok(cleaned) => log::info!("断开连接: 通过 UAC 成功清理 {} 个端口对", cleaned),
                    Err(e) => log::warn!(
                        "断开连接: UAC 清理失败: {} — 可通过状态栏[清理残留端口]按钮手动清理",
                        e
                    ),
                }
            }
        }
    }

    log::info!("会话已断开: {} (虚拟端口已清理)", session_name);
    if is_tftp {
        let _ = app.emit(
            "tftp-server-status",
            serde_json::json!({"session_id": session_id, "running": false}),
        );
    }
    if is_iperf {
        let _ = app.emit(
            "iperf-server-status",
            serde_json::json!({"session_id": session_id, "running": false}),
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
        serde_json::json!({"session_id": session_id}),
    );
    Ok(())
}

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
        serde_json::json!({"session_id": session_id, "name": new_name}),
    );
    Ok(())
}

#[tauri::command]
pub fn reorder_tabs(state: State<'_, AppState>, session_ids: Vec<String>) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.reorder_tabs(session_ids)?;
    Ok(())
}

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
    for id in store.tab_ids() {
        if let Some(h) = store.get_session(&id) {
            for sub in &h.sub_connections {
                if sub.state == SessionState::Disconnected || !sub.tabbed {
                    continue;
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

#[tauri::command]
pub fn list_network_peers(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<crate::kernel::session_store::PeerInfo>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    Ok(store.list_peers(&session_id))
}

#[tauri::command]
pub async fn close_network_peer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
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

    let network_runtime = state
        .network_adapter
        .runtime(&sid)
        .ok_or_else(|| "网络调试 runtime 注册失败".to_string())?;
    network_runtime
        .start(app.clone(), &sid)
        .map_err(|e| e.to_string())?;

    let (actual_name, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let handle = store
            .get_session(&sid)
            .ok_or_else(|| "网络调试会话创建失败".to_string())?;
        (handle.name.clone(), handle.connected_at)
    };
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

fn udp_send_impl(
    state: &State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    target: Option<&str>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
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

#[tauri::command]
pub fn network_udp_send(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    transcode: bool,
) -> Result<Vec<u8>, String> {
    udp_send_impl(&state, session_id, data, None, transcode)
}

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

#[tauri::command]
pub async fn load_sessions(app: AppHandle) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionStore::sessions_file_path(&app)?;
    let mut saved = SessionStore::load_from_disk(&path)?;
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

#[tauri::command]
pub async fn start_session_log(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let (session_name, port_name, data_mode) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn log_event(state: State<'_, AppState>, level: String, message: String) -> Result<(), String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    try_send_system_event(&log_engine.sender(), level, message, Local::now());
    Ok(())
}

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
            ("logging.system_enabled", serde_json::json!(previous_enabled)),
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

#[tauri::command]
pub fn get_log_dir(state: State<'_, AppState>) -> Result<String, String> {
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    let config = log_engine.get_config()?;
    Ok(config.log_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_log_config(state: State<'_, AppState>) -> Result<LogConfigResponse, String> {
    if !state.config_store.persistence_ready() {
        return Err("ConfigStore persistence is unavailable".to_string());
    }
    let log_engine = state.log_engine.lock().map_err(|e| e.to_string())?;
    log_engine.get_config_response()
}

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
            ("logging.session_enabled", serde_json::json!(next_session_enabled)),
            ("logging.file_max_size", serde_json::json!(next_file_max_size)),
            ("logging.buffer_size", serde_json::json!(next_buffer_size)),
            ("logging.flush_interval_ms", serde_json::json!(next_flush_interval_ms)),
            ("logging.retention_days", serde_json::json!(next_retention_days)),
        ])
        .map_err(|e| e.to_string())?;

    if let Err(apply_error) = log_engine.update_config(config) {
        let rollback = state.config_store.set_batch(&[
            ("logging.session_enabled", serde_json::json!(current.session_enabled)),
            ("logging.file_max_size", serde_json::json!(current.file_max_size)),
            ("logging.buffer_size", serde_json::json!(current.buffer_size)),
            ("logging.flush_interval_ms", serde_json::json!(current.flush_interval_ms)),
            ("logging.retention_days", serde_json::json!(current.retention_days)),
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

#[tauri::command]
pub fn stop_script_engine(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.stop_script(&session_id)
}

#[tauri::command]
pub fn rules_to_script(
    rules: Vec<crate::kernel::script_engine::codegen::AutoReplyRule>,
    name: String,
    match_strategy: String,
) -> String {
    crate::kernel::script_engine::codegen::rules_to_lua_script(&rules, &name, &match_strategy)
}

#[tauri::command]
pub fn test_match(
    pattern: String,
    mode: String,
    test_data: String,
    case_sensitive: bool,
    match_format: Option<String>,
) -> Result<serde_json::Value, String> {
    let is_hex = match_format.as_deref() == Some("hex");
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
    Ok(serde_json::json!({"valid": true, "matched": matched, "groups": groups}))
}

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
        Ok(serde_json::json!({"valid": true, "matched": matched, "groups": []}))
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
        Ok(serde_json::json!({"valid": true, "matched": matched, "groups": []}))
    }
}

fn test_match_lua_pattern(
    pattern: &str,
    test_data_str: Option<&str>,
) -> Result<serde_json::Value, String> {
    let data_str = test_data_str.unwrap_or("");
    if data_str.is_empty() {
        return Ok(serde_json::json!({"valid": true, "matched": null, "groups": []}));
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
    Ok(serde_json::json!({"valid": true, "matched": Some(matched), "groups": []}))
}

use crate::plugins::ssh::SshRuntime;
use crate::transfer::ssh_file_service::{
    sftp_chmod, sftp_delete, sftp_delete_batch, sftp_delete_recursive, sftp_list_dir, sftp_mkdir,
    sftp_new_file, sftp_read_head, sftp_rename, sftp_stat,
};

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

#[tauri::command]
pub async fn sftp_list_dir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<Vec<crate::transfer::ssh_file_service::SftpEntry>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_list_dir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

#[tauri::command]
pub async fn sftp_stat_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<crate::transfer::ssh_file_service::SftpFileInfo, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_stat(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

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

#[tauri::command]
pub async fn sftp_delete_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

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

#[tauri::command]
pub async fn sftp_mkdir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_mkdir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

#[tauri::command]
pub async fn sftp_new_file_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_new_file(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

#[tauri::command]
pub async fn sftp_delete_batch_cmd(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_batch(&ssh_runtime.session, &ssh_runtime.sftp, &paths).await
}

#[tauri::command]
pub async fn sftp_delete_recursive_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_recursive(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

#[tauri::command]
pub fn get_ssh_home_dir(state: State<'_, AppState>, session_id: String) -> Result<String, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    Ok(ssh_runtime
        .home_dir
        .clone()
        .unwrap_or_else(|| "/".to_string()))
}

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

#[tauri::command]
pub async fn stop_journald_stream(session_id: String) -> Result<(), String> {
    crate::plugins::ssh::journald::stop_journald_stream_confirm(&session_id).await;
    Ok(())
}

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

#[tauri::command]
pub fn stop_journald_export(session_id: String) -> Result<(), String> {
    crate::plugins::ssh::journald::stop_journald_export(&session_id);
    Ok(())
}

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
    let internal_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .resolve_parent_id(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };

    let pt: TransferProtocolType = protocol
        .parse()
        .map_err(|_| format!("无效的传输协议: {}", protocol))?;
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
        session_id,
    )
    .await
}

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
        pt,
        session_id,
        internal_id,
        download_dir,
        remote_paths
            .iter()
            .take(5)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", "),
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
        session_id,
    )
    .await
}

#[tauri::command]
pub fn file_transfer_cancel(
    state: State<'_, AppState>,
    session_id: String,
    transfer_id: Option<String>,
) -> Result<(), String> {
    let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
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

use crate::plugins::tftp::{self, TftpDynamicParams, TftpStatus};

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
    Ok(())
}

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
        serde_json::json!({"session_id": session_id, "running": false}),
    );
    Ok(())
}

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
    sync_tftp_server_params(&state, &session_id, &params);
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
    sync_tftp_server_params(&state, &session_id, &params);
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

use crate::plugins::iperf::{self, IperfDynamicParams, IperfStatus};

#[derive(Clone)]
struct RegisteredClientRun {
    abort: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

static IPERF_CLIENT_REGISTRY: LazyLock<Mutex<HashMap<String, RegisteredClientRun>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
            serde_json::json!({"session_id": sid, "running": false, "error": error}),
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
    let _lifecycle = iperf_sc.lifecycle.lock().await;
    iperf_sc
        .server_abort_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    iperf_sc
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit(
        "iperf-server-status",
        serde_json::json!({"session_id": session_id, "running": false}),
    );
    Ok(())
}

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
                    (
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(Mutex::new(None)),
                    )
                }
            }
        }
    };

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
            log::warn!(
                "[iperf] 上一轮客户端测速未在 10s 内收尾，拒绝重跑 (session={})",
                session_id
            );
            return Err("上一轮客户端测速仍在收尾，请稍后重试".into());
        }
    }
    client_abort_flag.store(false, Ordering::Relaxed);
    client_test_running.store(true, Ordering::Relaxed);
    sync_iperf_params(&state, &session_id, &params);

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

fn sanitize_iperf_params(params: &mut IperfDynamicParams) {
    params.parallel_streams = params.parallel_streams.clamp(1, 64);
    params.duration_secs = params.duration_secs.clamp(1, 86_400);
    params.report_interval_secs = params.report_interval_secs.clamp(1, 60);
}

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

#[tauri::command]
pub async fn iperf_get_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IperfStatus, String> {
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
                serde_json::json!({"password": "protocol-owned-field"}),
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
