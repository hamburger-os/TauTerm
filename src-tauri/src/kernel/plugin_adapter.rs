//! 协议适配器 trait 和插件清单类型
//!
//! 定义 `ProtocolAdapter` trait——任何协议插件必须实现此 trait。
//! 定义 `PluginManifest`——插件的元数据描述。

use std::any::Any;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::channel::{AsyncChannel, Channel, ContentType, IoStrategy};
use crate::channel::error::SessionError;
use crate::kernel::comm_handle::CommHandle;
use crate::kernel::file_transfer::FileTransfer;

/// 侧通道资源 trait
///
/// 协议插件可将协议特定的辅助资源（如 SSH Session + SFTP 缓存）实现此 trait，
/// 通过 `ProtocolConnection::side_channel` 返回给 `SessionStore`。
/// 调用方（如 SFTP/SCP 命令）通过 `as_any()` 获取 `&dyn Any` 后 `downcast_ref` 到具体类型。
///
/// `Arc<dyn SideChannel>` 可被克隆（仅增加引用计数），允许多个消费者共享同一资源。
pub trait SideChannel: Send + Sync {
    /// 返回 `&dyn Any` 以供类型安全的向下转型
    fn as_any(&self) -> &dyn Any;

    /// 自此侧通道创建文件传输处理器
    ///
    /// 默认返回 `None`。SSH 等协议覆盖此方法，从侧通道资源
    /// （如 `SshSideChannel` 中的 `russh::client::Handle`）构建 `SftpFileTransfer`，
    /// 消除调用方对 `downcast_ref` 的依赖。
    fn create_file_transfer(&self) -> Option<Arc<dyn FileTransfer>> {
        None
    }
}

/// 端点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub name: String,
    pub description: String,
}

/// I/O 通道类型枚举
///
/// - `Sync`：同步通道，由 `spawn_sync_io_loop` 驱动（串口等阻塞式传输）
/// - `Async`：异步通道，由 `spawn_async_io_loop` 驱动（SSH 等 tokio 协议）
pub enum ChannelKind {
    /// 同步通道（实现 `Channel` trait）
    Sync(Box<dyn Channel>),
    /// 异步通道（实现 `AsyncChannel` trait）
    Async(Box<dyn AsyncChannel>),
}

/// 连接产物
///
/// 除 I/O 通道外，协议可能携带的辅助资源：
/// - `comm_handle` — 协议特定的通信句柄。None 时由调用方使用默认 `CommHandle` 实现。
/// - `side_channel` — 供文件服务等侧通道操作的任意资源句柄（如 SSH Session 供 SFTP 复用）。
///
/// 所有协议插件通过实现 `connect()` 返回此结构。
/// 简单协议（如 Serial）仅填充 `channel`，`comm_handle` 和 `side_channel` 留 None。
/// 复合协议（如 SSH）可额外提供侧通道资源供文件服务复用。
pub struct ProtocolConnection {
    /// 双向 I/O 通道（Sync 或 Async）
    pub channel: ChannelKind,
    /// 协议特定的通信句柄（None 表示使用调用方默认实现）
    pub comm_handle: Option<Arc<dyn CommHandle>>,
    /// 侧通道资源句柄（None 表示无辅助资源）
    /// 使用 `Arc<dyn SideChannel>` 以允许多个消费者共享同一资源（如 SFTP 缓存）。
    pub side_channel: Option<Arc<dyn SideChannel>>,
    /// 会话关闭后、资源完全释放前所需的额外等待时间。
    /// 由适配器的 `teardown_delay()` 提供，`close_session()` 据此睡眠，
    /// 避免内核硬编码协议特定逻辑（如串口驱动释放端口的等待）。
    pub teardown_delay: std::time::Duration,
}

/// 传输协议标识 — 字符串包装类型
///
/// 微内核不枚举具体协议。插件声明任意协议标识，内核仅做路由。
/// 提供知名常量工厂方法（`ymodem()`、`sftp()` 等）供内建插件使用。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferProtocolType(String);

impl TransferProtocolType {
    /// 从字符串构造（大小写规范化）
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(s.as_ref().to_lowercase())
    }

    /// 返回内部字符串
    pub fn as_str(&self) -> &str {
        &self.0
    }

    // ── 知名协议常量工厂 ─────────────────────────────

    pub fn ymodem() -> Self { Self("ymodem".into()) }
    pub fn xmodem() -> Self { Self("xmodem".into()) }
    pub fn zmodem() -> Self { Self("zmodem".into()) }
    pub fn sftp() -> Self   { Self("sftp".into()) }
    pub fn ftp() -> Self    { Self("ftp".into()) }

    // ── 分类辅助方法 ─────────────────────────────────

    /// 是否为串口内联传输协议（需要 handoff 串口）
    pub fn is_serial_inline(&self) -> bool {
        matches!(self.0.as_str(), "ymodem" | "xmodem" | "zmodem")
    }

    /// 是否为侧通道传输协议
    pub fn is_side_channel(&self) -> bool {
        self.0 == "sftp"
    }

    /// 是否为独立连接传输协议
    pub fn is_separate_connection(&self) -> bool {
        self.0 == "ftp"
    }
}

/// 字符串解析为 TransferProtocolType（开放集合）
impl std::str::FromStr for TransferProtocolType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_lowercase();
        if lower.is_empty() {
            return Err("传输协议标识不能为空".into());
        }
        // 白名单验证：拒绝未识别的协议名称，避免输入错误静默回退到 SideChannel
        match lower.as_str() {
            "ymodem" | "xmodem" | "zmodem" | "sftp" | "ftp" => Ok(Self(lower)),
            _ => Err(format!("不支持的传输协议: '{}'", s)),
        }
    }
}

impl std::fmt::Display for TransferProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 插件清单
///
/// 描述插件的基本元数据。内建插件在编译时提供此信息。
/// 未来动态插件从 `manifest.json` 加载。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件唯一标识符（kebab-case）
    pub id: String,
    /// 人类可读名称
    pub name: String,
    /// 语义化版本
    pub version: String,
    /// 分类: "terminal", "file_transfer", "network_tool"
    pub category: String,
    /// 描述
    pub description: String,
    /// 图标标识
    pub icon: String,
    /// 内容类型: "terminal", "file_browser", "stats_dashboard", "custom"
    pub content_type: String,
    /// 能力声明列表
    pub capabilities: Vec<String>,
    /// 支持的传输协议列表
    pub transfer_protocols: Vec<TransferProtocolType>,
}

/// 协议适配器 trait
///
/// 任何会话类型插件必须在 Rust 端实现此 trait。
/// 每个方法都有默认实现返回错误，插件只需覆盖自己需要的方法。
///
/// 所有协议插件必须实现 `connect()`，返回 `ProtocolConnection`：
/// - **简单协议**（如 Serial）：仅填充 `channel`，辅助资源留 None。
/// - **复合协议**（如 SSH）：额外提供 `side_channel`（如 SSH Session 供 SFTP 复用）。
#[async_trait::async_trait]
pub trait ProtocolAdapter: Send + Sync {
    /// 创建连接，返回完整的连接产物（含辅助资源）。
    ///
    /// 所有协议必须实现此方法：
    /// - `channel` — I/O 通道（必填，`ChannelKind::Sync` 或 `ChannelKind::Async`）
    /// - `comm_handle` — 协议特定通信句柄（可选，None 表示使用调用方默认实现）
    /// - `side_channel` — 侧通道资源（可选，如 SSH Session 供 SFTP 复用）
    async fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError>;

    /// 枚举可用端点
    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(Vec::new())
    }

    /// 内容类型（由 SerialAdapter 覆盖，其他插件通过默认实现返回 Terminal）
    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    /// 支持的传输协议列表
    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        Vec::new()
    }

    /// I/O 策略
    /// - `Sync`：串口等阻塞式传输，由 `spawn_sync_io_loop` 驱动
    /// - `Async`：SSH（russh）等 tokio 协议，由 `spawn_async_io_loop` 驱动
    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
    }

    /// 会话关闭后、资源完全释放前所需的额外等待时间。
    ///
    /// 某些协议（如串口）在 I/O 线程 join 后需要短暂等待，确保底层驱动释放端口，
    /// 避免立即重连时端口仍被占用。默认为 0（无需等待），由需要的插件覆盖。
    fn teardown_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// 为此协议的连接创建文件传输处理器
    ///
    /// 默认从 `connection.side_channel` 委托。插件可覆盖以提供自定义逻辑。
    #[allow(dead_code)]
    fn create_file_transfer(
        &self,
        connection: &ProtocolConnection,
    ) -> Option<Arc<dyn FileTransfer>> {
        connection
            .side_channel
            .as_ref()
            .and_then(|sc| sc.create_file_transfer())
    }
}
