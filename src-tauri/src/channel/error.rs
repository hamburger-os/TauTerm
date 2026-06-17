//! 通道和会话错误类型
//!
//! 新架构结构化错误枚举。

use thiserror::Error;

/// 通道错误
#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[allow(dead_code)]
    #[error("通道已断开")]
    Disconnected,

    #[allow(dead_code)]
    #[error("超时")]
    Timeout,

    #[allow(dead_code)]
    #[error("不支持的操作")]
    Unsupported,
}

/// 会话错误（结构化错误枚举）
///
/// 所有变体供 ProtocolAdapter 实现使用。
/// `ConnectionFailed` 由 SerialAdapter 使用，其余变体供未来协议插件（SSH/Telnet 等）使用。
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("连接失败: {reason}")]
    ConnectionFailed { reason: String },

    #[error("认证失败: {reason}")]
    AuthFailed { reason: String },

    #[error("插件 '{0}' 不存在")]
    PluginNotFound(String),

    #[error("能力被拒绝: 插件缺少 '{capability}' 能力")]
    CapabilityDenied { capability: String },

    #[error("操作超时")]
    Timeout,

    #[error("I/O 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("通道错误: {0}")]
    ChannelError(#[from] ChannelError),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("序列化错误: {0}")]
    Serialization(String),

    #[error("参数无效: {0}")]
    InvalidParameter(String),

    #[error("{0}")]
    Other(String),
}
