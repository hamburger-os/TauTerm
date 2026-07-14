//! I/O 通道抽象层
//!
//! 定义协议无关的 `Channel` trait 和双模 I/O 策略。
//! 所有传输类型（串口、TCP、SSH channel、Pipe、UDP socket）通过实现 `Channel` trait
//! 成为可被 I/O 循环引擎驱动的统一接口。

pub mod error;
pub mod io_loop;
pub mod serial_channel;
pub mod serial_comm;


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
}

/// I/O 策略枚举
///
/// 插件在 `ProtocolAdapter::io_strategy()` 中声明自己需要的 I/O 模式。
/// 由 SerialAdapter 实现，非串口协议插件在未来版本中使用。
///
/// 预留: 用于区分同步/异步 I/O 策略。
/// 当前仅使用 `Sync` 变体（串口/Pipe 等阻塞式传输），
/// `Async` 变体为 SSH/TCP/HTTP 等非阻塞网络传输插件预留。
#[allow(dead_code)] // Async 变体为 SSH/TCP 插件预留，当前未构造
#[derive(Debug, Clone, PartialEq)]
pub enum IoStrategy {
    /// 同步模式：使用 `std::thread` 驱动 I/O 循环
    /// 适用于串口、Pipe 等阻塞式传输
    Sync,
    /// 异步模式：使用 `tokio::spawn` 驱动 I/O 循环
    /// 适用于 TCP、SSH、HTTP 等非阻塞网络传输（预留）
    Async,
}

/// 内容类型
///
/// 由 ProtocolAdapter::content_type() 返回，前端渲染器根据此值选择视图。
/// 各变体与前端渲染器的对应关系：
/// - `Terminal` → `TerminalRenderer` (xterm.js 终端)
/// - `FileBrowser` → `FileBrowserRenderer` (双栏文件浏览器，预留)
/// - `StatsDashboard` → `StatsDashboardRenderer` (统计仪表盘，预留)
/// - `Custom(String)` → `CustomRenderer` (插件自定义 UI，预留)
///
/// 当前仅 `Terminal` 变体被 Serial 插件使用，其余为多协议扩展预留。
#[allow(dead_code)] // FileBrowser/StatsDashboard/Custom 变体为多协议扩展预留，当前未构造
#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    /// xterm.js 终端渲染
    Terminal,
    /// 双栏文件浏览器（预留: SFTP/FTP 插件）
    FileBrowser,
    /// 统计仪表盘（预留: 网络监控/Syslog 插件）
    StatsDashboard,
    /// 插件自定义 UI
    Custom(String),
}

#[allow(dead_code)] // from_str/as_str 为多协议插件预留，当前仅 Serial 插件使用硬编码 "terminal"
impl ContentType {
    /// 从字符串解析内容类型
    pub fn from_str(s: &str) -> Self {
        match s {
            "terminal" => ContentType::Terminal,
            "file_browser" => ContentType::FileBrowser,
            "stats_dashboard" => ContentType::StatsDashboard,
            other => ContentType::Custom(other.to_string()),
        }
    }

    /// 返回内容类型的字符串表示
    pub fn as_str(&self) -> &str {
        match self {
            ContentType::Terminal => "terminal",
            ContentType::FileBrowser => "file_browser",
            ContentType::StatsDashboard => "stats_dashboard",
            ContentType::Custom(_) => "custom",
        }
    }
}
