use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};

pub struct UdpTransport {
    socket: UdpSocket,
}

impl UdpTransport {
    pub fn bind(host: &str, port: u16) -> Result<Self, TransportError> {
        let socket =
            UdpSocket::bind((host, port)).map_err(|error| TransportError::io("udp_bind", error))?;
        Ok(Self { socket })
    }

    pub fn set_read_timeout(&self, timeout: Duration) -> Result<(), TransportError> {
        self.socket
            .set_read_timeout(Some(timeout))
            .map_err(|error| TransportError::io("udp_set_read_timeout", error))
    }

    pub fn set_broadcast(&self, enabled: bool) -> Result<(), TransportError> {
        self.socket
            .set_broadcast(enabled)
            .map_err(|error| TransportError::io("udp_set_broadcast", error))
    }

    pub fn join_multicast_v4(
        &self,
        group: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> Result<(), TransportError> {
        self.socket
            .join_multicast_v4(&group, &interface)
            .map_err(|error| TransportError::io("udp_join_multicast", error))
    }

    pub fn set_multicast_ttl_v4(&self, ttl: u32) -> Result<(), TransportError> {
        self.socket
            .set_multicast_ttl_v4(ttl)
            .map_err(|error| TransportError::io("udp_set_multicast_ttl", error))
    }

    pub fn set_multicast_loop_v4(&self, enabled: bool) -> Result<(), TransportError> {
        self.socket
            .set_multicast_loop_v4(enabled)
            .map_err(|error| TransportError::io("udp_set_multicast_loop", error))
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<Option<(usize, SocketAddr)>, TransportError> {
        match self.socket.recv_from(buf) {
            Ok(result) => Ok(Some(result)),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(None)
            }
            Err(error) => Err(TransportError::io("udp_recv_from", error)),
        }
    }

    pub fn send_to(&self, data: &[u8], target: SocketAddr) -> Result<usize, TransportError> {
        self.socket
            .send_to(data, target)
            .map_err(|error| TransportError::io("udp_send_to", error))
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.socket
            .local_addr()
            .map_err(|error| TransportError::io("udp_local_addr", error))
    }

    pub fn try_clone(&self) -> Result<Self, TransportError> {
        self.socket
            .try_clone()
            .map(|socket| Self { socket })
            .map_err(|error| TransportError::io("udp_clone", error))
    }
}

pub fn resolve_udp(host: &str, port: u16) -> Result<SocketAddr, TransportError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| {
            TransportError::new(
                TransportErrorKind::Resolve,
                "udp_resolve",
                error.to_string(),
            )
        })?
        .next()
        .ok_or_else(|| {
            TransportError::new(
                TransportErrorKind::Resolve,
                "udp_resolve",
                format!("no address resolved for {host}:{port}"),
            )
        })
}
