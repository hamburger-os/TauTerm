//! SSH byte-stream driver.
//!
//! russh is asynchronous, but that execution model is confined to the transport layer.
//! Session/runtime code only sees a DataPlane handle.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::plugins::ssh::handler::SshHandler;
use crate::transport::{
    AsyncByteStream, ReadStatus, StreamCloseMetadata, TransportError, TransportErrorKind,
};

pub struct SshDriver {
    channel: russh::Channel<russh::client::Msg>,
    _handle: Arc<russh::client::Handle<SshHandler>>,
    pending: VecDeque<u8>,
    exit_status: Option<u32>,
    exit_signal: Option<String>,
    remote_eof: bool,
    closed: bool,
}

impl SshDriver {
    pub fn new(
        channel: russh::Channel<russh::client::Msg>,
        handle: Arc<russh::client::Handle<SshHandler>>,
    ) -> Self {
        Self {
            channel,
            _handle: handle,
            pending: VecDeque::new(),
            exit_status: None,
            exit_signal: None,
            remote_eof: false,
            closed: false,
        }
    }

    fn drain_pending(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.pending.len());
        for slot in &mut buf[..n] {
            *slot = self.pending.pop_front().expect("pending length checked");
        }
        n
    }

    fn deliver_chunk(&mut self, chunk: &[u8], buf: &mut [u8]) -> ReadStatus {
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        self.pending.extend(&chunk[n..]);
        ReadStatus::Data(n)
    }
}

#[async_trait::async_trait]
impl AsyncByteStream for SshDriver {
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        if buf.is_empty() {
            return Ok(ReadStatus::Idle);
        }
        if !self.pending.is_empty() {
            return Ok(ReadStatus::Data(self.drain_pending(buf)));
        }
        if self.remote_eof || self.closed {
            return Ok(ReadStatus::Eof);
        }

        loop {
            match self.channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { data, ext: 1 }) => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { .. }) => continue,
                Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                    self.exit_status = Some(exit_status);
                }
                Some(russh::ChannelMsg::ExitSignal { signal_name, .. }) => {
                    self.exit_signal = Some(format!("{signal_name:?}"));
                }
                Some(russh::ChannelMsg::Eof) => {
                    // SSH EOF is directional: the peer has finished sending data but the channel
                    // itself is not fully closed. Preserve that distinction so shutdown() can still
                    // complete our EOF/Close side of the channel handshake.
                    self.remote_eof = true;
                    return Ok(ReadStatus::Eof);
                }
                Some(russh::ChannelMsg::Close) | None => {
                    self.closed = true;
                    return Ok(ReadStatus::Eof);
                }
                Some(_) => {}
            }
        }
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::new(
                TransportErrorKind::RemoteClosed,
                "ssh_write",
                "SSH channel is already closed",
            ));
        }
        self.channel.data(data).await.map_err(|error| {
            TransportError::new(TransportErrorKind::Io, "ssh_write", error.to_string())
        })
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;

        // SSH channels have an explicit directional EOF followed by the channel close handshake.
        // Even when the remote side has already sent EOF, we still need to finish our side rather
        // than treating remote EOF as a fully closed channel.
        if let Err(error) = self.channel.eof().await {
            log::debug!("SSH channel EOF during shutdown failed: {error}");
        }
        self.channel.close().await.map_err(|error| {
            TransportError::new(TransportErrorKind::Io, "ssh_close", error.to_string())
        })
    }

    async fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::new(
                TransportErrorKind::RemoteClosed,
                "ssh_resize_terminal",
                "SSH channel is already closed",
            ));
        }
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|error| {
                TransportError::new(
                    TransportErrorKind::Io,
                    "ssh_resize_terminal",
                    error.to_string(),
                )
            })
    }

    fn supports_terminal_control(&self) -> bool {
        true
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        StreamCloseMetadata {
            exit_code: self.exit_status,
            signal: self.exit_signal.clone(),
        }
    }
}
