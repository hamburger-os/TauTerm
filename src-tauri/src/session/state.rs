use serde::{Deserialize, Serialize};

use crate::transport::{TransportCloseInfo, TransportErrorKind};

/// User-visible reason a live session stopped. This is a session concept rather than a byte-stream
/// trait concern: the transport reports the close class and the session decides terminal retention.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectKind {
    UserRequested,
    RemoteEof,
    IoError,
    DeviceRemoved,
    ProcessExited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectInfo {
    pub kind: DisconnectKind,
    pub reason: String,
    pub exit_code: Option<u32>,
    pub retain_terminal: bool,
}

impl DisconnectInfo {
    pub fn user_requested() -> Self {
        Self {
            kind: DisconnectKind::UserRequested,
            reason: "User requested disconnect".into(),
            exit_code: None,
            retain_terminal: false,
        }
    }

    pub fn remote_eof(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::RemoteEof,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn io_error(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::IoError,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn device_removed(reason: impl Into<String>) -> Self {
        Self {
            kind: DisconnectKind::DeviceRemoved,
            reason: reason.into(),
            exit_code: None,
            retain_terminal: true,
        }
    }

    pub fn process_exited(exit_code: u32, signal: Option<&str>) -> Self {
        let success = exit_code == 0 && signal.is_none();
        let reason = match signal {
            Some(signal) => format!("Local shell terminated by {signal}"),
            None if success => "Local shell exited normally".into(),
            None => format!("Local shell exited with code {exit_code}"),
        };
        Self {
            kind: DisconnectKind::ProcessExited,
            reason,
            exit_code: Some(exit_code),
            retain_terminal: !success,
        }
    }
}

impl From<TransportCloseInfo> for DisconnectInfo {
    fn from(info: TransportCloseInfo) -> Self {
        if let Some(exit_code) = info.exit_code {
            return DisconnectInfo::process_exited(exit_code, info.signal.as_deref());
        }
        let kind = match info.kind {
            TransportErrorKind::RemoteClosed => DisconnectKind::RemoteEof,
            TransportErrorKind::DeviceNotFound => DisconnectKind::DeviceRemoved,
            _ => DisconnectKind::IoError,
        };
        Self {
            kind,
            reason: info.reason,
            exit_code: None,
            retain_terminal: true,
        }
    }
}
