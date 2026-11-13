//! SFTP 能力对象。
//!
//! 一个 SSH 连接恰好拥有一个 SFTP 子系统会话；本对象是该会话的唯一 owner：
//! 惰性打开、单次初始化、失效后可重建。调用方拿到的只是 `Arc<SftpSession>`，
//! 不需要知道缓存、锁或通道打开细节。

use std::sync::Arc;

use russh::client::Handle;
use russh_sftp::client::SftpSession;
use tokio::sync::Mutex;

use super::error::{SftpError, SftpErrorCode};
use crate::plugins::ssh::handler::SshHandler;

/// SFTP 子系统能力。由 `SshRuntime` 持有并对外以 `Arc` 形式提供。
pub struct SftpCapability {
    handle: Arc<Handle<SshHandler>>,
    /// 缓存的 SFTP 会话。稳态读取只短暂持锁；初始化期间持锁以串行化首个打开，
    /// 避免并发调用重复建立子系统通道。
    cached: Mutex<Option<Arc<SftpSession>>>,
}

impl SftpCapability {
    pub fn new(handle: Arc<Handle<SshHandler>>) -> Arc<Self> {
        Arc::new(Self {
            handle,
            cached: Mutex::new(None),
        })
    }

    /// 返回已建立的 SFTP 会话；首次调用时打开 `subsystem sftp` 通道并完成 INIT。
    pub async fn client(&self) -> Result<Arc<SftpSession>, SftpError> {
        if let Some(session) = self.cached.lock().await.as_ref() {
            return Ok(session.clone());
        }
        self.open().await
    }

    /// 连接可用性探测。成功即表示本连接具备 SFTP 能力。
    pub async fn probe(&self) -> Result<(), SftpError> {
        self.client().await.map(|_| ())
    }

    /// 丢弃缓存会话。之后的 `client()` 会重新打开子系统通道。
    pub async fn invalidate(&self) {
        let mut cached = self.cached.lock().await;
        if cached.take().is_some() {
            log::info!("SFTP 子系统会话已失效，下次访问时重建");
        }
    }

    /// 观察到的错误若属于连接失效，则使缓存会话失效。
    pub async fn invalidate_if_broken(&self, error: &SftpError) {
        if error.is_connection_lost() {
            self.invalidate().await;
        }
    }

    /// 清理远端临时产物。失败只记日志：调用方此时通常已经在返回错误。
    pub async fn remove_best_effort(&self, path: &str) {
        match self.client().await {
            Ok(session) => {
                if let Err(error) = session.remove_file(path).await {
                    log::warn!("清理远端临时文件 '{}' 失败: {}", path, error);
                }
            }
            Err(error) => log::warn!("清理远端临时文件 '{}' 失败: {}", path, error),
        }
    }

    /// 远端 home（起始目录）。SFTP `realpath(".")` 是 OpenSSH `sftp` 客户端的标准做法：
    /// 不依赖 shell、不占用 exec 通道，sftp-only / chroot 主机同样可用。
    pub async fn home_dir(&self) -> Result<String, SftpError> {
        let session = match self.client().await {
            Ok(session) => session,
            Err(error) => {
                self.invalidate_if_broken(&error).await;
                return Err(error);
            }
        };

        let resolved = match session.canonicalize(".").await {
            Ok(path) if !path.is_empty() => Ok(path),
            Ok(_) => Err(SftpError::new(
                SftpErrorCode::NotFound,
                "远端未返回有效的起始目录",
            )),
            Err(primary) => match session.expand_path("~").await {
                Ok(Some(path)) if !path.is_empty() => Ok(path),
                _ => Err(SftpError::from_client(&primary)),
            },
        };
        if let Err(error) = &resolved {
            self.invalidate_if_broken(error).await;
        }
        resolved
    }

    async fn open(&self) -> Result<Arc<SftpSession>, SftpError> {
        let mut cached = self.cached.lock().await;
        if let Some(session) = cached.as_ref() {
            return Ok(session.clone());
        }

        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| SftpError::closed(format!("打开 SFTP 通道失败: {error}")))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| {
                // 该错误既可能是服务端拒绝子系统（CHANNEL_FAILURE），也可能是通道/传输
                // 瞬时失败，russh 不区分；文案保持两种可能，由探测缓存策略决定是否重试。
                SftpError::unsupported(format!(
                    "请求 SFTP 子系统失败（服务端可能未启用 Subsystem sftp）: {error}"
                ))
            })?;
        let session = SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| {
                SftpError::from_client(&error)
                    .with_message(format!("初始化 SFTP 会话失败: {error}"))
            })?;

        let session = Arc::new(session);
        *cached = Some(session.clone());
        log::info!("SFTP 子系统通道已建立并缓存");
        Ok(session)
    }
}
