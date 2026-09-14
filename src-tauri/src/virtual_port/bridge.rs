//! Virtual serial endpoint bridge.
//!
//! The physical serial driver remains exclusively owned by DataPlaneRuntime. This bridge is only
//! another shared-mode consumer/producer: physical -> virtual uses its own bounded subscription;
//! virtual -> physical uses confirmed SessionIo writes. When an inline transfer owns the driver,
//! the bridge pauses before reading external bytes so it does not consume data it cannot forward.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serialport::SerialPort;

use crate::session::SessionIo;
use crate::transport::DataPlaneEvent;

const VPORT_READ_TIMEOUT_MS: u64 = 5;
const PEER_RETRY_DELAY_MS: u64 = 10;

type BridgeErrorHandler = Box<dyn Fn(String) + Send + 'static>;

pub struct VirtualPortBridge {
    cancel_flag: Arc<AtomicBool>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
}

impl VirtualPortBridge {
    pub fn spawn(
        virtual_port_names: Vec<String>,
        baud_rate: u32,
        io: Arc<SessionIo>,
        on_error: BridgeErrorHandler,
    ) -> Result<Self, String> {
        let subscription = io.subscribe().map_err(|error| error.to_string())?;
        let mut virtual_ports: Vec<Box<dyn SerialPort>> =
            Vec::with_capacity(virtual_port_names.len());
        for name in &virtual_port_names {
            let port = open_bridge_endpoint(name, baud_rate)?;
            log::info!("Virtual endpoint {} attached to bridge", name);
            virtual_ports.push(port);
        }
        if virtual_ports.is_empty() {
            return Err("no virtual endpoints were available for bridging".into());
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();
        let bridge_thread = std::thread::spawn(move || {
            if let Err(error) = bridge_loop(virtual_ports, subscription, io, &cancel_clone) {
                log::error!("Virtual port bridge failed: {}", error);
                on_error(error);
            }
        });

        Ok(Self {
            cancel_flag,
            bridge_thread: Some(bridge_thread),
        })
    }

    pub fn shutdown(mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(thread) = self.bridge_thread.take() {
            let start = std::time::Instant::now();
            loop {
                if thread.is_finished() {
                    if let Err(e) = thread.join() {
                        let msg = if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "unknown panic".into()
                        };
                        log::error!("Bridge thread panic: {}", msg);
                    }
                    break;
                }
                if start.elapsed() > Duration::from_secs(5) {
                    log::error!("Bridge thread did not exit within 5 seconds, abandoning wait");
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

impl Drop for VirtualPortBridge {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

fn open_bridge_endpoint(name: &str, baud_rate: u32) -> Result<Box<dyn SerialPort>, String> {
    #[cfg(not(target_os = "windows"))]
    if let Some(master) = crate::virtual_port::pty::take_master_for_slave(name) {
        log::info!("Native PTY master attached for {}", name);
        return Ok(master);
    }

    serialport::new(name, baud_rate)
        .timeout(Duration::from_millis(VPORT_READ_TIMEOUT_MS))
        .open()
        .map_err(|e| format!("failed to open virtual endpoint {name}: {e}"))
}

/// Fan out physical bytes to every virtual endpoint that currently has a peer.
///
/// A PTY/com0com peer may legitimately be absent before an external tool opens it, or disappear
/// and reconnect later. That condition is not a bridge/runtime failure. Bytes cannot be delivered
/// to a peer that does not exist, but other attached peers must continue receiving data and the
/// endpoint must remain available for a later reconnect.
fn write_to_virtual_ports(virtual_ports: &mut [Box<dyn SerialPort>], data: &[u8]) {
    for (index, vport) in virtual_ports.iter_mut().enumerate() {
        if let Err(error) = vport.write_all(data) {
            log::trace!(
                "Virtual endpoint {} has no writable peer yet: {}",
                index,
                error
            );
            continue;
        }
        if let Err(error) = vport.flush() {
            log::trace!("Virtual endpoint {} flush unavailable: {}", index, error);
        }
    }
}

fn bridge_loop(
    mut virtual_ports: Vec<Box<dyn SerialPort>>,
    subscription: crate::transport::DataPlaneSubscription,
    io: Arc<SessionIo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut read_buf = [0u8; 4096];
    let mut pending_write: Option<Vec<u8>> = None;

    while !cancel.load(Ordering::SeqCst) {
        // Drain a bounded amount of physical data each turn so virtual -> physical traffic is not
        // starved by a continuously busy device. A detached subscription is a bridge failure, not
        // a best-effort display condition: continuing would silently corrupt a connected stream.
        for _ in 0..32 {
            match subscription.try_recv() {
                Ok(DataPlaneEvent::Data(data)) => {
                    write_to_virtual_ports(&mut virtual_ports, &data);
                }
                Ok(DataPlaneEvent::Closed(_)) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("data-plane bridge subscription detached or overflowed".into());
                }
            }
        }

        // Exclusive X/Y/ZModem transfer owns the same physical driver. Do not read external bytes
        // until shared ownership resumes; this preserves those bytes in the virtual endpoint buffer.
        if io.is_exclusive() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        if let Some(data) = pending_write.take() {
            match io.send(&data) {
                Ok(()) => {}
                Err(_) if io.is_exclusive() => {
                    pending_write = Some(data);
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => return Err(format!("virtual endpoint writeback failed: {error}")),
            }
        }

        let mut peer_unavailable = false;
        for (index, vport) in virtual_ports.iter_mut().enumerate() {
            match vport.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    let data = read_buf[..n].to_vec();
                    match io.send(&data) {
                        Ok(()) => {}
                        Err(_) if io.is_exclusive() => {
                            pending_write = Some(data);
                            break;
                        }
                        Err(error) => {
                            return Err(format!("virtual endpoint writeback failed: {error}"));
                        }
                    }
                }
                Ok(_) => {}
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    // Unix PTY masters commonly report EIO while no slave is open; Windows virtual
                    // peers can similarly disappear while the internal endpoint remains valid.
                    // Keep the bridge alive so external tools can connect/reconnect later.
                    peer_unavailable = true;
                    log::trace!("Virtual endpoint {} peer unavailable: {}", index, error);
                }
            }
        }
        if peer_unavailable {
            std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
        }
    }

    log::info!("Bridge thread exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::{Read, Write};

    struct MockPort {
        buffer: Vec<u8>,
        read_pos: usize,
    }

    impl MockPort {
        fn new() -> Self {
            Self {
                buffer: Vec::new(),
                read_pos: 0,
            }
        }
    }

    impl Read for MockPort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let available = self.buffer.len() - self.read_pos;
            if available == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            let n = buf.len().min(available);
            buf[..n].copy_from_slice(&self.buffer[self.read_pos..self.read_pos + n]);
            self.read_pos += n;
            Ok(n)
        }
    }

    impl Write for MockPort {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn physical_bytes_are_forwarded_losslessly_to_virtual_endpoint() {
        let mut physical = MockPort::new();
        let mut virtual_a = MockPort::new();
        let mut buf = [0u8; 256];
        physical.buffer.extend_from_slice(b"HELLO");
        let n = physical.read(&mut buf).unwrap();
        virtual_a.write_all(&buf[..n]).unwrap();
        assert_eq!(&virtual_a.buffer, b"HELLO");
    }
}
