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

impl TransferProtocolType {
    /// 构造规范化的传输协议标识。
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().to_lowercase())
    }

    /// 返回规范化后的传输协议标识。
    pub fn as_str(&self) -> &str {
        &self.0
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || !value
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        {
            return Err(format!("'{value}'（仅允许小写 a-z、0-9、-、_）"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for PluginId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for PluginId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PluginId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: PluginId,
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
    /// Adapter 自己声明稳定插件身份；PluginRuntime 在 bootstrap 时校验它与 manifest 一致。
    fn plugin_id(&self) -> Option<&'static str> {
        None
    }

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

    fn teardown_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
