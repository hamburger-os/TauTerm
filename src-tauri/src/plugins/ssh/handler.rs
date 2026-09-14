//! russh client Handler 实现
//!
//! russh 要求实现 `client::Handler` trait 以处理服务器推送的消息。
//! 主机密钥验证通过 `tokio::sync::oneshot` 通道与 `build_connection` 协程
//! 协同工作——`check_server_key` 计算 SHA256 指纹后阻塞等待用户确认，
//! `build_connection` 在另一个 select 分支中接收指纹并通知前端。

use russh::client::Handler;
use russh::keys::HashAlg;
use tokio::sync::oneshot;

/// 主机密钥验证请求
///
/// `check_server_key` 被调用时，记录服务器主机密钥算法并计算 SHA256 指纹，
/// 通过 `verifier_tx` 发送给 `build_connection`，然后阻塞等待用户确认。
pub(crate) struct HostKeyVerification {
    /// 主机密钥算法（如 `ssh-ed25519` / `ecdsa-sha2-nistp256` / `ssh-rsa`）
    pub algorithm: String,
    /// 主机密钥 SHA256 指纹（Base64 编码，如 "SHA256:xxxx"）
    pub fingerprint: String,
    /// 响应通道——`true` 表示用户接受，`false` 表示拒绝
    pub response: oneshot::Sender<bool>,
}

/// SSH 客户端 Handler
///
/// 在构造时接收验证请求通道，用于在 `check_server_key` 被调用时将主机密钥算法和
/// 指纹发送给 `build_connection` 协程。指纹格式为 `SHA256:<Base64>`，与 OpenSSH
/// 默认指纹格式一致。
pub struct SshHandler {
    /// 主机密钥验证请求通道。
    /// `check_server_key` 被调用时创建 `HostKeyVerification` 并通过此通道发送，
    /// 随后阻塞在 response oneshot 上等待用户确认。
    verifier_tx: Option<tokio::sync::mpsc::Sender<HostKeyVerification>>,
}

impl SshHandler {
    /// 创建一个新的 Handler，绑定到指定的验证请求通道。
    pub(crate) fn new(verifier_tx: tokio::sync::mpsc::Sender<HostKeyVerification>) -> Self {
        Self {
            verifier_tx: Some(verifier_tx),
        }
    }
}

impl Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let algorithm = server_public_key.algorithm().as_str().to_string();
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();

        log::info!("SSH 服务器主机密钥: algorithm={algorithm}, fingerprint={fingerprint}");

        if let Some(tx) = &self.verifier_tx {
            let (response_tx, response_rx) = oneshot::channel();
            let verification = HostKeyVerification {
                algorithm,
                fingerprint,
                response: response_tx,
            };

            match tx.send(verification).await {
                Ok(()) => match response_rx.await {
                    Ok(accepted) => {
                        if accepted {
                            log::info!("用户接受主机密钥");
                            Ok(true)
                        } else {
                            log::warn!("用户拒绝主机密钥");
                            Ok(false)
                        }
                    }
                    Err(_) => {
                        log::warn!("主机密钥验证超时或被取消");
                        Ok(false)
                    }
                },
                Err(error) => {
                    log::error!("无法发送主机密钥验证请求: {error}");
                    Ok(false)
                }
            }
        } else {
            log::error!("SSH Handler 未配置主机密钥验证器——拒绝连接");
            Ok(false)
        }
    }
}

impl Drop for SshHandler {
    fn drop(&mut self) {
        self.verifier_tx.take();
    }
}
