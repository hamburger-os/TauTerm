use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpConnectConfig {
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: u64,
    #[serde(default = "default_nodelay")]
    pub nodelay: bool,
}

impl Default for TcpConnectConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            read_timeout_ms: default_read_timeout_ms(),
            nodelay: default_nodelay(),
        }
    }
}

fn default_connect_timeout_ms() -> u64 { 5_000 }
fn default_read_timeout_ms() -> u64 { 20 }
fn default_nodelay() -> bool { true }

pub fn resolve_tcp(host: &str, port: u16) -> Result<Vec<SocketAddr>, TransportError> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|error| TransportError::new(TransportErrorKind::Resolve, "tcp_resolve", error.to_string()))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(TransportError::new(
            TransportErrorKind::Resolve,
            "tcp_resolve",
            format!("no address resolved for {host}:{port}"),
        ));
    }
    Ok(addrs)
}

pub fn connect_tcp(
    host: &str,
    port: u16,
    config: &TcpConnectConfig,
) -> Result<TcpDriver, TransportError> {
    let addrs = resolve_tcp(host, port)?;
    let mut last_error = None;
    for address in addrs {
        match TcpStream::connect_timeout(
            &address,
            Duration::from_millis(config.connect_timeout_ms.clamp(1, 120_000)),
        ) {
            Ok(stream) => return TcpDriver::from_stream(stream, config),
            Err(error) => last_error = Some(error),
        }
    }
    Err(TransportError::io(
        "tcp_connect",
        last_error.unwrap_or_else(|| std::io::Error::other("connection failed")),
    ))
}

pub struct TcpDriver {
    stream: TcpStream,
}

impl TcpDriver {
    pub fn from_stream(stream: TcpStream, config: &TcpConnectConfig) -> Result<Self, TransportError> {
        stream
            .set_read_timeout(Some(Duration::from_millis(config.read_timeout_ms.clamp(1, 1000))))
            .map_err(|error| TransportError::io("tcp_set_read_timeout", error))?;
        stream
            .set_nodelay(config.nodelay)
            .map_err(|error| TransportError::io("tcp_set_nodelay", error))?;
        Ok(Self { stream })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.stream.local_addr().map_err(|error| TransportError::io("tcp_local_addr", error))
    }

    pub fn peer_addr(&self) -> Result<SocketAddr, TransportError> {
        self.stream.peer_addr().map_err(|error| TransportError::io("tcp_peer_addr", error))
    }
}

impl BlockingByteStream for TcpDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        match self.stream.read(buf) {
            Ok(0) => Ok(ReadStatus::Eof),
            Ok(n) => Ok(ReadStatus::Data(n)),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(ReadStatus::Idle)
            }
            Err(error) => Err(TransportError::io("tcp_read", error)),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.stream
            .write_all(data)
            .map_err(|error| TransportError::io("tcp_write", error))
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        self.stream
            .flush()
            .map_err(|error| TransportError::io("tcp_flush", error))
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        match self.stream.shutdown(std::net::Shutdown::Both) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => Ok(()),
            Err(error) => Err(TransportError::io("tcp_shutdown", error)),
        }
    }
}

pub struct TcpListenerTransport {
    listener: TcpListener,
    client_config: TcpConnectConfig,
}

impl TcpListenerTransport {
    pub fn bind(
        host: &str,
        port: u16,
        client_config: TcpConnectConfig,
    ) -> Result<Self, TransportError> {
        let listener = TcpListener::bind((host, port))
            .map_err(|error| TransportError::io("tcp_bind", error))?;
        listener
            .set_nonblocking(false)
            .map_err(|error| TransportError::io("tcp_listener_mode", error))?;
        Ok(Self { listener, client_config })
    }

    pub fn accept(&self) -> Result<(TcpDriver, SocketAddr), TransportError> {
        let (stream, peer) = self
            .listener
            .accept()
            .map_err(|error| TransportError::io("tcp_accept", error))?;
        let driver = TcpDriver::from_stream(stream, &self.client_config)?;
        Ok((driver, peer))
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.listener.local_addr().map_err(|error| TransportError::io("tcp_listener_local_addr", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_an_invalid_service_fails_structurally() {
        let error = resolve_tcp("invalid host name with spaces", 502).unwrap_err();
        assert_eq!(error.kind, TransportErrorKind::Resolve);
    }
}
