use crate::transport::error::TransportError;

/// A read operation must distinguish temporary idleness from a real stream EOF.
/// This removes the old sync/async ambiguity around `Ok(0)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus {
    Data(usize),
    Idle,
    Eof,
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
}
