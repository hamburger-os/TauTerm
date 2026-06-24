//! 协议适配器 trait 和插件清单类型
//!
//! 定义 `ProtocolAdapter` trait——任何协议插件必须实现此 trait。
//! 定义 `PluginManifest`——插件的元数据描述。

use serde::{Deserialize, Serialize};
use crate::channel::{Channel, ContentType, IoStrategy};
use crate::channel::error::SessionError;

/// 端点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub name: String,
    pub description: String,
}

/// 传输协议标识
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransferProtocolType {
    YModem,
    XModem,
    ZModem,
    Sftp,
    Scp,
    Ftp,
}

/// 字符串解析为 TransferProtocolType
///
/// 支持大小写不敏感: "ymodem", "YMODEM", "YModem" 等
impl std::str::FromStr for TransferProtocolType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ymodem" => Ok(TransferProtocolType::YModem),
            "xmodem" => Ok(TransferProtocolType::XModem),
            "zmodem" => Ok(TransferProtocolType::ZModem),
            "sftp" => Ok(TransferProtocolType::Sftp),
            "scp" => Ok(TransferProtocolType::Scp),
            "ftp" => Ok(TransferProtocolType::Ftp),
            other => Err(format!("不支持的传输协议: '{}'。支持: ymodem, xmodem, zmodem, sftp, scp, ftp", other)),
        }
    }
}

impl std::fmt::Display for TransferProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = format!("{:?}", self).to_lowercase();
        write!(f, "{}", s)
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
pub trait ProtocolAdapter: Send + Sync {
    /// 创建连接，返回双向 I/O 通道
    fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<Box<dyn Channel>, SessionError>;

    /// 断开连接，清理资源
    #[allow(dead_code)]
    fn disconnect(&self, _channel: &mut Box<dyn Channel>) -> Result<(), SessionError> {
        Ok(())
    }

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

    /// I/O 策略（由 SerialAdapter 覆盖返回 Sync，网络插件覆盖返回 Async）
    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
    }
}
