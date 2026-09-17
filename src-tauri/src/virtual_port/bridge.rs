//! Virtual serial endpoint bridge.
//!
//! The physical serial driver remains exclusively owned by DataPlaneRuntime. Physical -> virtual
//! forwarding and virtual -> physical readers run in separate workers so an idle/stalled external
//! tool never delays the bounded DataPlane subscription. Virtual -> physical writes still go through
//! confirmed SessionIo, and readers pause while an inline transfer owns the physical driver.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::session::SessionIo;
use crate::transport::DataPlaneEvent;
use crate::virtual_port::backend::VirtualEndpoint;

const VPORT_READ_TIMEOUT_MS: u64 = 5;
const PEER_RETRY_DELAY_MS: u64 = 10;
const WORKER_POLL_MS: u64 = 20;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

type BridgeErrorHandler = Box<dyn Fn(String) + Send + 'static>;

struct BridgeEndpoint {
    external_path: String,
    port: Box<dyn SerialPort>,
}

pub struct VirtualPortBridge {
    cancel_flag: Arc<AtomicBool>,
    supervisor_thread: Option<JoinHandle<()>>,
    worker_threads: Vec<JoinHandle<()>>,
}

impl VirtualPortBridge {
    pub fn spawn(
        endpoints: Vec<VirtualEndpoint>,
        baud_rate: u32,
        io: Arc<SessionIo>,
        on_error: BridgeErrorHandler,
    ) -> Result<Self, String> {
        if endpoints.is_empty() {
            return Err("no virtual endpoints were available for bridging".into());
        }

        let subscription = io.subscribe().map_err(|error| error.to_string())?;
        let mut writer_endpoints = Vec::with_capacity(endpoints.len());
        let mut reader_endpoints = Vec::with_capacity(endpoints.len());

        for endpoint in endpoints {
            let reader = open_bridge_endpoint(&endpoint.bridge_path, baud_rate)?;
            let writer = reader.try_clone().map_err(|error| {
                format!(
                    "failed to clone virtual endpoint {} for independent bridge workers: {error}",
                    endpoint.external_path
                )
            })?;
            log::info!(
                "Virtual bridge attached for external endpoint {}",
                endpoint.external_path
            );
            writer_endpoints.push(BridgeEndpoint {
                external_path: endpoint.external_path.clone(),
                port: writer,
            });
            reader_endpoints.push(BridgeEndpoint {
                external_path: endpoint.external_path,
                port: reader,
            });
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (error_tx, error_rx) = mpsc::channel::<String>();
        let mut worker_threads = Vec::with_capacity(reader_endpoints.len() + 1);

        {
            let cancel = cancel_flag.clone();
            let errors = error_tx.clone();
            worker_threads.push(std::thread::spawn(move || {
                let result = physical_to_virtual_loop(writer_endpoints, subscription, &cancel);
                finish_worker(result, &cancel, &errors);
            }));
        }

        for endpoint in reader_endpoints {
            let cancel = cancel_flag.clone();
            let errors = error_tx.clone();
            let io = io.clone();
            worker_threads.push(std::thread::spawn(move || {
                let result = virtual_to_physical_loop(endpoint, io, &cancel);
                finish_worker(result, &cancel, &errors);
            }));
        }
        drop(error_tx);

        let supervisor_cancel = cancel_flag.clone();
        let supervisor_thread = std::thread::spawn(move || loop {
            match error_rx.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
                Ok(error) => {
                    supervisor_cancel.store(true, Ordering::SeqCst);
                    log::error!("Virtual port bridge failed: {error}");
                    on_error(error);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if supervisor_cancel.load(Ordering::SeqCst) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        });

        Ok(Self {
            cancel_flag,
            supervisor_thread: Some(supervisor_thread),
            worker_threads,
        })
    }

    pub fn shutdown(mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;

        if let Some(thread) = self.supervisor_thread.take() {
            join_bridge_thread("supervisor", thread, deadline);
        }
        for (index, thread) in self.worker_threads.drain(..).enumerate() {
            join_bridge_thread(&format!("worker-{index}"), thread, deadline);
        }
    }
}

impl Drop for VirtualPortBridge {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

fn join_bridge_thread(name: &str, thread: JoinHandle<()>, deadline: Instant) {
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            log::error!("Bridge {name} did not exit within shutdown deadline; detaching thread");
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Err(error) = thread.join() {
        let message = if let Some(message) = error.downcast_ref::<&str>() {
            message.to_string()
        } else if let Some(message) = error.downcast_ref::<String>() {
            message.clone()
        } else {
            "unknown panic".into()
        };
        log::error!("Bridge {name} thread panic: {message}");
    }
}

fn finish_worker(result: Result<(), String>, cancel: &AtomicBool, errors: &mpsc::Sender<String>) {
    if let Err(error) = result {
        if !cancel.load(Ordering::SeqCst) {
            // The supervisor owns the transition to cancelled. If a worker flips the flag here,
            // the supervisor can observe cancellation after a timeout before consuming this error
            // and exit without emitting `virtual-port-failed`.
            let _ = errors.send(error);
        }
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
        .map_err(|error| format!("failed to open virtual endpoint {name}: {error}"))
}

#[cfg(target_os = "windows")]
fn peer_is_open(endpoint: &mut BridgeEndpoint) -> Result<bool, String> {
    endpoint.port.read_data_set_ready().map_err(|error| {
        format!(
            "failed to query virtual peer {} presence: {error}",
            endpoint.external_path
        )
    })
}

#[cfg(not(target_os = "windows"))]
fn peer_is_open(_endpoint: &mut BridgeEndpoint) -> Result<bool, String> {
    // PTY masters expose peer absence through read/write EIO instead of modem-status pins.
    Ok(true)
}

fn write_bytes(writer: &mut dyn Write, data: &[u8]) -> std::io::Result<()> {
    writer.write_all(data)
}

/// Fan out physical bytes to every virtual endpoint that currently has a peer.
///
/// On Windows the internal com0com endpoint maps DSR to `ropen`, so a false DSR means the external
/// endpoint is not currently opened by another process. Bytes produced while no peer exists are not
/// historical backlog and are intentionally not queued. Once a peer exists, any write failure is a
/// bridge integrity failure rather than a best-effort drop.
fn write_to_virtual_ports(virtual_ports: &mut [BridgeEndpoint], data: &[u8]) -> Result<(), String> {
    for endpoint in virtual_ports.iter_mut() {
        if !peer_is_open(endpoint)? {
            continue;
        }

        if let Err(error) = write_bytes(endpoint.port.as_mut(), data) {
            #[cfg(target_os = "windows")]
            {
                return Err(format!(
                    "virtual peer {} write failed or stalled: {error}",
                    endpoint.external_path
                ));
            }
            #[cfg(not(target_os = "windows"))]
            {
                log::trace!(
                    "Virtual peer {} is not writable yet: {}",
                    endpoint.external_path,
                    error
                );
            }
        }
    }
    Ok(())
}

fn physical_to_virtual_loop(
    mut virtual_ports: Vec<BridgeEndpoint>,
    subscription: crate::transport::DataPlaneSubscription,
    cancel: &AtomicBool,
) -> Result<(), String> {
    while !cancel.load(Ordering::SeqCst) {
        match subscription.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
            Ok(DataPlaneEvent::Data(data)) => write_to_virtual_ports(&mut virtual_ports, &data)?,
            Ok(DataPlaneEvent::Closed(_)) => {
                cancel.store(true, Ordering::SeqCst);
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("data-plane bridge subscription detached or overflowed".into());
            }
        }
    }
    Ok(())
}

fn handle_virtual_read_error(
    endpoint: &mut BridgeEndpoint,
    error: &std::io::Error,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        if !peer_is_open(endpoint)? {
            log::trace!(
                "Virtual peer {} is currently closed: {}",
                endpoint.external_path,
                error
            );
            return Ok(true);
        }
        Err(format!(
            "virtual peer {} read failed while connected: {error}",
            endpoint.external_path
        ))
    }

    #[cfg(not(target_os = "windows"))]
    {
        log::trace!(
            "Virtual peer {} is currently unavailable: {}",
            endpoint.external_path,
            error
        );
        Ok(true)
    }
}

fn virtual_to_physical_loop(
    mut endpoint: BridgeEndpoint,
    io: Arc<SessionIo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut read_buf = [0u8; 4096];
    let mut pending_write: Option<Vec<u8>> = None;

    while !cancel.load(Ordering::SeqCst) {
        if io.is_exclusive() {
            std::thread::sleep(Duration::from_millis(VPORT_READ_TIMEOUT_MS));
            continue;
        }

        if let Some(data) = pending_write.take() {
            match io.send(&data) {
                Ok(()) => {}
                Err(_) if io.is_exclusive() => {
                    pending_write = Some(data);
                    std::thread::sleep(Duration::from_millis(VPORT_READ_TIMEOUT_MS));
                    continue;
                }
                Err(error) => return Err(format!("virtual endpoint writeback failed: {error}")),
            }
        }

        if !peer_is_open(&mut endpoint)? {
            std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
            continue;
        }

        match endpoint.port.read(&mut read_buf) {
            Ok(n) if n > 0 => {
                let data = read_buf[..n].to_vec();
                match io.send(&data) {
                    Ok(()) => {}
                    Err(_) if io.is_exclusive() => {
                        pending_write = Some(data);
                    }
                    Err(error) => {
                        return Err(format!("virtual endpoint writeback failed: {error}"));
                    }
                }
            }
            Ok(_) => {}
            Err(ref error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => {
                if handle_virtual_read_error(&mut endpoint, &error)? {
                    std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::{Read, Write};

    use super::{finish_worker, write_bytes};

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
            panic!("streaming bridge must not flush every physical chunk")
        }
    }

    #[test]
    fn worker_error_is_queued_before_supervisor_cancels_bridge() {
        let cancel = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        finish_worker(Err("boom".into()), &cancel, &tx);
        assert_eq!(rx.recv().unwrap(), "boom");
        assert!(!cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn physical_bytes_are_forwarded_losslessly_without_per_chunk_flush() {
        let mut physical = MockPort::new();
        let mut virtual_a = MockPort::new();
        let mut buf = [0u8; 256];
        physical.buffer.extend_from_slice(b"HELLO");
        let n = physical.read(&mut buf).unwrap();
        write_bytes(&mut virtual_a, &buf[..n]).unwrap();
        assert_eq!(&virtual_a.buffer, b"HELLO");
    }
}
