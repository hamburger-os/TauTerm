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
///
/// 插件用它维护自己的类型化 runtime registry；Session 核心只负责在注册成功后 attach，
/// 在关闭时 detach，不保存或解析任何协议具体类型。
pub trait SessionAttach: Send + Sync {
    fn on_attached(&self, session_id: &str);

    fn on_detached(&self, _session_id: &str) {}
}

/// 协议拥有的通用生命周期服务。
///
/// 这里只暴露 shutdown。协议特定命令必须从插件自己的类型化 registry 获取 runtime，
/// 不能通过 SessionStore 做 `Any` downcast。文件传输也作为独立 capability 显式暴露。
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

/// Backend rendering class. Frontend routing still uses the canonical manifest string; this enum
/// exists for adapter-level behavior without importing a transport implementation detail.
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

    pub fn is_serial_inline(&self) -> bool {
        matches!(self.0.as_str(), "ymodem" | "xmodem" | "zmodem")
    }

    pub fn is_auxiliary_transfer(&self) -> bool {
        self.0 == "sftp"
    }

    pub fn is_separate_connection(&self) -> bool {
        self.0 == "ftp"
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

    fn create_file_transfer(
        &self,
        connection: &ProtocolConnection,
    ) -> Option<Arc<dyn FileTransfer>> {
        connection.file_transfer.clone()
    }
}
