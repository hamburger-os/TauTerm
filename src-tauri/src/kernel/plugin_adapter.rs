//! 协议适配器 trait 和插件清单类型。
//!
//! Adapter 只描述协议如何建立会话；物理 I/O 由 transport DataPlane 持有，SessionStore
//! 协议统一返回 DataPlaneRuntime，不再区分同步/异步上层 I/O 模型。

use crate::kernel::file_transfer::FileTransfer;
use crate::session::SessionError;
use crate::transport::DataPlaneRuntime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 协议运行时与最终 Session id 的绑定钩子。
pub trait SessionAttach: Send + Sync {
    fn on_attached(&self, session_id: &str);

    fn on_detached(&self, _session_id: &str) {}
}

/// 协议拥有的通用生命周期服务。
pub trait SessionService: Send + Sync {
    fn shutdown(&self) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Terminal,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelOpenMode {
    Standard,
    Elevated,
}

/// Factory for independent child terminal DataPlanes (SSH shells / Local Shell instances).
#[async_trait::async_trait]
pub trait SessionChannelFactory: Send + Sync {
    async fn open_channel(&self, mode: ChannelOpenMode) -> Result<DataPlaneRuntime, SessionError>;

    fn supports_mode(&self, mode: ChannelOpenMode) -> bool {
        mode == ChannelOpenMode::Standard
    }

    fn child_name_prefix(&self) -> &'static str;
}

/// Adapter connection product. A terminal session has a DataPlane; a headless/container protocol
/// can omit it and expose explicit lifecycle/file-transfer/factory capabilities instead.
pub struct ProtocolConnection {
    pub data_plane: Option<DataPlaneRuntime>,
    pub service: Option<Arc<dyn SessionService>>,
    pub file_transfer: Option<Arc<dyn FileTransfer>>,
    pub channel_factory: Option<Arc<dyn SessionChannelFactory>>,
    pub on_attached: Option<Arc<dyn SessionAttach>>,
    pub teardown_delay: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferProtocolType(String);

/// 文件传输协议如何取得 I/O 所有权。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferExecutionMode {
    /// 临时独占 Session 主 DataPlane，例如 X/Y/ZModem。
    Inline,
    /// 使用 Session 的独立 capability/channel，不阻塞主终端 I/O，例如 SFTP。
    Auxiliary,
    /// 建立独立连接执行传输，例如未来 FTP/FTPS provider。
    SeparateConnection,
}

/// 协议能力描述。执行策略与能力在一个注册表中声明，编排器不再按协议名分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferProtocolDescriptor {
    pub execution_mode: TransferExecutionMode,
    pub can_send: bool,
    pub can_receive: bool,
    pub batch: bool,
    pub directories: bool,
    pub overwrite_policy: bool,
    pub resume: bool,
}

impl TransferProtocolDescriptor {
    const fn serial_batch() -> Self {
        Self {
            execution_mode: TransferExecutionMode::Inline,
            can_send: true,
            can_receive: true,
            batch: true,
            directories: false,
            overwrite_policy: false,
            resume: false,
        }
    }

    const fn serial_single() -> Self {
        Self {
            batch: false,
            ..Self::serial_batch()
        }
    }

    const fn sftp() -> Self {
        Self {
            execution_mode: TransferExecutionMode::Auxiliary,
            can_send: true,
            can_receive: true,
            batch: true,
            directories: true,
            overwrite_policy: true,
            resume: false,
        }
    }

    const fn ftp() -> Self {
        Self {
            execution_mode: TransferExecutionMode::SeparateConnection,
            can_send: true,
            can_receive: true,
            batch: true,
            directories: true,
            overwrite_policy: true,
            resume: false,
        }
    }
}

impl TransferProtocolType {
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(s.as_ref().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn ymodem() -> Self {
        Self("ymodem".into())
    }
    pub fn xmodem() -> Self {
        Self("xmodem".into())
    }
    pub fn zmodem() -> Self {
        Self("zmodem".into())
    }
    pub fn sftp() -> Self {
        Self("sftp".into())
    }
    pub fn ftp() -> Self {
        Self("ftp".into())
    }

    /// 当前内建 provider 的单一能力注册表。
    ///
    /// 新传输协议在这里声明执行模式与能力；调用方只消费 descriptor，不应再次根据
    /// 协议字符串推导 Inline/Auxiliary/SeparateConnection。
    pub fn descriptor(&self) -> Option<TransferProtocolDescriptor> {
        match self.0.as_str() {
            "ymodem" => Some(TransferProtocolDescriptor::serial_batch()),
            "xmodem" => Some(TransferProtocolDescriptor::serial_single()),
            "zmodem" => Some(TransferProtocolDescriptor::serial_batch()),
            "sftp" => Some(TransferProtocolDescriptor::sftp()),
            "ftp" => Some(TransferProtocolDescriptor::ftp()),
            _ => None,
        }
    }

    /// 兼容现有调用点的能力查询；语义由 descriptor 单一来源决定。
    pub fn is_serial_inline(&self) -> bool {
        self.descriptor()
            .is_some_and(|value| value.execution_mode == TransferExecutionMode::Inline)
    }

    pub fn is_auxiliary_transfer(&self) -> bool {
        self.descriptor()
            .is_some_and(|value| value.execution_mode == TransferExecutionMode::Auxiliary)
    }

    pub fn is_separate_connection(&self) -> bool {
        self.descriptor()
            .is_some_and(|value| value.execution_mode == TransferExecutionMode::SeparateConnection)
    }
}

impl std::str::FromStr for TransferProtocolType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_lowercase();
        if lower.is_empty() {
            return Err("传输协议标识不能为空".into());
        }
        if !lower
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!("无效的传输协议标识: '{s}'（仅允许 a-z、0-9、_）"));
        }
        Ok(Self(lower))
    }
}

impl std::fmt::Display for TransferProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub description: String,
    pub icon: String,
    pub content_type: String,
    pub send_bar: bool,
    pub capabilities: Vec<String>,
    pub transfer_protocols: Vec<TransferProtocolType>,
}

#[async_trait::async_trait]
pub trait ProtocolAdapter: Send + Sync {
    async fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError>;

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(Vec::new())
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        Vec::new()
    }

    fn teardown_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
