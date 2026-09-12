use serde::Serialize;

/// Stable transport failure classes. UI/localization and protocol layers must match this enum,
/// never parse human-readable error strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportErrorKind {
    Resolve,
    Connect,
    Bind,
    Timeout,
    PermissionDenied,
    DeviceNotFound,
    DeviceBusy,
    RemoteClosed,
    ConnectionReset,
    Cancelled,
    Busy,
    Unsupported,
    InvalidConfiguration,
    Io,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{operation}: {message}")]
pub struct TransportError {
    pub kind: TransportErrorKind,
    pub operation: &'static str,
    pub message: String,
}

impl TransportError {
    pub fn new(
        kind: TransportErrorKind,
        operation: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            message: message.into(),
        }
    }

    pub fn io(operation: &'static str, error: std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                TransportErrorKind::Timeout
            }
            std::io::ErrorKind::PermissionDenied => TransportErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound => TransportErrorKind::DeviceNotFound,
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                TransportErrorKind::ConnectionReset
            }
            std::io::ErrorKind::UnexpectedEof => TransportErrorKind::RemoteClosed,
            _ => TransportErrorKind::Io,
        };
        Self::new(kind, operation, error.to_string())
    }

    pub fn busy(operation: &'static str, message: impl Into<String>) -> Self {
        Self::new(TransportErrorKind::Busy, operation, message)
    }

    pub fn unsupported(operation: &'static str, message: impl Into<String>) -> Self {
        Self::new(TransportErrorKind::Unsupported, operation, message)
    }
}
