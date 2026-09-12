use std::sync::Arc;

use crate::kernel::charset::transcode_utf8_to_encoding;
use crate::transport::{DataPlaneHandle, DataPlaneSubscription, ExclusiveIo, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum SessionIoError {
    #[error("当前会话没有可写数据面")]
    NoPrimaryDataPlane,
    #[error("当前会话不支持按目标地址发送")]
    TargetedSendUnsupported,
    // The frontend owns the user-facing "发送失败" context. Keep the transport detail raw here
    // so IPC errors do not render as "发送失败: 发送失败: ...".
    #[error("{0}")]
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

    /// Confirm shared-mode bytes synchronously. This is intended for worker/internal callers that
    /// may block until the transport actor finishes the physical write. Tauri/WebView commands must
    /// use [`SessionIo::send_async`] instead.
    pub fn send(&self, data: &[u8]) -> Result<(), SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .write(data)
            .map_err(Into::into)
    }

    /// Confirm shared-mode bytes asynchronously. The returned future resolves only after the
    /// transport actor has executed the physical write, without blocking the Tauri command thread.
    pub async fn send_async(&self, data: Vec<u8>) -> Result<(), SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .write_async(data)
            .await
            .map_err(Into::into)
    }

    fn encode_text(&self, data: &[u8]) -> Vec<u8> {
        if self.encoding.eq_ignore_ascii_case("utf-8") {
            data.to_vec()
        } else {
            transcode_utf8_to_encoding(data, &self.encoding).unwrap_or_else(|| data.to_vec())
        }
    }

    /// Encode UTF-8 application text using the session encoding and return the exact bytes after a
    /// confirmed synchronous write.
    pub fn send_text(&self, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        let out = self.encode_text(data);
        self.send(&out)?;
        Ok(out)
    }

    /// Encode UTF-8 application text and asynchronously await the confirmed physical write. The
    /// exact encoded bytes are returned so frontend TX rendering/logging matches the wire payload.
    pub async fn send_text_async(&self, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        let out = self.encode_text(data);
        self.send_async(out.clone()).await?;
        Ok(out)
    }

    pub fn send_to(&self, target: &str, data: &[u8]) -> Result<(), SessionIoError> {
        self.targeted
            .as_ref()
            .ok_or(SessionIoError::TargetedSendUnsupported)?
            .send_to(target, data)
    }

    pub fn send_to_text(&self, target: &str, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        let out = self.encode_text(data);
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

    pub async fn resize_terminal_async(&self, cols: u32, rows: u32) -> Result<(), SessionIoError> {
        self.primary
            .as_ref()
            .ok_or(SessionIoError::NoPrimaryDataPlane)?
            .resize_terminal_async(cols, rows)
            .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{TransportError, TransportErrorKind};

    #[test]
    fn transport_send_error_does_not_duplicate_frontend_context() {
        let error: SessionIoError = TransportError::new(
            TransportErrorKind::RemoteClosed,
            "write",
            "transport runtime is closed",
        )
        .into();

        assert_eq!(error.to_string(), "write: transport runtime is closed");
        assert!(!error.to_string().starts_with("发送失败:"));
    }
}
