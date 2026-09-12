use thiserror::Error;

use crate::transport::TransportError;

/// Session/protocol orchestration error. Transport failures remain structured at their source and
/// are wrapped instead of being flattened into strings inside drivers.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{reason}")]
    ConnectionFailed { reason: String },
    #[error("认证失败: {reason}")]
    AuthFailed { reason: String },
    #[error("插件 '{0}' 不存在")]
    PluginNotFound(String),
    #[error("能力被拒绝: 插件缺少 '{capability}' 能力")]
    CapabilityDenied { capability: String },
    #[error("操作超时")]
    Timeout,
    #[error("传输错误: {0}")]
    Transport(#[from] TransportError),
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("序列化错误: {0}")]
    Serialization(String),
    #[error("参数无效: {0}")]
    InvalidParameter(String),
    #[error("{0}")]
    Other(String),
}
