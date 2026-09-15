//! SSH 协议插件（基于 russh async API）
//!
//! 实现 `ProtocolAdapter` trait，提供 SSH 远程终端连接能力。
//! 支持密码和 SSH 密钥两种认证方式。
//! 文件服务（SFTP）通过独立的侧通道操作，不中断终端 I/O 循环。
//! russh Handle 内部线程安全，终端 I/O 与 SFTP 可安全并发。

pub const PLUGIN_ID: &str = "ssh";

pub(crate) mod application;
mod driver;
pub mod handler;
pub mod journald;
mod known_hosts;

use serde::Deserialize;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::Mutex;
use zeroize::Zeroize;

use crate::kernel::plugin_adapter::ContentType;
use crate::kernel::plugin_adapter::{
    ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach,
    SessionChannelFactory, SessionService,
};
use crate::kernel::plugin_runtime::SessionRuntimeRegistry;
use crate::session::SessionError;
use crate::transport::{AsyncBridgeDriver, DataPlaneRuntime};
use driver::SshDriver;
use handler::SshHandler;
use known_hosts::{HostTrustDecision, KnownHostStore};

const SSH_NETWORK_PHASE_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_KEY_VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
const SSH_HOME_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const SSH_HOME_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthMethod {
    #[default]
    Password,
    Key,
}

impl SshAuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Key => "key",
        }
    }
}

/// SSH 连接配置。
///
/// 这是运行时配置，不是持久化模型。认证秘密只在连接期间存在于该对象中，Drop 时会
/// 主动清零；Session Library 只持久化 credential reference。
#[derive(Clone, Deserialize)]
pub struct SshConfig {
    /// 远程主机地址（IP 或域名）
    pub host: String,
    /// SSH 端口（默认 22）
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// 登录用户名
    pub username: String,
    /// 认证方式
    #[serde(default)]
    pub auth_method: SshAuthMethod,
    /// 密码（password 认证时使用）
    pub password: Option<String>,
    /// SSH 私钥内容（key 认证时使用）
    pub private_key: Option<String>,
    /// 私钥密码短语（可选）
    pub passphrase: Option<String>,
    /// 数据模式: "text" | "hex" | "dual"
    #[serde(default = "default_data_mode")]
    pub data_mode: String,
    /// 是否启用文件服务
    #[serde(default)]
    pub file_service_enabled: bool,
    /// 文件服务协议: "sftp"
    #[serde(default = "default_file_service_protocol")]
    pub file_service_protocol: String,
}

impl fmt::Debug for SshConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("auth_method", &self.auth_method)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("data_mode", &self.data_mode)
            .field("file_service_enabled", &self.file_service_enabled)
            .field("file_service_protocol", &self.file_service_protocol)
            .finish()
    }
}

impl Drop for SshConfig {
    fn drop(&mut self) {
        if let Some(password) = self.password.as_mut() {
            password.zeroize();
        }
        if let Some(private_key) = self.private_key.as_mut() {
            private_key.zeroize();
        }
        if let Some(passphrase) = self.passphrase.as_mut() {
            passphrase.zeroize();
        }
    }
}

impl SshConfig {
    fn validate(&self) -> Result<(), SessionError> {
        if normalize_ssh_host(&self.host).is_empty() {
            return Err(SessionError::InvalidParameter(
                "SSH 主机地址不能为空".into(),
            ));
        }
        if self.port == 0 {
            return Err(SessionError::InvalidParameter(
                "SSH 端口必须在 1..=65535 范围内".into(),
            ));
        }
        if self.username.trim().is_empty() {
            return Err(SessionError::InvalidParameter("SSH 用户名不能为空".into()));
        }
        match self.auth_method {
            SshAuthMethod::Password if self.password.as_deref().unwrap_or_default().is_empty() => {
                return Err(SessionError::InvalidParameter(
                    "SSH 密码认证缺少密码凭据".into(),
                ));
            }
            SshAuthMethod::Key
                if self
                    .private_key
                    .as_deref()
                    .is_none_or(|key| key.trim().is_empty()) =>
            {
                return Err(SessionError::InvalidParameter(
                    "SSH 密钥认证缺少私钥凭据".into(),
                ));
            }
            _ => {}
        }
        if self.file_service_enabled && self.file_service_protocol != "sftp" {
            return Err(SessionError::InvalidParameter(format!(
                "不支持的 SSH 文件服务协议: {}",
                self.file_service_protocol
            )));
        }
        Ok(())
    }
}

fn default_ssh_port() -> u16 {
    22
}
fn default_data_mode() -> String {
    "text".into()
}
fn default_file_service_protocol() -> String {
    "sftp".into()
}

fn normalize_ssh_host(host: &str) -> &str {
    let trimmed = host.trim();
    trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed)
}

fn format_ssh_endpoint(host: &str, port: u16) -> String {
    let host = normalize_ssh_host(host);
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn auth_failure_reason(result: &russh::client::AuthResult, method: SshAuthMethod) -> String {
    match result {
        russh::client::AuthResult::Success => "SSH 认证成功".into(),
        russh::client::AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => {
            if *partial_success {
                format!(
                    "{} 认证已部分通过，但服务器要求继续认证；remaining_methods={remaining_methods:?}",
                    method.as_str()
                )
            } else {
                format!(
                    "{} 认证被服务器拒绝；remaining_methods={remaining_methods:?}",
                    method.as_str()
                )
            }
        }
    }
}

/// russh `connect_stream` 会在初始 KEX 完成前启动内部 session task。
/// 仅丢弃 connect future 不能保证该 task 立刻停止，因此保留一个底层 socket duplicate：
/// 连接建立失败、超时或调用 future 被取消时，Drop 主动 shutdown 底层 socket；成功后 disarm。
struct SocketAbortGuard {
    socket: Option<std::net::TcpStream>,
}

impl SocketAbortGuard {
    fn new(socket: std::net::TcpStream) -> Self {
        Self {
            socket: Some(socket),
        }
    }

    fn disarm(&mut self) {
        drop(self.socket.take());
    }
}

impl Drop for SocketAbortGuard {
    fn drop(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
}

async fn connect_ssh_socket(
    host: &str,
    port: u16,
) -> Result<(tokio::net::TcpStream, SocketAbortGuard), SessionError> {
    let endpoint = format_ssh_endpoint(host, port);
    let socket = tokio::time::timeout(
        SSH_NETWORK_PHASE_TIMEOUT,
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| SessionError::ConnectionFailed {
        reason: format!(
            "SSH TCP 连接超时（{}s）: {endpoint}",
            SSH_NETWORK_PHASE_TIMEOUT.as_secs()
        ),
    })?
    .map_err(|error| SessionError::ConnectionFailed {
        reason: format!("SSH TCP 连接失败 '{endpoint}': {error}"),
    })?;

    socket
        .set_nodelay(true)
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("SSH TCP_NODELAY 配置失败 '{endpoint}': {error}"),
        })?;

    let std_socket = socket
        .into_std()
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("SSH TCP socket 转换失败 '{endpoint}': {error}"),
        })?;
    let abort_socket = std_socket
        .try_clone()
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("SSH TCP socket guard 创建失败 '{endpoint}': {error}"),
        })?;
    let socket = tokio::net::TcpStream::from_std(std_socket).map_err(|error| {
        SessionError::ConnectionFailed {
            reason: format!("SSH async socket 恢复失败 '{endpoint}': {error}"),
        }
    })?;

    Ok((socket, SocketAbortGuard::new(abort_socket)))
}

/// SSH 协议适配器
///
/// Adapter 持有 host-key verifier 与非持有型 Session runtime 索引；每次 `connect()` 建立全新的 TCP/SSH 会话。
/// 通过 `connect()` 返回 `ProtocolConnection`，携带：
/// - `channel`: `SshChannel`（终端 I/O，async 路径）
/// - 会话 I/O：由 SessionStore 统一绑定返回的 DataPlaneRuntime 与 SessionIo
/// - `runtime`: `SshRuntime`（供 SFTP 文件服务复用 SSH Handle 和 SFTP 缓存）
pub struct SshAdapter {
    host_key_verifier: HostKeyVerifier,
    runtimes: SessionRuntimeRegistry<SshRuntime>,
}

impl SshAdapter {
    pub fn new() -> Self {
        Self {
            host_key_verifier: HostKeyVerifier::new(),
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn configure_known_hosts(&self, path: std::path::PathBuf) -> Result<(), String> {
        self.host_key_verifier.configure_known_hosts(path)
    }

    pub async fn respond_to_host_key_verification(
        &self,
        request_id: &str,
        accepted: bool,
    ) -> Result<bool, String> {
        self.host_key_verifier.respond(request_id, accepted).await
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<SshRuntime>> {
        self.runtimes.get(session_id)
    }

    /// 使用类型化的 `SshConfig` 直接建立连接（跳过二次 JSON 解析）。
    ///
    /// `connect_session_ssh` 已在前端参数验证阶段反序列化 `SshConfig`，
    /// 此方法复用已有实例，避免 `ProtocolAdapter::connect()` 内部再次解析。
    ///
    /// `app_handle` 和 `verifier` 用于主机密钥用户确认流程：
    /// - 连接时 emit `ssh-host-key-verify` 事件到前端
    /// - 前端调用 `confirm_host_key` 命令回传用户决策
    /// - verifier 在 async 上下文中等待用户响应
    pub async fn connect_with_config(
        &self,
        config: SshConfig,
        app_handle: tauri::AppHandle,
    ) -> Result<ProtocolConnection, SessionError> {
        config.validate()?;
        let result =
            build_connection_with_config(config, app_handle, &self.host_key_verifier).await?;
        let shared = Arc::new(SshRuntime::new(
            result.session,
            result.host_key_fingerprint,
            result.home_dir,
        ));
        let bridge = AsyncBridgeDriver::new(Box::new(result.driver))?;
        let file_transfer = Arc::new(crate::transfer::sftp_transfer::SftpFileTransfer::new(
            shared.session.clone(),
            shared.sftp.clone(),
        ));
        Ok(ProtocolConnection {
            data_plane: Some(DataPlaneRuntime::spawn(Box::new(bridge))),
            service: Some(shared.clone()),
            file_transfer: Some(file_transfer),
            channel_factory: Some(shared.clone()),
            on_attached: Some(Arc::new(RuntimeAttach {
                runtime: shared,
                runtimes: self.runtimes.clone(),
            })),
            teardown_delay: std::time::Duration::ZERO,
        })
    }
}

struct RuntimeAttach {
    runtime: Arc<SshRuntime>,
    runtimes: SessionRuntimeRegistry<SshRuntime>,
}

impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }

    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}

#[derive(Debug)]
pub struct SshRuntime {
    pub session: Arc<Mutex<russh::client::Handle<SshHandler>>>,
    pub sftp: Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    pub host_key_fingerprint: String,
    pub home_dir: String,
}

impl SshRuntime {
    fn new(
        session: Arc<Mutex<russh::client::Handle<SshHandler>>>,
        host_key_fingerprint: String,
        home_dir: String,
    ) -> Self {
        Self {
            session,
            sftp: Arc::new(Mutex::new(None)),
            host_key_fingerprint,
            home_dir,
        }
    }
}

#[async_trait::async_trait]
impl SessionService for SshRuntime {
    async fn shutdown(&self) -> Result<(), SessionError> {
        let mut session = self.session.lock().await;
        let _ = session
            .disconnect(russh::Disconnect::ByApplication, "TauTerm session closed", "en")
            .await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionChannelFactory for SshRuntime {
    async fn open_channel(&self, _mode: ChannelOpenMode) -> Result<DataPlaneRuntime, SessionError> {
        let mut session = self.session.lock().await;
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("SSH 打开新 Channel 失败: {e}"),
            })?;
        channel
            .request_pty(true, "xterm-256color", 120, 40, 0, 0, &[])
            .await
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("SSH 请求 PTY 失败: {e}"),
            })?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("SSH 请求 shell 失败: {e}"),
            })?;
        let driver = SshDriver::new(channel, self.session.clone());
        let bridge = AsyncBridgeDriver::new(Box::new(driver))?;
        Ok(DataPlaneRuntime::spawn(Box::new(bridge)))
    }

    fn child_name_prefix(&self) -> &'static str {
        "Channel"
    }
}

// ... remainder unchanged ...
