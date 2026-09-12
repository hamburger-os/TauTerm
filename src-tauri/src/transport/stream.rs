use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};

const ASYNC_BRIDGE_READ_SLICE: Duration = Duration::from_millis(20);

/// A read operation must distinguish temporary idleness from a real stream EOF.
/// This removes the old sync/async ambiguity around `Ok(0)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus {
    Data(usize),
    Idle,
    Eof,
}

/// Optional process/channel metadata captured by terminal-like transports when the remote endpoint
/// exits. Keeping it in the transport contract preserves exit status without making the session
/// runtime depend on PTY or SSH implementation details.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamCloseMetadata {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

/// Blocking driver contract hidden inside the transport runtime.
///
/// Implementations may wrap serial ports, std TCP streams or blocking PTYs. Protocol/session
/// code never sees this trait directly; it receives a `DataPlaneHandle` instead.
pub trait BlockingByteStream: Send + 'static {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError>;
    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError>;
    fn flush(&mut self) -> Result<(), TransportError>;
    fn shutdown(&mut self) -> Result<(), TransportError>;

    /// Optional input purge used before ownership-sensitive protocols such as X/Y/ZModem.
    fn purge_input(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    /// Internal driver hook. The public capability is exposed separately by the session runtime.
    fn resize_terminal(&mut self, _cols: u32, _rows: u32) -> Result<(), TransportError> {
        Err(TransportError::unsupported(
            "resize_terminal",
            "transport has no terminal-control capability",
        ))
    }

    fn supports_terminal_control(&self) -> bool {
        false
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        StreamCloseMetadata::default()
    }
}

/// Async byte-stream driver contract used by protocol-native async transports such as SSH.
///
/// The async distinction is intentionally confined to the transport layer. `AsyncBridgeDriver`
/// adapts this contract into the same blocking actor used by every `DataPlaneHandle`, so session
/// and protocol orchestration never branch on sync/async I/O strategy.
#[async_trait::async_trait]
pub trait AsyncByteStream: Send + 'static {
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError>;
    async fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError>;
    async fn flush(&mut self) -> Result<(), TransportError>;
    async fn shutdown(&mut self) -> Result<(), TransportError>;

    async fn resize_terminal(&mut self, _cols: u32, _rows: u32) -> Result<(), TransportError> {
        Err(TransportError::unsupported(
            "resize_terminal",
            "transport has no terminal-control capability",
        ))
    }

    fn supports_terminal_control(&self) -> bool {
        false
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        StreamCloseMetadata::default()
    }
}

/// Bridges an async protocol-native stream into the blocking transport actor without leaking an
/// async/sync transport enum into higher layers. Reads are time-sliced so the actor can continue
/// servicing writes, resize requests and shutdown while the remote side is idle.
pub struct AsyncBridgeDriver {
    runtime: tokio::runtime::Runtime,
    inner: Box<dyn AsyncByteStream>,
    read_slice: Duration,
}

impl AsyncBridgeDriver {
    pub fn new(inner: Box<dyn AsyncByteStream>) -> Result<Self, TransportError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                TransportError::new(
                    TransportErrorKind::Io,
                    "async_bridge_runtime",
                    error.to_string(),
                )
            })?;
        Ok(Self {
            runtime,
            inner,
            // Keep idle SSH command latency aligned with the blocking TCP/serial actor cadence.
            // The timer is created only after entering this owned runtime (see read()).
            read_slice: ASYNC_BRIDGE_READ_SLICE,
        })
    }
}

impl BlockingByteStream for AsyncBridgeDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        let read_slice = self.read_slice;
        let runtime = &self.runtime;
        let inner = &mut self.inner;
        runtime.block_on(async {
            match tokio::time::timeout(read_slice, inner.read(buf)).await {
                Ok(result) => result,
                Err(_) => Ok(ReadStatus::Idle),
            }
        })
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        let runtime = &self.runtime;
        let inner = &mut self.inner;
        runtime.block_on(async { inner.write_all(data).await })
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        let runtime = &self.runtime;
        let inner = &mut self.inner;
        runtime.block_on(async { inner.flush().await })
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        let runtime = &self.runtime;
        let inner = &mut self.inner;
        runtime.block_on(async { inner.shutdown().await })
    }

    fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
        let runtime = &self.runtime;
        let inner = &mut self.inner;
        runtime.block_on(async { inner.resize_terminal(cols, rows).await })
    }

    fn supports_terminal_control(&self) -> bool {
        self.inner.supports_terminal_control()
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        self.inner.close_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PendingAsyncStream;

    #[async_trait::async_trait]
    impl AsyncByteStream for PendingAsyncStream {
        async fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::future::pending::<()>().await;
            Ok(ReadStatus::Idle)
        }

        async fn write_all(&mut self, _data: &[u8]) -> Result<(), TransportError> {
            tokio::task::yield_now().await;
            Ok(())
        }

        async fn flush(&mut self) -> Result<(), TransportError> {
            tokio::task::yield_now().await;
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), TransportError> {
            tokio::task::yield_now().await;
            Ok(())
        }

        async fn resize_terminal(&mut self, _cols: u32, _rows: u32) -> Result<(), TransportError> {
            tokio::task::yield_now().await;
            Ok(())
        }

        fn supports_terminal_control(&self) -> bool {
            true
        }
    }

    #[test]
    fn async_bridge_read_timeout_works_without_ambient_tokio_runtime() {
        let mut bridge = AsyncBridgeDriver::new(Box::new(PendingAsyncStream)).unwrap();
        bridge.read_slice = Duration::from_millis(1);
        let mut buf = [0u8; 8];

        assert_eq!(bridge.read(&mut buf).unwrap(), ReadStatus::Idle);
    }

    #[test]
    fn async_bridge_runs_all_driver_futures_inside_owned_runtime() {
        let mut bridge = AsyncBridgeDriver::new(Box::new(PendingAsyncStream)).unwrap();

        bridge.write_all(b"hello").unwrap();
        bridge.flush().unwrap();
        bridge.resize_terminal(120, 40).unwrap();
        bridge.shutdown().unwrap();
    }
}
