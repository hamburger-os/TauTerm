//! SSH exec 通道。
//!
//! 远端命令执行是 SSH 的第三条通道类型（终端 = shell，文件 = subsystem sftp，
//! 命令 = exec）。本模块拥有 exec 通道的打开、命令启动与有界输出收集；
//! journald 是当前唯一消费者，它自己负责构造 `journalctl` 命令字符串与 shell 转义。

use std::sync::Arc;
use std::time::Duration;

use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg};

use crate::plugins::ssh::handler::SshHandler;

/// 有界命令执行的资源上限。
pub struct ExecLimits {
    pub max_stdout: usize,
    pub max_stderr: usize,
    pub timeout: Duration,
}

/// 命令执行结果。stdout/stderr 分开保留，exit status 单独表达失败原因。
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
}

impl ExecOutput {
    pub fn failure_message(&self, command: &str) -> String {
        let stderr = String::from_utf8_lossy(&self.stderr).trim().to_string();
        if stderr.is_empty() {
            format!(
                "{command} exited with status {}",
                self.exit_status.unwrap_or(u32::MAX)
            )
        } else {
            format!(
                "{command} exited with status {}: {stderr}",
                self.exit_status.unwrap_or(u32::MAX)
            )
        }
    }
}

/// exec 通道错误。
#[derive(Debug, Clone)]
pub enum ExecError {
    ChannelOpen(String),
    Request(String),
    Timeout,
    OutputTooLarge,
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelOpen(message) => write!(f, "SSH exec channel open failed: {message}"),
            Self::Request(message) => write!(f, "SSH exec request failed: {message}"),
            Self::Timeout => write!(f, "远端命令执行超时"),
            Self::OutputTooLarge => write!(f, "远端命令输出超过安全上限"),
        }
    }
}

impl std::error::Error for ExecError {}

/// 打开一条 session 通道（尚未启动命令）。
///
/// 拆成 open/start 两步是为了让流式消费者在“通道已打开、命令尚未启动”之间插入
/// 取消检查点：取消后再 exec 会在远端多跑一条命令。
pub async fn open_channel(handle: &Arc<Handle<SshHandler>>) -> Result<Channel<Msg>, ExecError> {
    handle
        .channel_open_session()
        .await
        .map_err(|error| ExecError::ChannelOpen(error.to_string()))
}

/// 在已打开的通道上启动远端命令。
///
/// 命令字符串原样交给远端登录 shell（SSH exec 语义）；调用方负责对用户输入做
/// shell 转义。
pub async fn start(channel: &mut Channel<Msg>, command: &str) -> Result<(), ExecError> {
    channel
        .exec(true, command)
        .await
        .map_err(|error| ExecError::Request(error.to_string()))
}

/// 打开一条 exec 通道并启动远端命令（open + start 的便捷组合）。
///
/// 需要“打开后、启动前”取消检查点的调用方应改用 `open_channel` + `start`。
pub async fn open(
    handle: &Arc<Handle<SshHandler>>,
    command: &str,
) -> Result<Channel<Msg>, ExecError> {
    let mut channel = open_channel(handle).await?;
    start(&mut channel, command).await?;
    Ok(channel)
}

/// 在 exec 通道上执行命令并收集有界输出。
///
/// 只有在通道 Close/EOF 之后才返回，确保迟到到达的 exit status 被观察到。
pub async fn run(
    handle: &Arc<Handle<SshHandler>>,
    command: &str,
    limits: &ExecLimits,
) -> Result<ExecOutput, ExecError> {
    let mut channel = open(handle, command).await?;
    collect(&mut channel, limits).await
}

/// 收集一条已打开的 exec 通道的完整输出。
pub async fn collect(
    channel: &mut Channel<Msg>,
    limits: &ExecLimits,
) -> Result<ExecOutput, ExecError> {
    let collect = async {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    if stdout.len().saturating_add(data.len()) > limits.max_stdout {
                        return Err(ExecError::OutputTooLarge);
                    }
                    stdout.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExtendedData { data, ext }) => {
                    if ext == 1 && stderr.len() < limits.max_stderr {
                        let remaining = limits.max_stderr - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(remaining)]);
                    }
                }
                Some(ChannelMsg::ExitStatus {
                    exit_status: status,
                }) => {
                    exit_status = Some(status);
                }
                Some(ChannelMsg::Eof) => {
                    // 继续等到 Close/None：exit status 可能在 EOF 之后到达。
                }
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        Ok(ExecOutput {
            stdout,
            stderr,
            exit_status,
        })
    };

    tokio::time::timeout(limits.timeout, collect)
        .await
        .map_err(|_| ExecError::Timeout)?
}
