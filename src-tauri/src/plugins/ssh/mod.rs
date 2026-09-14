//! SSH 协议插件（基于 russh async API）
//!
//! 实现 `ProtocolAdapter` trait，提供 SSH 远程终端连接能力。
//! 支持密码和 SSH 密钥两种认证方式。
//! 文件服务（SFTP）通过独立的侧通道操作，不中断终端 I/O 循环。
//! russh Handle 内部线程安全，终端 I/O 与 SFTP 可安全并发。

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
    SessionChannelFactory, SessionService, TransferProtocolType,
};
use crate::session::SessionError;
use crate::transport::{AsyncBridgeDriver, DataPlaneRuntime};
use driver::SshDriver;
use handler::SshHandler;
use known_hosts::{HostTrustDecision, KnownHostStore};

const SSH_NETWORK_PHASE_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_KEY_VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

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

/// SSH 协议适配器
///
/// 无状态结构体——每次 `connect()` 调用建立全新的 TCP 连接和 SSH 会话。
/// 通过 `connect()` 返回 `ProtocolConnection`，携带：
/// - `channel`: `SshChannel`（终端 I/O，async 路径）
/// - 会话 I/O：由 SessionStore 统一绑定返回的 DataPlaneRuntime 与 SessionIo
/// - `runtime`: `SshRuntime`（供 SFTP 文件服务复用 SSH Handle 和 SFTP 缓存）
pub struct SshAdapter;

impl SshAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<SshRuntime>> {
        runtime(session_id)
    }

    /// 使用类型化的 `SshConfig` 直接建立连接（跳过二次 JSON 解析）。
    ///
    /// `connect_session_ssh` 已在前端参数验证阶段反序列化 `SshConfig`，
    /// 此方法复用已有实例，避免 `ProtocolAdapter::connect()` 内部再次解析。
    ///
    /// `app_handle` 和 `verifier` 用于主机密钥用户确认流程：
    /// - 连接时 emit `ssh-host-key-verify` 事件到前端
    /// - 前端调用 `confirm_host_key` 命令回传用户决策
    /// - verifier 在 async 上下文中阻塞等待用户响应
    pub async fn connect_with_config(
        &self,
        config: SshConfig,
        app_handle: tauri::AppHandle,
        verifier: &HostKeyVerifier,
    ) -> Result<ProtocolConnection, SessionError> {
        config.validate()?;
        let result = build_connection_with_config(config, app_handle, verifier).await?;
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
            on_attached: Some(Arc::new(RuntimeAttach { runtime: shared })),
            teardown_delay: self.teardown_delay(),
        })
    }
}

/// 主机密钥验证器
///
/// 管理 SSH 连接过程中待用户确认的主机密钥验证请求。
/// 由 AppState 持有，供 `build_connection_with_config`（写入待确认项）
/// 和 `confirm_host_key` Tauri 命令（读取并回传用户决定）双方并发访问。
///
/// 使用 `tokio::sync::Mutex` 而非 `std::sync::Mutex`，
/// 因为 `build_connection_with_config` 在 async 上下文中持有锁时需 `.await`。
struct PendingHostKeyVerification {
    response: tokio::sync::oneshot::Sender<bool>,
    host: String,
    port: u16,
    fingerprint: String,
}

pub struct HostKeyVerifier {
    pending: std::sync::Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, PendingHostKeyVerification>>,
    >,
    known_hosts: KnownHostStore,
}

impl HostKeyVerifier {
    pub fn new() -> Self {
        Self {
            pending: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            known_hosts: KnownHostStore::new(),
        }
    }

    pub fn configure_known_hosts(&self, path: std::path::PathBuf) -> Result<(), String> {
        self.known_hosts.configure(path)
    }

    fn evaluate(&self, host: &str, port: u16, fingerprint: &str) -> HostTrustDecision {
        self.known_hosts.evaluate(host, port, fingerprint)
    }

    fn touch_known_host(&self, host: &str, port: u16) {
        self.known_hosts.touch(host, port);
    }

    /// 注册一次明确的主机验证请求。request_id 而不是 fingerprint 作为键，
    /// 因此多台使用同一 host key 的设备可安全并发验证。
    pub async fn register(
        &self,
        host: &str,
        port: u16,
        fingerprint: &str,
    ) -> (String, tokio::sync::oneshot::Receiver<bool>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(
            request_id.clone(),
            PendingHostKeyVerification {
                response: tx,
                host: host.to_string(),
                port,
                fingerprint: fingerprint.to_string(),
            },
        );
        (request_id, rx)
    }

    pub async fn cancel(&self, request_id: &str) {
        self.pending.lock().await.remove(request_id);
    }

    /// 用户确认或拒绝 SSH 主机密钥。接受时先持久化 trust，再放行连接；
    /// 如果 known-host 写入失败则 fail-closed。
    pub async fn respond(&self, request_id: &str, accept: bool) -> Result<bool, String> {
        let Some(pending) = self.pending.lock().await.remove(request_id) else {
            return Ok(false);
        };

        if accept {
            if let Err(error) =
                self.known_hosts
                    .trust(&pending.host, pending.port, &pending.fingerprint)
            {
                let _ = pending.response.send(false);
                return Err(error);
            }
        }

        let _ = pending.response.send(accept);
        Ok(true)
    }
}

/// 供 SFTP 文件服务使用的类型化运行时资源。
///
/// 持有 SSH 会话引用和缓存的 SFTP 对象。SessionStore 只持有协议无关生命周期
/// capability；SSH 命令通过插件自己的 typed runtime registry 按 session_id 获取本对象。
///
/// - `session` — russh Handle（内部线程安全，与 SshChannel 共享同一 Arc）
/// - `sftp` — 缓存的 SFTP 子系统通道，避免每次操作重新协商
pub struct SshRuntime {
    /// russh Handle（内部线程安全，与 SshChannel 共享）
    pub session: Arc<russh::client::Handle<SshHandler>>,
    /// 缓存的 SFTP 对象，首次 SFTP 操作时惰性创建。
    /// 使用 `tokio::sync::Mutex` 以便多个 SFTP 命令并发访问时共享缓存。
    pub sftp: Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    /// 主机密钥 SHA256 指纹（首次连接时由 check_server_key 产生）。
    /// 由 connect_session_ssh 通过 session-connected 事件传递到前端。
    pub host_key_fingerprint: Option<String>,
    /// SSH 连接建立时通过 `echo $HOME` 解析的远程用户 home 目录
    pub home_dir: Option<String>,
}

fn runtime_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<SshRuntime>>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<SshRuntime>>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn runtime(session_id: &str) -> Option<Arc<SshRuntime>> {
    runtime_registry().lock().ok()?.get(session_id).cloned()
}

struct RuntimeAttach {
    runtime: Arc<SshRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        if let Ok(mut map) = runtime_registry().lock() {
            map.insert(session_id.to_string(), self.runtime.clone());
        }
    }
    fn on_detached(&self, session_id: &str) {
        // SSH-owned background operations are keyed by the final Session id, so their
        // lifecycle cleanup belongs in the SSH attachment hook rather than SessionStore.
        journald::stop_journald_stream(session_id);
        journald::stop_journald_export(session_id);
        if let Ok(mut map) = runtime_registry().lock() {
            map.remove(session_id);
        }
    }
}

impl SshRuntime {
    pub fn new(
        session: Arc<russh::client::Handle<SshHandler>>,
        host_key_fingerprint: Option<String>,
        home_dir: Option<String>,
    ) -> Self {
        Self {
            session,
            sftp: Arc::new(Mutex::new(None)),
            host_key_fingerprint,
            home_dir,
        }
    }

    /// 获取 russh client Handle（用于 SSH 多连接：复用已有 session 打开新 channel）。
    pub fn handle(&self) -> Arc<russh::client::Handle<SshHandler>> {
        self.session.clone()
    }
}

impl SessionService for SshRuntime {}

#[async_trait::async_trait]
impl SessionChannelFactory for SshRuntime {
    async fn open_channel(&self, mode: ChannelOpenMode) -> Result<DataPlaneRuntime, SessionError> {
        if mode != ChannelOpenMode::Standard {
            return Err(SessionError::CapabilityDenied {
                capability: "elevated_shell".into(),
            });
        }
        let driver = open_pty_shell_channel(self.handle()).await?;
        let bridge = AsyncBridgeDriver::new(Box::new(driver))?;
        Ok(DataPlaneRuntime::spawn(Box::new(bridge)))
    }

    fn child_name_prefix(&self) -> &'static str {
        "Channel"
    }
}

/// 建立连接的产物
struct BuildConnectionResult {
    driver: SshDriver,
    session: Arc<russh::client::Handle<SshHandler>>,
    /// 主机密钥 SHA256 指纹（如 "SHA256:xxxx"），供前端展示/确认
    host_key_fingerprint: Option<String>,
    /// 远程 home 目录，连接建立时通过 `echo $HOME` 解析
    home_dir: Option<String>,
}

/// 建立连接的核心逻辑（async）— 直接接收类型化 `SshConfig`。
///
/// 网络阶段使用独立 deadline；等待用户确认 Host Key 时暂停该 deadline，确认完成后重新
/// 开始一个网络阶段。这样用户交互不会误占 TCP/KEX 的网络超时预算。
async fn build_connection_with_config(
    config: SshConfig,
    app_handle: tauri::AppHandle,
    verifier: &HostKeyVerifier,
) -> Result<BuildConnectionResult, SessionError> {
    let connect_host = normalize_ssh_host(&config.host).to_string();
    let addr = format_ssh_endpoint(&connect_host, config.port);
    let russh_config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        inactivity_timeout: Some(Duration::from_secs(300)),
        nodelay: true,
        ..Default::default()
    });

    // 创建主机密钥验证通道。一次 KEX 最多有一个待确认 server key；后续 re-key 仍由
    // russh Handler 走相同 fail-closed policy。
    let (verifier_tx, mut verifier_rx) =
        tokio::sync::mpsc::channel::<handler::HostKeyVerification>(1);

    let handler = SshHandler::new(verifier_tx);
    let config_clone = russh_config.clone();
    let target_host = connect_host.clone();
    let target_port = config.port;

    // connect() 本身会跨越 check_server_key。不能用一个固定 timeout 包住整个 Future，
    // 否则用户阅读并确认 Host Key 的时间也会被算进网络超时。
    let mut connect_task = tokio::spawn(async move {
        russh::client::connect(config_clone, (target_host.as_str(), target_port), handler).await
    });
    let network_deadline = tokio::time::sleep(SSH_NETWORK_PHASE_TIMEOUT);
    tokio::pin!(network_deadline);
    let mut verifier_open = true;

    // 收集主机密钥指纹（如有）
    let mut host_key_fingerprint: Option<String> = None;

    let mut handle = loop {
        tokio::select! {
            result = &mut connect_task => {
                break result
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("SSH 连接 task 失败: {e}"),
                    })?
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("SSH 连接失败 '{addr}': {e}"),
                    })?;
            }
            _ = &mut network_deadline => {
                connect_task.abort();
                let _ = connect_task.await;
                return Err(SessionError::ConnectionFailed {
                    reason: format!(
                        "SSH 网络阶段超时（{}s）: {addr}",
                        SSH_NETWORK_PHASE_TIMEOUT.as_secs()
                    ),
                });
            }
            verification = verifier_rx.recv(), if verifier_open => {
                let Some(verification) = verification else {
                    verifier_open = false;
                    continue;
                };

                host_key_fingerprint = Some(verification.fingerprint.clone());
                log::info!("SSH 主机密钥指纹: {}", verification.fingerprint);

                let accepted = match verifier.evaluate(
                    &connect_host,
                    config.port,
                    &verification.fingerprint,
                ) {
                    HostTrustDecision::Trusted => {
                        verifier.touch_known_host(&connect_host, config.port);
                        log::info!(
                            "SSH known-host 匹配，自动信任 {}",
                            format_ssh_endpoint(&connect_host, config.port)
                        );
                        true
                    }
                    HostTrustDecision::Unknown => {
                        let (request_id, wait_rx) = verifier
                            .register(&connect_host, config.port, &verification.fingerprint)
                            .await;
                        let emitted = app_handle.emit("ssh-host-key-verify", serde_json::json!({
                            "request_id": request_id,
                            "host": connect_host.as_str(),
                            "port": config.port,
                            "fingerprint": verification.fingerprint,
                        }));
                        if let Err(error) = emitted {
                            verifier.cancel(&request_id).await;
                            log::error!("发送 SSH Host Key 确认事件失败: {error}");
                            false
                        } else {
                            log::info!("等待用户确认新的 SSH 主机密钥...");
                            match tokio::time::timeout(HOST_KEY_VERIFY_TIMEOUT, wait_rx).await {
                                Ok(result) => result.unwrap_or(false),
                                Err(_elapsed) => {
                                    verifier.cancel(&request_id).await;
                                    log::warn!(
                                        "主机密钥验证超时 ({}s)，自动拒绝",
                                        HOST_KEY_VERIFY_TIMEOUT.as_secs()
                                    );
                                    false
                                }
                            }
                        }
                    }
                    HostTrustDecision::Unavailable { reason } => {
                        log::error!(
                            "SSH known-host 存储不可用，拒绝连接 {}: {}",
                            format_ssh_endpoint(&connect_host, config.port),
                            reason
                        );
                        false
                    }
                    HostTrustDecision::Changed {
                        expected_fingerprint,
                    } => {
                        let _ = app_handle.emit("ssh-host-key-changed", serde_json::json!({
                            "host": connect_host.as_str(),
                            "port": config.port,
                            "expected_fingerprint": expected_fingerprint,
                            "actual_fingerprint": verification.fingerprint,
                        }));
                        log::error!(
                            "SSH HOST KEY CHANGED: {}，默认拒绝连接",
                            format_ssh_endpoint(&connect_host, config.port)
                        );
                        false
                    }
                };

                let _ = verification.response.send(accepted);
                if !accepted {
                    connect_task.abort();
                    let _ = connect_task.await;
                    return Err(SessionError::ConnectionFailed {
                        reason: "SSH 主机密钥未受信任、验证不可用或已发生变化".into(),
                    });
                }

                // 用户确认时间不属于网络超时；KEX 在得到答复后重新获得完整网络预算。
                network_deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + SSH_NETWORK_PHASE_TIMEOUT);
            }
        }
    };

    // 2. 认证
    let auth_result = match config.auth_method {
        SshAuthMethod::Password => {
            let password = config.password.as_deref().unwrap_or("");
            handle
                .authenticate_password(&config.username, password)
                .await
                .map_err(|e| SessionError::AuthFailed {
                    reason: format!("密码认证协议失败: {e}"),
                })?
        }
        SshAuthMethod::Key => {
            let private_key_str = config.private_key.as_deref().unwrap_or("");
            let mut key_pair =
                russh::keys::PrivateKey::from_openssh(private_key_str).map_err(|e| {
                    SessionError::AuthFailed {
                        reason: format!("私钥解析失败: {e}"),
                    }
                })?;
            if key_pair.is_encrypted() {
                let pass = config.passphrase.as_deref().unwrap_or("");
                if pass.is_empty() {
                    return Err(SessionError::AuthFailed {
                        reason: "私钥已加密但未提供密码短语".into(),
                    });
                }
                key_pair = key_pair
                    .decrypt(pass)
                    .map_err(|e| SessionError::AuthFailed {
                        reason: format!("私钥解密失败（密码短语错误或密钥损坏）: {e}"),
                    })?;
            }

            // RSA 的签名算法属于协议协商结果，不属于私钥本身。优先使用 server-sig-algs；
            // 对未发送 EXT_INFO 的服务器保持现代 rsa-sha2-512 best-effort，不静默降级到
            // 已废弃的 ssh-rsa/SHA-1。
            let hash_alg = if key_pair.algorithm().is_rsa() {
                match handle.best_supported_rsa_hash().await.map_err(|e| {
                    SessionError::AuthFailed {
                        reason: format!("协商 RSA 签名算法失败: {e}"),
                    }
                })? {
                    Some(Some(hash_alg)) => Some(hash_alg),
                    Some(None) => {
                        return Err(SessionError::AuthFailed {
                            reason: "服务器仅接受旧式 ssh-rsa/SHA-1，TauTerm 默认拒绝不安全降级"
                                .into(),
                        });
                    }
                    None => Some(russh::keys::HashAlg::Sha512),
                }
            } else {
                None
            };

            let key_with_hash =
                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg);
            handle
                .authenticate_publickey(&config.username, key_with_hash)
                .await
                .map_err(|e| SessionError::AuthFailed {
                    reason: format!("密钥认证协议失败: {e}"),
                })?
        }
    };

    if !auth_result.success() {
        return Err(SessionError::AuthFailed {
            reason: auth_failure_reason(&auth_result, config.auth_method),
        });
    }

    // 2.5 — 解析远程 home 目录（通过 exec 通道执行 echo $HOME）
    // 超时 5s 防止远程主机无响应时阻塞整个连接建立流程
    let home_dir = tokio::time::timeout(Duration::from_secs(5), async {
        match handle.channel_open_session().await {
            Ok(mut exec_chan) => {
                if exec_chan.exec(true, "echo $HOME").await.is_err() {
                    return None;
                }
                let mut output = Vec::new();
                loop {
                    match exec_chan.wait().await {
                        Some(russh::ChannelMsg::Data { data }) => {
                            output.extend_from_slice(data.as_ref());
                        }
                        Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                            break
                        }
                        _ => continue,
                    }
                }
                String::from_utf8(output)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            }
            Err(e) => {
                log::warn!("打开 SSH exec 通道失败 (home dir): {e}");
                None
            }
        }
    })
    .await
    .unwrap_or_else(|_elapsed| {
        log::warn!("SSH home_dir exec 超时（5s），回退到默认路径");
        None
    });

    // 3. 打开远端 PTY + shell（共享函数，供 open_channel 复用）
    let handle = Arc::new(handle);
    let ssh_channel = open_pty_shell_channel(handle.clone()).await?;

    Ok(BuildConnectionResult {
        driver: ssh_channel,
        session: handle,
        host_key_fingerprint,
        home_dir,
    })
}

/// 在已有 SSH Handle 上打开新的 PTY + shell 通道。
///
/// 由 `build_connection_with_config`（首次连接）和 `open_channel`
/// Tauri 命令（SSH 多连接）共用。跳过了 TCP 连接、密钥验证和认证步骤。
pub async fn open_pty_shell_channel(
    handle: Arc<russh::client::Handle<SshHandler>>,
) -> Result<SshDriver, SessionError> {
    // 1. 打开交互式 shell 通道
    let channel =
        handle
            .channel_open_session()
            .await
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("打开 SSH 通道失败: {e}"),
            })?;

    // 2. 请求 PTY（终端）
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("请求 PTY 失败: {e}"),
        })?;

    // 3. 启动 shell
    channel
        .request_shell(true)
        .await
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("启动 shell 失败: {e}"),
        })?;

    Ok(SshDriver::new(channel, handle))
}

#[async_trait::async_trait]
impl ProtocolAdapter for SshAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        _params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        // SSH 必须使用 connect_with_config()，因为生产连接需要 AppHandle +
        // HostKeyVerifier 才能执行持久 known-host 信任策略。通用 trait 路径 fail-closed。
        Err(SessionError::CapabilityDenied {
            capability: "ssh_trusted_connection".into(),
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![TransferProtocolType::sftp()]
    }

    /// SSH 无硬件端点枚举 — 返回空列表
    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(auth_method: SshAuthMethod) -> SshConfig {
        SshConfig {
            host: "example.test".into(),
            port: 22,
            username: "root".into(),
            auth_method,
            password: Some("secret".into()),
            private_key: Some("key".into()),
            passphrase: None,
            data_mode: "text".into(),
            file_service_enabled: true,
            file_service_protocol: "sftp".into(),
        }
    }

    #[test]
    fn ssh_auth_method_is_typed_at_deserialization_boundary() {
        let password: SshAuthMethod = serde_json::from_str("\"password\"").unwrap();
        let key: SshAuthMethod = serde_json::from_str("\"key\"").unwrap();
        assert_eq!(password, SshAuthMethod::Password);
        assert_eq!(key, SshAuthMethod::Key);
        assert!(serde_json::from_str::<SshAuthMethod>("\"agent\"").is_err());
    }

    #[test]
    fn ipv6_endpoint_is_normalized_without_string_address_ambiguity() {
        assert_eq!(normalize_ssh_host("[2001:db8::1]"), "2001:db8::1");
        assert_eq!(
            format_ssh_endpoint("2001:db8::1", 2222),
            "[2001:db8::1]:2222"
        );
        assert_eq!(format_ssh_endpoint("example.test", 22), "example.test:22");
    }

    #[test]
    fn config_validation_rejects_missing_credentials_and_invalid_port() {
        let mut password = config(SshAuthMethod::Password);
        password.password = None;
        assert!(matches!(
            password.validate(),
            Err(SessionError::InvalidParameter(_))
        ));

        let mut key = config(SshAuthMethod::Key);
        key.private_key = None;
        assert!(matches!(
            key.validate(),
            Err(SessionError::InvalidParameter(_))
        ));

        let mut port = config(SshAuthMethod::Password);
        port.port = 0;
        assert!(matches!(
            port.validate(),
            Err(SessionError::InvalidParameter(_))
        ));
    }

    #[test]
    fn debug_output_never_contains_ssh_secrets() {
        let config = config(SshAuthMethod::Password);
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"));
        assert_eq!(debug.matches("<redacted>").count(), 2);
    }
}
