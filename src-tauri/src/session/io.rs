use std::sync::Arc;

use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::transport::{DataPlaneHandle, TransportError};

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

/// Script/send-bar facing capability composed from the canonical DataPlane plus optional targeted
/// addressing. This type has no receive callback registry: consumers subscribe to DataPlane events.
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
}
