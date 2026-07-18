//! SSH Channel 实现（基于 russh async API）
//!
//! 包装 `russh::Channel<russh::Msg>` 实现 `AsyncChannel` trait。
//! 由 `spawn_async_io_loop`（tokio task）驱动。
//! russh Handle 内部线程安全，无需 Mutex——终端 I/O 与 SFTP 可安全并发。

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::channel::error::ChannelError;
use crate::channel::AsyncChannel;
use crate::plugins::ssh::handler::SshHandler;

/// SSH 通道
///
/// 包装 russh 的 Channel 类型，实现 `AsyncChannel` trait。
/// 持有 `Arc<russh::client::Handle<SshHandler>>` 以保持 SSH 会话存活
/// （SshSideChannel 也持有同一 Arc），russh Handle 内部线程安全，
/// 允许终端 I/O 和 SFTP 操作并发访问同一会话。
pub struct SshChannel {
    /// russh channel（用于 wait 读取数据、data 写入、window_change 调整 PTY）
    channel: russh::Channel<russh::client::Msg>,
    /// 持有 Handle 引用以保持会话存活（SshSideChannel 也持有同一 Arc）
    _handle: Arc<russh::client::Handle<SshHandler>>,
    /// 连接状态标志。read() 返回 0/Eof/Close 时置 false。
    connected: AtomicBool,
}

impl SshChannel {
    /// 创建新的 SSH 通道
    pub fn new(
        channel: russh::Channel<russh::client::Msg>,
        handle: Arc<russh::client::Handle<SshHandler>>,
    ) -> Self {
        Self {
            channel,
            _handle: handle,
            connected: AtomicBool::new(true),
        }
    }
}

#[async_trait::async_trait]
impl AsyncChannel for SshChannel {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // russh Channel 不实现 AsyncRead，需要通过 wait() 循环获取 ChannelMsg::Data
        loop {
            match self.channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    let data_slice = data.as_ref();
                    let n = data_slice.len();
                    if n > buf.len() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "SSH data chunk {} bytes exceeds buf capacity {} — increase RCV_BUF_SIZE",
                                n,
                                buf.len()
                            ),
                        ));
                    }
                    buf[..n].copy_from_slice(data_slice);
                    return Ok(n);
                }
                Some(russh::ChannelMsg::ExtendedData { data, ext }) => {
                    // stderr 等扩展数据，写入主数据流（终端场景下合并显示）
                    if ext == 1 {
                        let data_slice = data.as_ref();
                        let n = data_slice.len();
                        if n > buf.len() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "SSH extended data chunk {} bytes exceeds buf capacity {} — increase RCV_BUF_SIZE",
                                    n,
                                    buf.len()
                                ),
                            ));
                        }
                        buf[..n].copy_from_slice(data_slice);
                        return Ok(n);
                    }
                    // 其他扩展数据忽略
                    continue;
                }
                Some(russh::ChannelMsg::Eof) => {
                    // 远端发送 EOF，返回 0 表示正常结束
                    self.connected.store(false, Ordering::Relaxed);
                    return Ok(0);
                }
                Some(russh::ChannelMsg::Close) => {
                    self.connected.store(false, Ordering::Relaxed);
                    return Ok(0);
                }
                Some(_) => {
                    // 其他消息（ExitStatus、Signal 等）忽略，继续等待数据
                    continue;
                }
                None => {
                    // channel 已关闭
                    self.connected.store(false, Ordering::Relaxed);
                    return Ok(0);
                }
            }
        }
    }

    async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.channel
            .data(buf)
            .await
            .map_err(|e| std::io::Error::other(format!("SSH write 失败: {}", e)))?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        // russh 无显式 flush 概念，data 立即发送
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn set_timeout(&mut self, _dur: Duration) -> Result<(), ChannelError> {
        // async 模式无需 timeout 设置
        Ok(())
    }

    /// 转发 PTY 窗口大小调整到远端 SSH 通道
    async fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<(), ChannelError> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|e| ChannelError::Io(std::io::Error::other(format!("PTY resize 失败: {}", e))))?;
        Ok(())
    }

    fn try_handoff(&mut self) -> Option<Box<dyn Any>> {
        None
    }
}
