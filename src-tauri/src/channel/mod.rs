//! I/O 通道抽象层
//!
//! 定义协议无关的 `Channel` trait 和双模 I/O 策略。
//! 所有传输类型（串口、TCP、SSH channel、Pipe、UDP socket）通过实现 `Channel` trait
//! 成为可被 I/O 循环引擎驱动的统一接口。

pub mod async_io_loop;
#[cfg(windows)]
pub mod elevated_shell_channel;
pub mod error;
pub mod io_loop;
pub mod local_shell_channel;
pub mod serial_channel;
pub mod serial_comm;
pub mod ssh_channel;

use error::ChannelError;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::io::{Read, Write};
use std::time::Duration;

/// 会话 I/O 结束原因。前端据此决定是否保留终端现场，避免解析本地化错误字符串。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectKind {
    UserRequested,
    RemoteEof,
    IoError,
    DeviceRemoved,
    ProcessExited,
}

/// 协议无关的结构化断开信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectInfo {
    pub kind: DisconnectKind,
    pub reason: String,
    pub exit_code: Option<u32>,
    pub retain_terminal: bool,
}

impl DisconnectInfo {
    pub fn user_requested() -> Self {
        Self {
            kind: DisconnectKind::UserRequested,
            reason: "User requested disconnect".into(),
            exit_code: None,
            retain_terminal: false,
        }
    }

    pub fn remote_eof(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::RemoteEof,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn io_error(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::IoError,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn device_removed(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::DeviceRemoved,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn process_exited(exit_code: u32, signal: Option<&str>) -> Self {
        let success = exit_code == 0 && signal.is_none();
        let reason = match signal {
            Some(signal) => format!("Local shell terminated by {signal}"),
            None if success => "Local shell exited normally".into(),
            None => format!("Local shell exited with code {exit_code}"),
        };
        Self {
            kind: DisconnectKind::ProcessExited,
            reason,
            exit_code: Some(exit_code),
            retain_terminal: !success,
        }
    }
}

/// 统一 I/O 通道 trait
///
/// 所有传输类型必须实现此 trait。
/// 继承 `Read` + `Write` 提供标准字节流操作。
/// 必须 object-safe（可用作 `Box<dyn Channel>`）。
pub trait Channel: Read + Write + Send {
    /// 通道是否仍处于连接状态
    fn is_connected(&self) -> bool;

    /// 设置读写超时
    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError>;

    /// I/O 循环启动回调：在数据读循环开始前通知通道其 session_id。
    ///
    /// 默认空实现。需要会话标识的协议（如 Telnet 回显事件回调需
    /// 附带 session_id 发射事件）覆盖此方法，保证首条数据到达前注入完成。
    fn on_session_started(&mut self, _session_id: &str) {}

    /// 尝试交出底层传输的所有权（用于 Inline 传输策略）
    ///
    /// 返回 `Some(Box<dyn Any>)` 如果传输支持所有权交出。
    /// 返回 `None` 表示不支持（如 SSH channel），应使用 SideChannel 策略。
    fn try_handoff(&mut self) -> Option<Box<dyn Any>> {
        None // 默认不支持交出
    }

    /// 请求 PTY 窗口大小调整（仅 SSH 等支持 PTY 的协议需要实现）。
    ///
    /// 默认实现为空操作，串口等无 PTY 概念的协议直接忽略。
    /// 前端终端 resize 时通过 IoLoopCmd::ResizePty 触发。
    fn resize_pty(&mut self, _cols: u32, _rows: u32) -> Result<(), ChannelError> {
        Ok(())
    }

    /// 请求通道执行协议特定的优雅关闭。I/O loop 在处理 Shutdown 时调用。
    fn shutdown(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }

    /// 允许通道用更精确的信息替代 I/O loop 提供的默认断开原因。
    fn disconnect_info(&self, fallback: DisconnectInfo) -> DisconnectInfo {
        fallback
    }
}

/// I/O 策略枚举
///
/// 插件在 `ProtocolAdapter::io_strategy()` 中声明自己需要的 I/O 模式。
/// - `Sync`：串口、Pipe 等阻塞式传输，由 `spawn_sync_io_loop` 驱动（std::thread）
/// - `Async`：SSH（russh）等基于 tokio 的协议，由 `spawn_async_io_loop` 驱动（tokio task）
#[derive(Debug, Clone, PartialEq)]
pub enum IoStrategy {
    /// 同步模式：使用 `std::thread` 驱动 I/O 循环
    /// 适用于串口、Pipe 等阻塞式传输
    Sync,
    /// 异步模式：使用 tokio task 驱动 I/O 循环
    /// 适用于 SSH（russh async API）等基于 tokio 的协议
    Async,
}

/// 异步 I/O 通道 trait
///
/// 与同步 `Channel` trait 并存。仅 SSH（russh async API）等基于 tokio 的协议实现此 trait。
/// 串口继续实现同步 `Channel`，由 `spawn_sync_io_loop` 驱动。
#[async_trait::async_trait]
pub trait AsyncChannel: Send {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
    async fn flush(&mut self) -> std::io::Result<()>;
    fn is_connected(&self) -> bool;
    fn set_timeout(&mut self, _dur: Duration) -> Result<(), ChannelError> {
        Ok(())
    }
    /// 请求 PTY 窗口大小调整（仅 SSH 等支持 PTY 的协议需要实现）
    async fn resize_pty(&mut self, _cols: u32, _rows: u32) -> Result<(), ChannelError> {
        Ok(())
    }
    async fn shutdown(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }
    fn disconnect_info(&self, fallback: DisconnectInfo) -> DisconnectInfo {
        fallback
    }
    /// 尝试交出底层传输的所有权（用于 Inline 传输策略）
    ///
    /// 异步路径默认不支持（SSH 使用 SideChannel 策略）
    fn try_handoff(&mut self) -> Option<Box<dyn Any>> {
        None
    }
}

/// 内容类型
///
/// 由 ProtocolAdapter::content_type() 返回，前端渲染器根据此值选择视图。
/// 当前仅 `Terminal` 变体被使用（Serial、SSH 插件），前端通过 manifest.content_type
/// 字符串字段进行渲染器调度（见 TabContentDispatcher.tsx）。
/// 后端 ContentType 枚举仅用于日志记录，未来多协议扩展时按需新增变体。
#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    /// xterm.js 终端渲染
    Terminal,
    /// 插件自定义视图（TFTP / iPerf / 网络调试）
    Custom,
}
