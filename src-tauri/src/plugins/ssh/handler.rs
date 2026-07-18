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
/// `check_server_key` 被调用时，计算服务器主机密钥的 SHA256 指纹，
/// 通过 `verifier_tx` 发送给 `build_connection`，然后阻塞等待用户确认。
pub(crate) struct HostKeyVerification {
    /// 主机密钥 SHA256 指纹（Base64 编码，如 "SHA256:xxxx"）
    pub fingerprint: String,
    /// 响应通道——`true` 表示用户接受，`false` 表示拒绝
    pub response: oneshot::Sender<bool>,
}

/// SSH 客户端 Handler
///
/// 在构造时接收一个 `oneshot::Sender`，用于在 `check_server_key`
/// 被调用时将主机密钥指纹发送给 `build_connection` 协程。
/// 指纹格式：`SHA256:<Base64>`（与 OpenSSH 默认指纹格式一致）。
pub struct SshHandler {
    /// 主机密钥验证请求通道。
    /// `check_server_key` 被调用时创建 `HostKeyVerification` 并通过此通道发送。
    /// 随后阻塞在 `response` oneshot 上等待用户确认。
    ///
    /// 使用 `Option` 以便在 `connect` 完成后清理（drop tx 使接收端退出循环）。
    verifier_tx: Option<tokio::sync::mpsc::Sender<HostKeyVerification>>,
}

impl SshHandler {
    /// 创建一个新的 Handler，绑定到指定的验证请求通道。
    pub fn new(verifier_tx: tokio::sync::mpsc::Sender<HostKeyVerification>) -> Self {
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
        // 计算 SHA256 指纹（与 `ssh-keygen -lf` 默认输出格式一致）
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256);
        // fingerprint 类型实现了 Display，输出格式为 "SHA256:xxxx"
        let display = fingerprint.to_string();

        log::info!("SSH 服务器主机密钥指纹: {}", display);

        // 尝试发送验证请求
        if let Some(tx) = &self.verifier_tx {
            let (response_tx, response_rx) = oneshot::channel();
            let verification = HostKeyVerification {
                fingerprint: display,
                response: response_tx,
            };

            match tx.send(verification).await {
                Ok(()) => {
                    // 阻塞等待用户确认（由 build_connection 协程中继到前端）
                    match response_rx.await {
                        Ok(accepted) => {
                            if accepted {
                                log::info!("用户接受主机密钥");
                                return Ok(true);
                            }
                            log::warn!("用户拒绝主机密钥");
                            return Ok(false);
                        }
                        Err(_) => {
                            // oneshot sender 被丢弃（前端关闭/超时）
                            log::warn!("主机密钥验证超时或被取消");
                            return Ok(false);
                        }
                    }
                }
                Err(e) => {
                    log::error!("无法发送主机密钥验证请求: {}", e);
                    return Ok(false);
                }
            }
        }

        // 无验证器配置：拒绝连接
        log::error!("SSH Handler 未配置主机密钥验证器——拒绝连接");
        Ok(false)
    }
}

impl Drop for SshHandler {
    fn drop(&mut self) {
        // 释放 verifier_tx 以通知 build_connection 的接收循环退出
        self.verifier_tx.take();
    }
}
