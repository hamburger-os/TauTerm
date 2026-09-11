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

        loop {
            match self.channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { .. }) => continue,
                Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                    self.exit_status = Some(exit_status);
                }
                Some(russh::ChannelMsg::ExitSignal { signal_name, .. }) => {
                    self.exit_signal = Some(format!("{signal_name:?}"));
                }
                Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                    return Ok(ReadStatus::Eof);
                }
                Some(_) => {}
            }
        }
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.channel.data(data).await.map_err(|error| {
            TransportError::new(TransportErrorKind::Io, "ssh_write", error.to_string())
        })
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
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
