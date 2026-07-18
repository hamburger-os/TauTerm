//! I/O 通道抽象层
//!
//! 定义协议无关的 `Channel` trait 和双模 I/O 策略。
//! 所有传输类型（串口、TCP、SSH channel、Pipe、UDP socket）通过实现 `Channel` trait
//! 成为可被 I/O 循环引擎驱动的统一接口。

pub mod error;
pub mod io_loop;
pub mod async_io_loop;
pub mod serial_channel;
pub mod serial_comm;
pub mod ssh_channel;


use std::io::{Read, Write};
use std::time::Duration;
use std::any::Any;
use error::ChannelError;

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
}
