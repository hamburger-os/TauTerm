//! 串口 Channel 实现
//!
//! 包装 `Box<dyn SerialPort>` 实现 `Channel` trait。
//! 支持端口所有权交出（用于 YModem 等 Inline 传输策略）。

use std::any::Any;
use std::io::{Read, Write};
use std::time::Duration;
use crate::channel::{Channel, error::ChannelError};

/// 串口通道
///
/// 包装 serialport 库的串口类型，实现 `Channel` trait。
/// 端口用 `Option` 包装以支持所有权交出。
/// 交出后所有 I/O 操作将 panic（调用方应在交出前停止 I/O 循环）。
pub struct SerialChannel {
    port: Option<Box<dyn serialport::SerialPort>>,
    connected: bool,
}

impl SerialChannel {
    pub fn new(port: Box<dyn serialport::SerialPort>) -> Self {
        Self {
            port: Some(port),
            connected: true,
        }
    }

    fn port_mut(&mut self) -> &mut Box<dyn serialport::SerialPort> {
        self.port.as_mut().expect("port already handed off")
    }
}

impl Read for SerialChannel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.port_mut().read(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    Ok(0)
                } else {
                    self.connected = false;
                    Err(e)
                }
            }
        }
    }
}

impl Write for SerialChannel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.port_mut().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.port_mut().flush()
    }
}

impl Channel for SerialChannel {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError> {
        self.port_mut()
            .set_timeout(dur)
            .map_err(|e| ChannelError::Io(e.into()))
    }

    /// 交出底层串口的所有权（用于 YModem 等 Inline 传输策略）
    ///
    /// 调用后通道标记为断开。返回 `Box<dyn Any>` 内含 `Box<dyn SerialPort>`。
    /// 调用方通过 `downcast::<Box<dyn SerialPort>>()` 取回端口。
    fn try_handoff(&mut self) -> Option<Box<dyn Any>> {
        self.connected = false;
        self.port.take().map(|p| Box::new(p) as Box<dyn Any>)
    }
}
