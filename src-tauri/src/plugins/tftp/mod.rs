//! TFTP protocol plugin.
//!
//! The session owns one UDP listener and exposes client/server transfers through a
//! side channel. Defaults are kept for compatibility, while validation and
//! diagnostics are deliberately fail-closed around filesystem and bind errors.

pub mod client;
pub mod counting_socket;
pub mod server;
pub mod transfer;

use std::any::Any;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::channel::error::SessionError;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    ProtocolAdapter, ProtocolConnection, SideChannel, TransferProtocolType,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpConfig {
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    pub file_root: String,
    #[serde(default = "default_true", deserialize_with = "deserialize_bool")]
    pub write_enabled: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_bool")]
    pub overwrite: bool,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub single_port: bool,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub exposure_confirmed: bool,
}

fn default_listen_ip() -> String {
    "0.0.0.0".into()
}
fn default_listen_port() -> u16 {
    69
}
fn default_true() -> bool {
    true
}

/// 容忍持久化/旧数据中误存的非布尔值（如空对象 `{}`、字符串、数字等）——此类字段
/// 一旦被错误序列化，默认 serde 会崩溃于 `invalid type: map, expected a boolean`。
/// 这里凡非字面 `true` 一律视为 `false`。对安全敏感的 `exposure_confirmed` 尤其重要：
/// 损坏/错误的值绝不自动授权暴露写入口——必须显式 `true` 才视为已确认（与
/// `exposure_warning`/connect 里 `!config.exposure_confirmed` 的失败关闭语义一致）。
fn deserialize_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    Ok(v.as_bool().unwrap_or(false))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpDynamicParams {
    #[serde(default = "default_blksize")]
    pub blksize: u16,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u8,
    #[serde(default = "default_windowsize")]
    pub windowsize: u16,
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
    #[serde(default = "default_rollover")]
    pub rollover: TftpRollover,
    #[serde(default)]
    pub window_wait: u64,
    #[serde(default = "default_true")]
    pub clean_on_error: bool,
    #[serde(default = "default_repeat_count")]
    pub repeat_count: u8,
}

fn default_blksize() -> u16 {
    512
}
fn default_timeout_secs() -> u8 {
    5
}
fn default_windowsize() -> u16 {
    1
}
fn default_max_retries() -> usize {
    6
}
fn default_rollover() -> TftpRollover {
    TftpRollover::Enforce0
}
fn default_repeat_count() -> u8 {
    1
}

impl Default for TftpDynamicParams {
    fn default() -> Self {
        Self {
            blksize: default_blksize(),
            timeout_secs: default_timeout_secs(),
            windowsize: default_windowsize(),
            max_retries: default_max_retries(),
            rollover: default_rollover(),
            window_wait: 0,
            clean_on_error: true,
            repeat_count: default_repeat_count(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TftpRollover {
    None,
    Enforce0,
    Enforce1,
    DontCare,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Send,
    Receive,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferRole {
    Client,
    Server,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    Pending,
    Transferring,
    Completed,
    Failed,
    Cancelled,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTransfer {
    pub id: u64,
    pub direction: TransferDirection,
    pub role: TransferRole,
    pub remote_addr: String,
    pub filename: String,
    pub local_path: Option<String>,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub blocks_transferred: u64,
    pub status: TransferStatus,
    pub error: Option<String>,
    pub started_at_ms: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub id: String,
    pub remote_addr: String,
    pub filename: String,
    pub is_write: bool,
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpStatus {
    pub server_running: bool,
    pub listen_addr: Option<String>,
    pub listen_port: Option<u16>,
    pub file_root: String,
    pub dynamic_params: TftpDynamicParams,
}

pub struct TftpSideChannel {
    pub socket: Arc<std::net::UdpSocket>,
    pub config: TftpConfig,
    pub dynamic_params: Arc<Mutex<TftpDynamicParams>>,
    pub server_running: Arc<AtomicBool>,
    pub abort_flag: Arc<AtomicBool>,
    pub next_transfer_id: Arc<AtomicU64>,
    pub active_server_transfers: Arc<AtomicU64>,
}

impl TftpSideChannel {
    pub fn new(socket: Arc<std::net::UdpSocket>, config: TftpConfig) -> Self {
        Self {
            socket,
            config,
            dynamic_params: Arc::new(Mutex::new(TftpDynamicParams::default())),
            server_running: Arc::new(AtomicBool::new(false)),
            abort_flag: Arc::new(AtomicBool::new(false)),
            next_transfer_id: Arc::new(AtomicU64::new(1)),
            active_server_transfers: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn get_params(&self) -> TftpDynamicParams {
        self.dynamic_params.lock().unwrap().clone()
    }
}

impl SideChannel for TftpSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        self.abort_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
        log::info!("[TFTP] server shutdown requested");
    }
}

pub struct TftpAdapter;

impl TftpAdapter {
    pub fn new() -> Self {
        Self
    }
}

/// Returns a warning for configurations that intentionally expose a writable
/// server to non-loopback interfaces. The defaults are not changed; callers can
/// surface this warning before starting the session.
pub fn exposure_warning(config: &TftpConfig) -> Option<&'static str> {
    let ip: IpAddr = config.listen_ip.parse().ok()?;
    if !ip.is_loopback() && config.write_enabled && config.overwrite {
        Some(
            "TFTP is listening on a non-loopback interface with remote writes and overwrite enabled. Use only on trusted networks.",
        )
    } else {
        None
    }
}

fn bind_error(listen_addr: SocketAddr, error: std::io::Error) -> SessionError {
    #[cfg(target_os = "linux")]
    if error.kind() == std::io::ErrorKind::PermissionDenied && listen_addr.port() < 1024 {
        return SessionError::IoError(std::io::Error::new(
            error.kind(),
            format!(
                "cannot bind privileged TFTP port {} as a normal Linux user: {}. Choose a port >= 1024 or grant only the required bind capability; do not run the whole application as root",
                listen_addr.port(), error
            ),
        ));
    }

    SessionError::IoError(std::io::Error::new(
        error.kind(),
        format!("cannot bind TFTP address {}: {}", listen_addr, error),
    ))
}

#[async_trait::async_trait]
impl ProtocolAdapter for TftpAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let mut config: TftpConfig = serde_json::from_value(params.clone()).map_err(|e| {
            // 打印未解析的原始 params：serde 报错（如 "invalid type: map,
            // expected a boolean"）只说明某字段类型不符，不指明是哪个字段/
            // 哪个调用方写成了错误形状（如重连时 tab.params 被动态参数污染）。
            // 输出完整 JSON 到运行日志，便于定位具体污染点。
            log::error!("[TFTP] 连接参数解析失败: {}；原始 params={}", e, params);
            SessionError::ConnectionFailed {
                reason: format!("TFTP configuration parse failed: {}", e),
            }
        })?;

        let root = PathBuf::from(&config.file_root);
        if !root.is_absolute() {
            return Err(SessionError::ConnectionFailed {
                reason: "TFTP file root must be an absolute path".into(),
            });
        }
        let canonical_root = root
            .canonicalize()
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("TFTP file root is unavailable or cannot be resolved: {}", e),
            })?;
        if !canonical_root.is_dir() {
            return Err(SessionError::ConnectionFailed {
                reason: "TFTP file root must be an existing directory".into(),
            });
        }
        config.file_root = canonical_root.to_string_lossy().to_string();

        let listen_ip: IpAddr =
            config
                .listen_ip
                .parse()
                .map_err(|e| SessionError::ConnectionFailed {
                    reason: format!("invalid TFTP listen IP '{}': {}", config.listen_ip, e),
                })?;
        let listen_addr = SocketAddr::new(listen_ip, config.listen_port);

        if exposure_warning(&config).is_some() && !config.exposure_confirmed {
            return Err(SessionError::ConnectionFailed {
                reason: "TFTP is exposed to a non-loopback network with remote writes and overwrite enabled; explicit user confirmation is required".into(),
            });
        }
        if let Some(warning) = exposure_warning(&config) {
            log::warn!("[TFTP] confirmed exposure: {}", warning);
        }

        let socket = std::net::UdpSocket::bind(listen_addr)
            .map_err(|error| bind_error(listen_addr, error))?;

        log::info!("TFTP socket bound to {}", listen_addr);
        let side_channel = Arc::new(TftpSideChannel::new(Arc::new(socket), config));

        Ok(ProtocolConnection {
            channel: None,
            comm_handle: None,
            side_channel: Some(side_channel),
            channel_factory: None,
            teardown_delay: Duration::from_millis(100),
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Async
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![]
    }

    fn teardown_delay(&self) -> Duration {
        Duration::from_millis(100)
    }
}

pub fn apply_oack_window_options(
    options: &[tftpd::TransferOption],
    params: &mut TftpDynamicParams,
) {
    if let Some(ws) = options
        .iter()
        .find(|o| o.option == tftpd::OptionType::WindowSize)
        .map(|o| o.value as u16)
    {
        params.windowsize = ws;
    }
    if let Some(ww) = options
        .iter()
        .find(|o| o.option == tftpd::OptionType::WindowWait)
        .map(|o| o.value)
    {
        params.window_wait = ww;
    }
}

pub fn build_oack_options(
    params: &TftpDynamicParams,
    transfer_size: Option<u64>,
) -> Vec<tftpd::TransferOption> {
    let mut opts = vec![
        tftpd::TransferOption {
            option: tftpd::OptionType::BlockSize,
            value: params.blksize as u64,
        },
        tftpd::TransferOption {
            option: tftpd::OptionType::Timeout,
            value: params.timeout_secs as u64,
        },
        tftpd::TransferOption {
            option: tftpd::OptionType::WindowSize,
            value: params.windowsize as u64,
        },
    ];
    if let Some(ts) = transfer_size {
        opts.push(tftpd::TransferOption {
            option: tftpd::OptionType::TransferSize,
            value: ts,
        });
    }
    if params.window_wait > 0 {
        opts.push(tftpd::TransferOption {
            option: tftpd::OptionType::WindowWait,
            value: params.window_wait,
        });
    }
    opts
}

pub fn try_start_server(
    app: &tauri::AppHandle,
    side_channel: &Arc<dyn crate::kernel::plugin_adapter::SideChannel>,
    session_id: &str,
) -> Result<(), String> {
    let tftp_sc = side_channel
        .as_any()
        .downcast_ref::<TftpSideChannel>()
        .ok_or_else(|| "side channel is not TFTP".to_string())?;

    if tftp_sc
        .server_running
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err("TFTP server is already running".into());
    }

    tftp_sc
        .abort_flag
        .store(false, std::sync::atomic::Ordering::Relaxed);
    server::spawn_tftp_server(server::TftpServerContext {
        app: app.clone(),
        socket: tftp_sc.socket.clone(),
        config: tftp_sc.config.clone(),
        params: tftp_sc.dynamic_params.clone(),
        abort: tftp_sc.abort_flag.clone(),
        server_running: tftp_sc.server_running.clone(),
        next_transfer_id: tftp_sc.next_transfer_id.clone(),
        active_server_transfers: tftp_sc.active_server_transfers.clone(),
        session_id: session_id.to_string(),
    });

    log::info!("[TFTP] server started (session={})", session_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_warning_only_for_writable_non_loopback() {
        let mut config = TftpConfig {
            listen_ip: "0.0.0.0".into(),
            listen_port: 69,
            file_root: "/tmp".into(),
            write_enabled: true,
            overwrite: true,
            single_port: false,
            exposure_confirmed: false,
        };
        assert!(exposure_warning(&config).is_some());
        config.listen_ip = "127.0.0.1".into();
        assert!(exposure_warning(&config).is_none());
    }

    #[test]
    fn ipv6_listen_address_is_constructed_without_string_concatenation() {
        let ip: IpAddr = "::1".parse().unwrap();
        let addr = SocketAddr::new(ip, 69);
        assert_eq!(addr.to_string(), "[::1]:69");
    }
}
