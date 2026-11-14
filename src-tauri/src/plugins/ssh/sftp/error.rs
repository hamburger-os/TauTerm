//! SFTP 错误分类。
//!
//! russh-sftp 的传输错误与高层上下文在这里被归类为可判别的 `SftpErrorCode`；
//! 上层（文件服务、传输、远程文档、IPC 边界）只消费 code，不解析错误字符串。
//! 已知状态码由 `from_client` 直接映射；服务端把细分错误塞进通用 `Failure` 时
//! （例如 ELOOP 只体现在文案里），`from_client` 与服务包装层的 `From<String>`
//! 共用同一次关键词归类，保证同一种错误在两条路径上得到同一个 code。

use serde::Serialize;

/// 前端可判别的 SFTP 错误类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpErrorCode {
    /// 目标不存在。
    NotFound,
    /// 服务端拒绝访问（权限、只读文件系统）。
    PermissionDenied,
    /// 目标已存在且调用方要求不覆盖。
    Conflict,
    /// SFTP 通道/连接已经不可用，缓存会话必须失效。
    Closed,
    /// 单次请求超时；通道可能仍然存活，但按连接异常处理。
    Timeout,
    /// 服务端不支持该操作（例如缺少某个协议扩展）。
    Unsupported,
    /// 符号链接解析出现循环或层数过深（服务端 ELOOP / “too many levels”）。
    Loop,
    /// 其它 I/O 错误。
    Io,
    /// 协议/数据格式错误。
    Protocol,
}

/// SFTP 错误：code 是程序判断依据，message 是面向用户的说明。
#[derive(Debug, Clone, Serialize)]
pub struct SftpError {
    pub code: SftpErrorCode,
    pub message: String,
}

impl SftpError {
    pub fn new(code: SftpErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn closed(message: impl Into<String>) -> Self {
        Self::new(SftpErrorCode::Closed, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(SftpErrorCode::Unsupported, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(SftpErrorCode::Io, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(SftpErrorCode::Conflict, message)
    }

    /// 该错误是否意味着缓存的 SFTP 会话已经不可用、必须重建。
    ///
    /// I/O 错误按连接失效处理：SFTP 通道上出现 IO 错误几乎等价于通道断开，
    /// 重建会话比继续在坏通道上重试更可靠。
    pub fn is_connection_lost(&self) -> bool {
        matches!(
            self.code,
            SftpErrorCode::Closed | SftpErrorCode::Timeout | SftpErrorCode::Io
        )
    }

    /// 替换面向用户的 message（错误类别不变）。
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// 把 russh-sftp 客户端错误映射为可判别 code。
    pub fn from_client(error: &russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error as ClientError;
        use russh_sftp::protocol::StatusCode;

        match error {
            ClientError::Status(status) => match status.status_code {
                StatusCode::NoSuchFile => {
                    Self::new(SftpErrorCode::NotFound, status.error_message.clone())
                }
                StatusCode::PermissionDenied => Self::new(
                    SftpErrorCode::PermissionDenied,
                    status.error_message.clone(),
                ),
                StatusCode::NoConnection | StatusCode::ConnectionLost => {
                    Self::closed(status.error_message.clone())
                }
                StatusCode::OpUnsupported => {
                    Self::new(SftpErrorCode::Unsupported, status.error_message.clone())
                }
                StatusCode::BadMessage => {
                    Self::new(SftpErrorCode::Protocol, status.error_message.clone())
                }
                _ => Self::from(status.error_message.clone()),
            },
            ClientError::Timeout => Self::new(SftpErrorCode::Timeout, "SFTP 请求超时"),
            ClientError::IO(message) => Self::new(SftpErrorCode::Io, message.clone()),
            ClientError::Limited(message) => Self::new(SftpErrorCode::Io, message.clone()),
            ClientError::UnexpectedPacket => {
                Self::new(SftpErrorCode::Protocol, "SFTP 服务端返回了意外的数据包")
            }
            ClientError::UnexpectedBehavior(message) => {
                Self::new(SftpErrorCode::Protocol, message.clone())
            }
        }
    }
}

impl std::fmt::Display for SftpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SftpError {}

impl From<SftpError> for String {
    fn from(error: SftpError) -> Self {
        error.message
    }
}

/// 服务层错误的唯一入口。
///
/// 服务公共入口是 `*_inner(...) -> Result<_, String>` 的薄包装，内部辅助函数在拼接
/// “操作 + 上下文 + 底层错误”时仍是文本；这里按 russh-sftp 的英文状态描述（以及本项目
/// 固定中文文案的关键词）归类一次。`from_client` 用于 capability / 结构化路径：已知
/// 状态码直接映射，通用 `Failure` 复用同一关键词归类（服务端不把 ELOOP 等细分错误
/// 暴露成独立状态码，只能从文案判别）。修改错误文案时不得移除这些可判别关键词，
/// 否则 `closed` / `unsupported` / `loop` 会退化成 `io`。
impl From<String> for SftpError {
    fn from(message: String) -> Self {
        Self {
            code: classify_message(&message),
            message,
        }
    }
}

fn classify_message(message: &str) -> SftpErrorCode {
    let lower = message.to_ascii_lowercase();
    if lower.contains("no such file") || lower.contains("not found") || message.contains("不存在")
    {
        SftpErrorCode::NotFound
    } else if lower.contains("too many levels of symbolic links")
        || lower.contains("too many symbolic links")
        || message.contains("层数过深")
        || message.contains("链接循环")
    {
        SftpErrorCode::Loop
    } else if lower.contains("permission denied") || message.contains("权限") {
        SftpErrorCode::PermissionDenied
    } else if lower.contains("already exists")
        || message.contains("已存在")
        || message.contains("冲突")
    {
        SftpErrorCode::Conflict
    } else if lower.contains("timeout") || message.contains("超时") {
        SftpErrorCode::Timeout
    } else if lower.contains("connection") || lower.contains("closed") || message.contains("通道")
    {
        SftpErrorCode::Closed
    } else if lower.contains("subsystem")
        || message.contains("子系统")
        || message.contains("不支持")
    {
        SftpErrorCode::Unsupported
    } else {
        SftpErrorCode::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_timeout_io_force_session_rebuild() {
        assert!(SftpError::closed("x").is_connection_lost());
        assert!(SftpError::new(SftpErrorCode::Timeout, "x").is_connection_lost());
        assert!(SftpError::io("x").is_connection_lost());
        assert!(!SftpError::new(SftpErrorCode::NotFound, "x").is_connection_lost());
        assert!(!SftpError::new(SftpErrorCode::PermissionDenied, "x").is_connection_lost());
        assert!(!SftpError::conflict("x").is_connection_lost());
    }

    #[test]
    fn legacy_text_bridge_keeps_actionable_categories() {
        assert_eq!(
            SftpError::from("读取目录 /x 失败: No such file".to_string()).code,
            SftpErrorCode::NotFound
        );
        assert_eq!(
            SftpError::from("No such file".to_string()).code,
            SftpErrorCode::NotFound
        );
        assert_eq!(
            SftpError::from("Permission denied".to_string()).code,
            SftpErrorCode::PermissionDenied
        );
        assert_eq!(
            SftpError::from("远端文件已存在".to_string()).code,
            SftpErrorCode::Conflict
        );
        assert_eq!(
            SftpError::from("Connection lost".to_string()).code,
            SftpErrorCode::Closed
        );
    }

    #[test]
    fn ipc_error_payload_is_serializable_with_code_and_message() {
        let payload =
            serde_json::to_value(SftpError::new(SftpErrorCode::Conflict, "已存在")).unwrap();
        assert_eq!(payload["code"], "conflict");
        assert_eq!(payload["message"], "已存在");
    }

    #[test]
    fn symlink_loops_are_classified_as_loop_not_io() {
        assert_eq!(
            SftpError::from(
                "获取文件信息 '/a' 失败: Too many levels of symbolic links".to_string()
            )
            .code,
            SftpErrorCode::Loop
        );
        assert_eq!(
            SftpError::new(SftpErrorCode::Loop, "链接层数过深").code,
            SftpErrorCode::Loop
        );
    }

    #[test]
    fn generic_failure_status_keeps_actionable_classification() {
        use russh_sftp::client::error::Error as ClientError;
        use russh_sftp::protocol::{Status, StatusCode};

        let loop_status = Status {
            id: 0,
            status_code: StatusCode::Failure,
            error_message: "Too many levels of symbolic links".to_string(),
            language_tag: String::new(),
        };
        assert_eq!(
            SftpError::from_client(&ClientError::Status(loop_status)).code,
            SftpErrorCode::Loop
        );

        let missing_status = Status {
            id: 0,
            status_code: StatusCode::Failure,
            error_message: "No such file".to_string(),
            language_tag: String::new(),
        };
        assert_eq!(
            SftpError::from_client(&ClientError::Status(missing_status)).code,
            SftpErrorCode::NotFound
        );
    }
}
