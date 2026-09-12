use std::sync::Arc;

use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::transport::{
    DataPlaneHandle, DataPlaneSubscription, ExclusiveIo, TransportError,
};

#[derive(Debug, thiserror::Error)]
pub enum SessionIoError {
    #[error("当前会话没有可写数据面")]
    NoPrimaryDataPlane,
    #[error("当前会话不支持按目标地址发送")]
    TargetedSendUnsupported,
    #[error("发送失败: {0}")]
    Send(String),
}

impl From<TransportError> for SessionIoError {
    fn from(error: TransportError) -> Self {
        Self::Send(error.to_string())
    }
}

/// Optional capability for datagram/multi-peer sessions. Ordinary streams do not implement it.
pub trait TargetedIo: Send + Sync {
    fn send_to(&self, target: &str, data: &[u8]) -> Result<(), SessionIoError>;
}

/// Session-facing I/O capability composed from the canonical DataPlane plus optional targeted
/// addressing. This type deliberately has no receive callback registry: every consumer owns an
/// independent DataPlane subscription.
#[derive(Clone)]
pub struct SessionIo {
    primary: Option<DataPlaneHandle>,
    targeted: Option<Arc<dyn TargetedIo>>,
    encoding: Arc<str>,
}

impl SessionIo {
    pub fn new(
        primary: Option<DataPlaneHandle>,
        targeted: Option<Arc<dyn TargetedIo>>,
        encoding: impl Into<String>,
    ) -> Self {
        Self {
            primary,
            targeted,
            encoding: Arc::<str>::from(encoding.into()),
        }
    }

    pub fn primary(&self) -> Option<&DataPlaneHandle> {
        self.primary.as_ref()
    }

    pub fn subscribe(&self) -> Result<DataPlaneSubscription, SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .subscribe()
            .map_err(Into::into)
    }

    pub fn send(&self, data: &[u8]) -> Result<(), SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .write(data)
            .map_err(Into::into)
    }

    /// Encode UTF-8 application text using the session encoding and return the exact bytes written.
    pub fn send_text(&self, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        let out = if self.encoding.eq_ignore_ascii_case("utf-8") {
            data.to_vec()
        } else {
            transcode_utf8_to_encoding(data, &self.encoding).unwrap_or_else(|| data.to_vec())
        };
        self.send(&out)?;
        Ok(out)
    }

    pub fn send_to(&self, target: &str, data: &[u8]) -> Result<(), SessionIoError> {
        self.targeted
            .as_ref()
            .ok_or(SessionIoError::TargetedSendUnsupported)?
            .send_to(target, data)
    }

    pub fn send_to_text(&self, target: &str, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        let out = if self.encoding.eq_ignore_ascii_case("utf-8") {
            data.to_vec()
        } else {
            transcode_utf8_to_encoding(data, &self.encoding).unwrap_or_else(|| data.to_vec())
        };
        self.send_to(target, &out)?;
        Ok(out)
    }

    pub fn resize_terminal(&self, cols: u32, rows: u32) -> Result<(), SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .resize_terminal(cols, rows)
            .map_err(Into::into)
    }

    pub fn acquire_exclusive(
        &self,
        owner_name: impl Into<String>,
        purge_input: bool,
    ) -> Result<ExclusiveIo, SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .acquire_exclusive(owner_name, purge_input)
            .map_err(Into::into)
    }

    pub fn tx_bytes(&self) -> u64 {
        self.primary.as_ref().map_or(0, DataPlaneHandle::tx_bytes)
    }

    pub fn rx_bytes(&self) -> u64 {
        self.primary.as_ref().map_or(0, DataPlaneHandle::rx_bytes)
    }

    pub fn is_connected(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(DataPlaneHandle::is_connected)
    }
}
