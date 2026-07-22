//! SSH 协议插件（基于 russh async API）
//!
//! 实现 `ProtocolAdapter` trait，提供 SSH 远程终端连接能力。
//! 支持密码和 SSH 密钥两种认证方式。
//! 文件服务（SFTP）通过独立的侧通道操作，不中断终端 I/O 循环。
//! russh Handle 内部线程安全，终端 I/O 与 SFTP 可安全并发。

pub mod handler;

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::channel::{ContentType, IoStrategy};
use crate::channel::error::SessionError;
use crate::channel::ssh_channel::SshChannel;
use crate::kernel::file_transfer::FileTransfer;
use crate::kernel::plugin_adapter::{EndpointInfo, ProtocolAdapter, ProtocolConnection, SideChannel, TransferProtocolType};
use handler::SshHandler;

/// SSH 连接配置
///
/// 由前端 ConnectDialog 构造并通过 Tauri invoke 传递。
/// 所有字段通过 `serde_json::Value` 解析。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// 远程主机地址（IP 或域名）
    pub host: String,
    /// SSH 端口（默认 22）
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// 登录用户名
    pub username: String,
    /// 认证方式: "password" | "key"
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    /// 密码（auth_method == "password" 时使用）
    pub password: Option<String>,
    /// SSH 私钥内容（auth_method == "key" 时，前端直接传入私钥文本）
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

fn default_ssh_port() -> u16 { 22 }
fn default_auth_method() -> String { "password".into() }
fn default_data_mode() -> String { "text".into() }
fn default_file_service_protocol() -> String { "sftp".into() }

/// SSH 协议适配器
///
/// 无状态结构体——每次 `connect()` 调用建立全新的 TCP 连接和 SSH 会话。
/// 通过 `connect()` 返回 `ProtocolConnection`，携带：
/// - `channel`: `SshChannel`（终端 I/O，async 路径）
/// - `comm_handle`: None（由 SessionStore 统一使用默认 CommHandle 包装 write_tx）
/// - `side_channel`: `SshSideChannel`（供 SFTP 文件服务复用 SSH Handle 和 SFTP 缓存）
pub struct SshAdapter;

impl SshAdapter {
    pub fn new() -> Self {
        Self
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
        let result = build_connection_with_config(config, Some(app_handle), Some(verifier)).await?;
        Ok(ProtocolConnection {
            channel: crate::kernel::plugin_adapter::ChannelKind::Async(Box::new(result.channel)),
            comm_handle: None,
            side_channel: Some(Arc::new(SshSideChannel::new(
                result.session,
                result.host_key_fingerprint,
            ))),
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
pub struct HostKeyVerifier {
    inner: std::sync::Arc<tokio::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>
    >>,
}

impl HostKeyVerifier {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// 注册一个待确认的验证请求，返回 `wait_rx` 供调用方阻塞等待用户决定。
    /// `fingerprint` 作为键（SHA256 指纹），允许多个并发连接各自独立等待。
    pub async fn register(&self, fingerprint: &str) -> tokio::sync::oneshot::Receiver<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.inner.lock().await.insert(fingerprint.to_string(), tx);
        rx
    }

    /// 用户确认后响应验证请求（accept=true 接受，accept=false 拒绝）。
    /// 返回 `true` 表示找到对应请求并已响应，`false` 表示指纹未找到（可能已超时或重复确认）。
    pub async fn respond(&self, fingerprint: &str, accept: bool) -> bool {
        if let Some(tx) = self.inner.lock().await.remove(fingerprint) {
            let _ = tx.send(accept);
            true
        } else {
            false
        }
    }
}

/// 供 SFTP 文件服务使用的侧通道资源。
///
/// 持有 SSH 会话引用和缓存的 SFTP 对象，通过 `ProtocolConnection::side_channel`
/// 传递给 `SessionStore`。SFTP 命令通过 `downcast_ref::<SshSideChannel>()` 还原。
///
/// - `session` — russh Handle（内部线程安全，与 SshChannel 共享同一 Arc）
/// - `sftp` — 缓存的 SFTP 子系统通道，避免每次操作重新协商
pub struct SshSideChannel {
    /// russh Handle（内部线程安全，与 SshChannel 共享）
    pub session: Arc<russh::client::Handle<SshHandler>>,
    /// 缓存的 SFTP 对象，首次 SFTP 操作时惰性创建。
    /// 使用 `tokio::sync::Mutex` 以便多个 SFTP 命令并发访问时共享缓存。
    pub sftp: Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    /// 主机密钥 SHA256 指纹（首次连接时由 check_server_key 产生）。
    /// 由 connect_session_ssh 通过 session-connected 事件传递到前端。
    pub host_key_fingerprint: Option<String>,
}

impl SshSideChannel {
    pub fn new(
        session: Arc<russh::client::Handle<SshHandler>>,
        host_key_fingerprint: Option<String>,
    ) -> Self {
        Self {
            session,
            sftp: Arc::new(Mutex::new(None)),
            host_key_fingerprint,
        }
    }
}

impl SideChannel for SshSideChannel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn create_file_transfer(&self) -> Option<Arc<dyn FileTransfer>> {
        Some(Arc::new(crate::transfer::sftp_transfer::SftpFileTransfer::new(
            self.session.clone(),
            self.sftp.clone(),
        )))
    }
}

/// 建立连接的产物
struct BuildConnectionResult {
    channel: SshChannel,
    session: Arc<russh::client::Handle<SshHandler>>,
    /// 主机密钥 SHA256 指纹（如 "SHA256:xxxx"），供前端展示/确认
    host_key_fingerprint: Option<String>,
}

/// 建立连接的核心逻辑（async）— 旧接口，解析 JSON 后委托给内部实现。
async fn build_connection(
    params: &serde_json::Value,
) -> Result<BuildConnectionResult, SessionError> {
    let config: SshConfig = serde_json::from_value(params.clone())
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("SSH 配置解析失败: {}", e),
        })?;
    build_connection_with_config(config, None, None).await
}

/// 建立连接的核心逻辑（async）— 直接接收类型化 `SshConfig`。
///
/// 由 `connect_with_config()` 调用（`connect_session_ssh` 路径），
/// 避免 `connect_session_ssh` 解析一次后 `build_connection` 再重复解析。
///
/// 当 `app_handle` 和 `verifier` 均提供时，主机密钥验证将等待用户确认
/// （通过 Tauri 事件 `ssh-host-key-verify` 发送指纹到前端，
/// 前端调用 `confirm_host_key` 命令回传用户决策）。
/// 否则回退到自动接受行为（MVP 遗留，不推荐）。
async fn build_connection_with_config(
    config: SshConfig,
    app_handle: Option<tauri::AppHandle>,
    verifier: Option<&HostKeyVerifier>,
) -> Result<BuildConnectionResult, SessionError> {

    // 1. SSH 连接（russh 内部处理 TCP + 握手）
    let addr = format!("{}:{}", config.host, config.port);
    let russh_config = Arc::new(russh::client::Config::default());
    /// TCP 连接 + SSH 握手超时（秒），不含用户主机密钥确认时间
    const SSH_CONNECT_TIMEOUT_SECS: u64 = 15;
    /// 主机密钥用户确认超时（秒）。前端弹出确认对话框后，用户需在此时间内响应。
    const HOST_KEY_VERIFY_TIMEOUT_SECS: u64 = 30;

    // 创建主机密钥验证通道
    // mpsc 容量为 1：一次连接最多触发一次 check_server_key
    let (verifier_tx, mut verifier_rx) =
        tokio::sync::mpsc::channel::<handler::HostKeyVerification>(1);

    let handler = SshHandler::new(verifier_tx);
    let config_clone = russh_config.clone();
    let addr_clone = addr.clone();

    // 在独立 task 中执行 connect，以便并发处理主机密钥验证
    // 外层 timeout 防止 TCP 连接/握手无限期阻塞
    let mut connect_task = tokio::spawn(async move {
        match tokio::time::timeout(
            std::time::Duration::from_secs(SSH_CONNECT_TIMEOUT_SECS),
            russh::client::connect(config_clone, addr_clone.as_str(), handler),
        )
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => {
                log::warn!("SSH TCP 连接/握手超时 ({}s)", SSH_CONNECT_TIMEOUT_SECS);
                Err(russh::Error::Disconnect)
            }
        }
    });

    // 收集主机密钥指纹（如有）
    let mut host_key_fingerprint: Option<String> = None;

    // select! 并发等待 connect 完成 或 主机密钥验证请求
    let mut handle = loop {
        tokio::select! {
            result = &mut connect_task => {
                // connect task 完成
                break result
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("SSH 连接 task 失败: {}", e),
                    })?
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("SSH 连接失败 '{}': {}", addr, e),
                    })?;
            }
            Some(verification) = verifier_rx.recv() => {
                // 收到主机密钥验证请求
                host_key_fingerprint = Some(verification.fingerprint.clone());
                log::info!("SSH 主机密钥指纹: {}", verification.fingerprint);

                // 用户确认或自动接受
                let accepted = match (app_handle.as_ref(), verifier) {
                    (Some(app), Some(v)) => {
                        // 注册待确认项，前端通过 confirm_host_key 命令响应
                        let wait_rx = v.register(&verification.fingerprint).await;
                        let _ = app.emit("ssh-host-key-verify", serde_json::json!({
                            "fingerprint": verification.fingerprint,
                        }));
                        log::info!("等待用户确认主机密钥...");
                        // 超时保护：前端若未在规定时间内调用 confirm_host_key，
                        // 自动拒绝以释放连接资源
                        tokio::time::timeout(
                            std::time::Duration::from_secs(HOST_KEY_VERIFY_TIMEOUT_SECS),
                            wait_rx,
                        )
                        .await
                        .map(|r| r.unwrap_or(false))
                        .unwrap_or_else(|_elapsed| {
                            log::warn!(
                                "主机密钥验证超时 ({}s)，自动拒绝",
                                HOST_KEY_VERIFY_TIMEOUT_SECS
                            );
                            false
                        })
                    }
                    _ => {
                        // 回退：无 AppHandle 时自动接受（兼容 MVP/测试路径）
                        log::warn!("主机密钥验证不可用（缺少 AppHandle），自动接受");
                        true
                    }
                };
                let _ = verification.response.send(accepted);
                if !accepted {
                    return Err(SessionError::ConnectionFailed {
                        reason: "用户拒绝了主机密钥".into(),
                    });
                }
            }
        }
    };

    // 2. 认证
    let authed = match config.auth_method.as_str() {
        "password" => {
            let password = config.password.as_deref().unwrap_or("");
            handle
                .authenticate_password(&config.username, password)
                .await
                .map_err(|e| SessionError::AuthFailed {
                    reason: format!("密码认证失败: {}", e),
                })?
        }
        "key" => {
            let private_key_str = config.private_key.as_deref().unwrap_or("");
            // russh::keys::PrivateKey 与 russh 0.62 内部的 ssh-key 版本一致，
            // 使用 russh::keys::PrivateKey（与 russh 0.62 同一 ssh-key 版本树），避免版本不匹配
            let mut key_pair = russh::keys::PrivateKey::from_openssh(private_key_str)
                .map_err(|e| SessionError::AuthFailed {
                    reason: format!("私钥解析失败: {}", e),
                })?;
            // 若密钥已加密，使用 passphrase 尝试解密
            if key_pair.is_encrypted() {
                let pass = config.passphrase.as_deref().unwrap_or("");
                if pass.is_empty() {
                    return Err(SessionError::AuthFailed {
                        reason: "私钥已加密但未提供密码短语".into(),
                    });
                }
                key_pair = key_pair.decrypt(pass)
                    .map_err(|e| SessionError::AuthFailed {
                        reason: format!("私钥解密失败（密码短语错误或密钥损坏）: {}", e),
                    })?;
            }
            let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(
                Arc::new(key_pair),
                Some(russh::keys::HashAlg::Sha512),
            );
            handle
                .authenticate_publickey(&config.username, key_with_hash)
                .await
                .map_err(|e| SessionError::AuthFailed {
                    reason: format!("密钥认证失败: {}", e),
                })?
        }
        other => {
            return Err(SessionError::ConnectionFailed {
                reason: format!("不支持的认证方式: {}", other),
            });
        }
    };

    if !authed.success() {
        return Err(SessionError::AuthFailed {
            reason: "SSH 认证未通过".into(),
        });
    }

    // 3. 打开交互式 shell 通道
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("打开 SSH 通道失败: {}", e),
        })?;

    // 4. 请求 PTY（终端）
    channel
        .request_pty(
            true,
            "xterm-256color",
            80,
            24,
            0,
            0,
            &[],
        )
        .await
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("请求 PTY 失败: {}", e),
        })?;

    // 5. 启动 shell
    channel
        .request_shell(true)
        .await
        .map_err(|e| SessionError::ConnectionFailed {
            reason: format!("启动 shell 失败: {}", e),
        })?;

    let handle = Arc::new(handle);
    let ssh_channel = SshChannel::new(channel, handle.clone());

    Ok(BuildConnectionResult {
        channel: ssh_channel,
        session: handle,
        host_key_fingerprint,
    })
}

#[async_trait::async_trait]
impl ProtocolAdapter for SshAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let result = build_connection(params).await?;
        // comm_handle 留空：所有协议的脚本通信均通过 SessionStore 内部的 write_tx
        // 统一包装为默认 CommHandle（write_tx 在 store 内部创建）。
        //
        // 主机密钥指纹（result.host_key_fingerprint）由 connect_session_ssh
        // 通过 SSH 连接的 session-connected 事件传递给前端。
        Ok(ProtocolConnection {
            channel: crate::kernel::plugin_adapter::ChannelKind::Async(Box::new(result.channel)),
            comm_handle: None,
            side_channel: Some(Arc::new(SshSideChannel::new(
                result.session,
                result.host_key_fingerprint,
            ))),
            teardown_delay: self.teardown_delay(),
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn io_strategy(&self) -> IoStrategy {
        // russh 是 async API，使用异步 I/O 循环
        IoStrategy::Async
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![TransferProtocolType::sftp()]
    }

    /// SSH 无硬件端点枚举 — 返回空列表
    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(Vec::new())
    }
}
