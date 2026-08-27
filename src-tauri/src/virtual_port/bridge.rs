//! VirtualPortBridge — virtual serial endpoint I/O bridge.
//!
//! Windows opens the internal side of each com0com pair by name. Linux/macOS
//! consume the already-created PTY master retained by the native PTY backend.
//! In both cases the bridge presents the same data flow:
//!
//! physical serial -> virtual endpoint(s)
//! virtual endpoint(s) -> physical serial

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serialport::SerialPort;

const VPORT_READ_TIMEOUT_MS: u64 = 5;

pub struct VirtualPortBridge {
    cancel_flag: Arc<AtomicBool>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
}

impl VirtualPortBridge {
    pub fn spawn(
        virtual_port_names: Vec<String>,
        baud_rate: u32,
        data_rx: mpsc::Receiver<Vec<u8>>,
        write_tx: mpsc::SyncSender<Vec<u8>>,
    ) -> Self {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();

        let bridge_thread = std::thread::spawn(move || {
            bridge_loop(
                virtual_port_names,
                baud_rate,
                data_rx,
                write_tx,
                &cancel_clone,
            );
        });

        Self {
            cancel_flag,
            bridge_thread: Some(bridge_thread),
        }
    }

    pub fn shutdown(mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(thread) = self.bridge_thread.take() {
            let start = std::time::Instant::now();
            loop {
                if thread.is_finished() {
                    match thread.join() {
                        Ok(()) => {}
                        Err(e) => {
                            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = e.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "unknown panic".into()
                            };
                            log::error!("Bridge thread panic: {}", msg);
                        }
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
    if let Some(master) = crate::virtual_port::socat::take_master_for_slave(name) {
        log::info!("Native PTY master attached for {}", name);
        return Ok(master);
    }

    serialport::new(name, baud_rate)
        .timeout(Duration::from_millis(VPORT_READ_TIMEOUT_MS))
        .open()
        .map_err(|e| format!("failed to open virtual endpoint {name}: {e}"))
}

fn bridge_loop(
    virtual_port_names: Vec<String>,
    baud_rate: u32,
    data_rx: mpsc::Receiver<Vec<u8>>,
    write_tx: mpsc::SyncSender<Vec<u8>>,
    cancel: &AtomicBool,
) {
    let mut virtual_ports: Vec<Box<dyn SerialPort>> = Vec::new();
    for name in &virtual_port_names {
        match open_bridge_endpoint(name, baud_rate) {
            Ok(port) => {
                let _ = port.clear(serialport::ClearBuffer::All);
                virtual_ports.push(port);
                log::info!("Virtual endpoint {} attached to bridge", name);
            }
            Err(e) => log::error!("{}", e),
        }
    }

    if virtual_ports.is_empty() {
        log::warn!("No virtual endpoints available, bridge thread exiting");
        return;
    }

    let mut read_buf = [0u8; 4096];

    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        match data_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(data) => {
                for vport in &mut virtual_ports {
                    if vport.write_all(&data).is_err() {
                        log::trace!("Write to virtual endpoint failed (peer closed)");
                    }
                }
                for vport in &mut virtual_ports {
                    let _ = vport.flush();
                }

                loop {
                    match data_rx.try_recv() {
                        Ok(data) => {
                            for vport in &mut virtual_ports {
                                if vport.write_all(&data).is_err() {
                                    log::trace!("Write to virtual endpoint failed (peer closed)");
                                }
                            }
                            for vport in &mut virtual_ports {
                                let _ = vport.flush();
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            log::info!("Data channel disconnected, bridge exiting");
                            return;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("Data channel disconnected, bridge exiting");
                return;
            }
        }

        for vport in &mut virtual_ports {
            match vport.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    if write_tx.try_send(read_buf[..n].to_vec()).is_err() {
                        log::trace!("Bridge write channel full, dropped {} bytes", n);
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => {
                    // External applications can disconnect/reconnect independently.
                }
            }
        }
    }

    log::info!("Bridge thread exited");
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
    fn test_bidirectional_logic() {
        let mut physical = MockPort::new();
        let mut virtual_a = MockPort::new();
        let mut buf = [0u8; 256];

        physical.buffer.extend_from_slice(b"HELLO");
        let n = physical.read(&mut buf).unwrap();
        virtual_a.write_all(&buf[..n]).unwrap();
        assert_eq!(&virtual_a.buffer, b"HELLO");

        virtual_a.buffer.clear();
        virtual_a.buffer.extend_from_slice(b"WORLD");
        let n = virtual_a.read(&mut buf).unwrap();
        physical.buffer.clear();
        physical.write_all(&buf[..n]).unwrap();
        assert_eq!(&physical.buffer, b"WORLD");
    }
}
