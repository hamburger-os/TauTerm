//! VirtualPortBridge — Virtual serial port I/O bridge thread
//!
//! Runs in a dedicated std::thread:
//! 1. Opens all virtual port A's (COM10, COM12, …)
//! 2. Receives physical serial data from `data_rx` → broadcasts to all virtual ports
//! 3. Reads external software writes from each virtual port → sends to physical port
//!    via `write_tx` channel asynchronously
//!
//! ## Performance design
//!
//! - Virtual port read timeout set to 5ms (local kernel driver data arrives instantly)
//! - Physical port write-back uses a dedicated mpsc channel (avoids holding SessionStore
//!   Mutex inside the bridge loop)
//! - Read buffer reuse reduces heap allocations

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serialport::SerialPort;

/// Virtual port read timeout (milliseconds).
/// Local com0com kernel driver data arrives near-instantly; short timeout
/// prevents the bridge loop from accumulating latency across idle ports
/// in multi-port scenarios.
const VPORT_READ_TIMEOUT_MS: u64 = 5;

pub struct VirtualPortBridge {
    cancel_flag: Arc<AtomicBool>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
}

impl VirtualPortBridge {
    /// 启动桥接线程（使用 channel 异步写回）。
    ///
    /// * `virtual_port_names` — 虚拟端口 A 的名称列表（"COM10", "COM12", …）
    /// * `baud_rate` — 虚拟端口打开时的波特率（应与物理端口一致）
    /// * `data_rx` — 接收物理端口的输出数据，广播到所有虚拟端口
    /// * `write_tx` — 将虚拟端口收到的数据通过 channel 异步发送到物理端口写线程
    ///
    /// 桥接循环内使用 `try_send()` 非阻塞发送，避免因 SessionStore Mutex
    /// 争用而阻塞虚拟端口读取。
    pub fn spawn(
        virtual_port_names: Vec<String>,
        baud_rate: u32,
        data_rx: mpsc::Receiver<Vec<u8>>,
        write_tx: mpsc::SyncSender<Vec<u8>>,
    ) -> Self {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();

        let bridge_thread = std::thread::spawn(move || {
            bridge_loop(virtual_port_names, baud_rate, data_rx, write_tx, &cancel_clone);
        });

        Self {
            cancel_flag,
            bridge_thread: Some(bridge_thread),
        }
    }

    pub fn shutdown(mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(thread) = self.bridge_thread.take() {
            // Join with timeout (5 seconds) to prevent hanging on blocked I/O
            let start = std::time::Instant::now();
            loop {
                if thread.is_finished() {
                    match thread.join() {
                        Ok(()) => {}
                        Err(e) => {
                            // Bridge thread panicked — capture panic info for diagnostics
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
                    // JoinHandle drop 会 detach，cancel_flag 已设置，线程自行退出
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
        // Don't join — avoid deadlock during panic unwind.
        // shutdown() already took the JoinHandle via take();
        // if shutdown wasn't called, JoinHandle's Drop will detach the thread,
        // but cancel_flag is set so the thread will exit on its next loop iteration.
    }
}

fn bridge_loop(
    virtual_port_names: Vec<String>,
    baud_rate: u32,
    data_rx: mpsc::Receiver<Vec<u8>>,
    write_tx: mpsc::SyncSender<Vec<u8>>,
    cancel: &AtomicBool,
) {
    let mut virtual_ports: Vec<Box<dyn SerialPort>> = Vec::new();
    let timeout = Duration::from_millis(VPORT_READ_TIMEOUT_MS);
    for name in &virtual_port_names {
        match serialport::new(name, baud_rate)
            .timeout(timeout)
            .open()
        {
            Ok(port) => {
                let _ = port.clear(serialport::ClearBuffer::All);
                virtual_ports.push(port);
                log::info!("Virtual port {} opened (bridge)", name);
            }
            Err(e) => {
                log::error!("Failed to open virtual port {}: {}", name, e);
            }
        }
    }

    if virtual_ports.is_empty() {
        log::warn!("No virtual ports available, bridge thread exiting");
        return;
    }

    let mut read_buf = [0u8; 4096];

    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        // 1. Physical port data (from I/O loop → mpsc channel) → broadcast to virtual ports
        //    Use recv_timeout instead of try_recv + sleep; blocks when idle instead of spinning
        match data_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(data) => {
                // 批量写入所有虚拟端口：先 write_all，最后批量 flush
                for vport in &mut virtual_ports {
                    if vport.write_all(&data).is_err() {
                        log::trace!("Write to virtual port failed (peer closed)");
                    }
                }
                for vport in &mut virtual_ports {
                    let _ = vport.flush();
                }
                // Drain any remaining data buffered in the channel
                loop {
                    match data_rx.try_recv() {
                        Ok(data) => {
                            for vport in &mut virtual_ports {
                                if vport.write_all(&data).is_err() {
                                    log::trace!("Write to virtual port failed (peer closed)");
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
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 10ms 内无物理数据到达，继续检查虚拟端口读取
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("Data channel disconnected, bridge exiting");
                return;
            }
        }

        // 2. Virtual ports → physical port (external software writes forwarded back)
        //    Use try_send for non-blocking send: avoids blocking the bridge loop
        //    on SessionStore Mutex contention.
        //    Drop data when channel is full (best-effort): frame loss is better than
        //    blocking the bridge loop; the physical port recovers naturally.
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
                    // External software closing a virtual port is normal, don't abort
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
        fn new() -> Self { Self { buffer: Vec::new(), read_pos: 0 } }
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
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn test_bidirectional_logic() {
        // Simulate: write to physical → read on virtual
        let mut physical = MockPort::new();
        let mut virtual_a = MockPort::new();
        let mut buf = [0u8; 256];

        // physical receives data, forwards to virtual
        physical.buffer.extend_from_slice(b"HELLO");
        let n = physical.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"HELLO");
        virtual_a.write_all(&buf[..n]).unwrap();
        assert_eq!(&virtual_a.buffer, b"HELLO");

        // external software writes to virtual → forwarded to physical
        virtual_a.buffer.clear();
        virtual_a.buffer.extend_from_slice(b"WORLD");
        let n = virtual_a.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"WORLD");
        physical.buffer.clear();
        physical.write_all(&buf[..n]).unwrap();
        assert_eq!(&physical.buffer, b"WORLD");
    }
}
