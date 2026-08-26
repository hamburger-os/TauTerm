//! TCP 裸字节通道（网络调试会话用）
//!
//! 包装 `std::net::TcpStream` 实现内核 `Channel` trait。
//!
//! 参考 Telnet 通道的 EOF 探测模式：`TcpStream` 在对端关闭时 `read` 返回 `Ok(0)`，
//! 而内核 I/O 循环把 `Ok(0)` 视为"空闲"而非"断开"——因此用 `try_clone` 探针
//! 先 `peek` 区分"真 EOF"（触发断开）与"暂无可读数据"（进入空闲）。

use std::io::{ErrorKind, Read, Write};
use std::time::Duration;

use crate::channel::{error::ChannelError, Channel};

/// 通道读超时（与串口/Telnet 的 50ms 策略一致）
pub(crate) const READ_TIMEOUT: Duration = Duration::from_millis(50);

/// TCP 裸字节通道
pub struct TcpChannel {
    stream: std::net::TcpStream,
    /// EOF 探针（`try_clone` 共享底层 socket 与超时配置，不消费数据）
    probe: std::net::TcpStream,
    connected: bool,
}

impl TcpChannel {
    pub fn new(stream: std::net::TcpStream) -> std::io::Result<Self> {
        let probe = stream.try_clone()?;
        probe.set_read_timeout(Some(READ_TIMEOUT))?;
        Ok(Self {
            stream,
            probe,
            connected: true,
        })
    }
}

impl Read for TcpChannel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // EOF 检测必须先行：区分真 EOF 与空闲
        match self.probe.peek(&mut [0u8; 1]) {
            Ok(0) => {
                self.connected = false;
                return Err(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "对端已关闭连接",
                ));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                return Ok(0);
            }
            Err(e) => {
                self.connected = false;
                return Err(e);
            }
            Ok(_) => {}
        }
        // peek 确认有数据：read 应能立即返回；少数竞态下（数据被并发消费）返回空闲
        match self.stream.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => Ok(0),
            Err(e) => {
                self.connected = false;
                Err(e)
            }
        }
    }
}

impl Write for TcpChannel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

impl Channel for TcpChannel {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError> {
        self.stream
            .set_read_timeout(Some(dur))
            .map_err(ChannelError::Io)
    }
}
