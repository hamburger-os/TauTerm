//! Virtual serial endpoint bridge.
//!
//! Physical serial ownership remains inside DataPlaneRuntime. The physical -> virtual subscription
//! pump only performs bounded, non-blocking fan-out; every external endpoint owns an independent
//! writer actor and byte budget so one stalled peer cannot block the DataPlane or sibling endpoints.
//! Virtual -> physical readers remain per-endpoint workers and use confirmed SessionIo writes.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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
const EGRESS_QUEUE_MESSAGES: usize = 1024;
const EGRESS_BACKLOG_WINDOW_MS: u64 = 2_000;
const EGRESS_MIN_BYTES: usize = 64 * 1024;
const EGRESS_MAX_BYTES: usize = 1024 * 1024;
const WRITE_STALL_DEADLINE: Duration = Duration::from_secs(2);

const ENDPOINT_ACTIVE: u8 = 0;
const ENDPOINT_BACKPRESSURED: u8 = 1;

type BridgeEventHandler = Box<dyn Fn(VirtualPortBridgeEvent) + Send + 'static>;

#[derive(Debug, Clone)]
pub enum VirtualPortBridgeEvent {
    EndpointBackpressured {
        external_path: String,
        reason: String,
        queued_bytes: usize,
        backlog_limit_bytes: usize,
        stalled_for_ms: u64,
    },
    EndpointRecovered {
        external_path: String,
    },
    Fatal {
        reason: String,
    },
}

struct BridgeEndpoint {
    external_path: String,
    port: Box<dyn SerialPort>,
}

struct EndpointShared {
    external_path: String,
    peer_open: AtomicBool,
    state: AtomicU8,
    queued_bytes: AtomicUsize,
    backlog_limit_bytes: usize,
    last_progress: Mutex<Instant>,
    events: mpsc::Sender<VirtualPortBridgeEvent>,
}

impl EndpointShared {
    fn new(
        external_path: String,
        backlog_limit_bytes: usize,
        peer_open: bool,
        events: mpsc::Sender<VirtualPortBridgeEvent>,
    ) -> Self {
        Self {
            external_path,
            peer_open: AtomicBool::new(peer_open),
            state: AtomicU8::new(ENDPOINT_ACTIVE),
            queued_bytes: AtomicUsize::new(0),
            backlog_limit_bytes,
            last_progress: Mutex::new(Instant::now()),
            events,
        }
    }

    fn is_backpressured(&self) -> bool {
        self.state.load(Ordering::Acquire) == ENDPOINT_BACKPRESSURED
    }

    fn record_progress(&self) {
        let now = Instant::now();
        match self.last_progress.lock() {
            Ok(mut last) => *last = now,
            Err(poisoned) => *poisoned.into_inner() = now,
        }
    }

    fn stalled_for_ms(&self) -> u64 {
        let elapsed = match self.last_progress.lock() {
            Ok(last) => last.elapsed(),
            Err(poisoned) => poisoned.into_inner().elapsed(),
        };
        elapsed.as_millis().min(u64::MAX as u128) as u64
    }

    fn reserve_bytes(&self, bytes: usize) -> bool {
        self.queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|next| *next <= self.backlog_limit_bytes)
            })
            .is_ok()
    }

    fn release_bytes(&self, bytes: usize) {
        let _ = self
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                Some(current.saturating_sub(bytes))
            });
    }

    fn set_peer_open(&self, open: bool) {
        let previous = self.peer_open.swap(open, Ordering::AcqRel);
        if previous == open {
            return;
        }
        if open {
            log::info!("Virtual peer {} opened", self.external_path);
        } else {
            log::info!("Virtual peer {} closed", self.external_path);
        }
    }

    fn mark_backpressured(&self, reason: impl Into<String>) {
        if self
            .state
            .compare_exchange(
                ENDPOINT_ACTIVE,
                ENDPOINT_BACKPRESSURED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }

        let reason = reason.into();
        let queued_bytes = self.queued_bytes.load(Ordering::Acquire);
        let stalled_for_ms = self.stalled_for_ms();
        log::warn!(
            "Virtual peer {} backpressured: {}; queued_bytes={}, backlog_limit_bytes={}, stalled_for_ms={}",
            self.external_path,
            reason,
            queued_bytes,
            self.backlog_limit_bytes,
            stalled_for_ms
        );
        let _ = self
            .events
            .send(VirtualPortBridgeEvent::EndpointBackpressured {
                external_path: self.external_path.clone(),
                reason,
                queued_bytes,
                backlog_limit_bytes: self.backlog_limit_bytes,
                stalled_for_ms,
            });
    }

    fn recover_after_reopen(&self) {
        if self
            .state
            .compare_exchange(
                ENDPOINT_BACKPRESSURED,
                ENDPOINT_ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        self.queued_bytes.store(0, Ordering::Release);
        self.record_progress();
        log::info!(
            "Virtual peer {} recovered after close/reopen; starting a fresh stream",
            self.external_path
        );
        let _ = self.events.send(VirtualPortBridgeEvent::EndpointRecovered {
            external_path: self.external_path.clone(),
        });
    }
}

struct EgressTarget {
    sender: mpsc::SyncSender<Arc<[u8]>>,
    shared: Arc<EndpointShared>,
}

impl EgressTarget {
    fn enqueue(&self, data: Arc<[u8]>) -> Result<(), String> {
        if !self.shared.peer_open.load(Ordering::Acquire) || self.shared.is_backpressured() {
            return Ok(());
        }

        if !self.shared.reserve_bytes(data.len()) {
            self.shared.mark_backpressured(format!(
                "egress byte budget exhausted while peer remained open (incoming={} bytes)",
                data.len()
            ));
            return Ok(());
        }

        match self.sender.try_send(data) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(data)) => {
                self.shared.release_bytes(data.len());
                self.shared.mark_backpressured(
                    "egress message queue exhausted while peer remained open",
                );
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(data)) => {
                self.shared.release_bytes(data.len());
                Err(format!(
                    "virtual peer {} egress writer stopped unexpectedly",
                    self.shared.external_path
                ))
            }
        }
    }
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
        on_event: BridgeEventHandler,
    ) -> Result<Self, String> {
        if endpoints.is_empty() {
            return Err("no virtual endpoints were available for bridging".into());
        }

        let subscription = io
            .subscribe("virtual-port-bridge")
            .map_err(|error| error.to_string())?;
        let backlog_limit_bytes = egress_backlog_limit_bytes(baud_rate);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (event_tx, event_rx) = mpsc::channel::<VirtualPortBridgeEvent>();
        let mut worker_threads = Vec::with_capacity(endpoints.len() * 2 + 1);
        let mut egress_targets = Vec::with_capacity(endpoints.len());

        for endpoint in endpoints {
            let reader_port = open_bridge_endpoint(&endpoint.bridge_path, baud_rate)?;
            let writer_port = reader_port.try_clone().map_err(|error| {
                format!(
                    "failed to clone virtual endpoint {} for independent bridge workers: {error}",
                    endpoint.external_path
                )
            })?;
            let mut writer_endpoint = BridgeEndpoint {
                external_path: endpoint.external_path.clone(),
                port: writer_port,
            };
            let reader_endpoint = BridgeEndpoint {
                external_path: endpoint.external_path.clone(),
                port: reader_port,
            };

            #[cfg(target_os = "windows")]
            let initial_peer_open = match peer_is_open(&mut writer_endpoint) {
                Ok(open) => open,
                Err(error) => {
                    log::warn!(
                        "Virtual peer {} initial presence query failed: {}",
                        endpoint.external_path,
                        error
                    );
                    false
                }
            };
            #[cfg(not(target_os = "windows"))]
            let initial_peer_open = true;

            let shared = Arc::new(EndpointShared::new(
                endpoint.external_path.clone(),
                backlog_limit_bytes,
                initial_peer_open,
                event_tx.clone(),
            ));
            let (egress_tx, egress_rx) = mpsc::sync_channel(EGRESS_QUEUE_MESSAGES);
            egress_targets.push(EgressTarget {
                sender: egress_tx,
                shared: shared.clone(),
            });

            {
                let cancel = cancel_flag.clone();
                let shared = shared.clone();
                let events = event_tx.clone();
                worker_threads.push(std::thread::spawn(move || {
                    if let Err(reason) =
                        physical_to_virtual_writer_loop(writer_endpoint, egress_rx, &shared, &cancel)
                    {
                        let _ = events.send(VirtualPortBridgeEvent::Fatal { reason });
                    }
                }));
            }

            {
                let cancel = cancel_flag.clone();
                let shared = shared.clone();
                let events = event_tx.clone();
                let io = io.clone();
                worker_threads.push(std::thread::spawn(move || {
                    if let Err(reason) =
                        virtual_to_physical_loop(reader_endpoint, io, &shared, &cancel)
                    {
                        let _ = events.send(VirtualPortBridgeEvent::Fatal { reason });
                    }
                }));
            }

            log::info!(
                "Virtual bridge attached for external endpoint {} (egress_limit={} bytes)",
                endpoint.external_path,
                backlog_limit_bytes
            );
        }

        {
            let cancel = cancel_flag.clone();
            let events = event_tx.clone();
            worker_threads.push(std::thread::spawn(move || {
                if let Err(reason) = physical_subscription_pump(subscription, egress_targets, &cancel)
                {
                    let _ = events.send(VirtualPortBridgeEvent::Fatal { reason });
                }
            }));
        }
        drop(event_tx);

        let supervisor_cancel = cancel_flag.clone();
        let supervisor_thread = std::thread::spawn(move || loop {
            match event_rx.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
                Ok(VirtualPortBridgeEvent::Fatal { reason }) => {
                    supervisor_cancel.store(true, Ordering::SeqCst);
                    log::error!("Virtual port bridge failed: {reason}");
                    on_event(VirtualPortBridgeEvent::Fatal { reason });
                    break;
                }
                Ok(event) => on_event(event),
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

fn egress_backlog_limit_bytes(baud_rate: u32) -> usize {
    let bytes_per_second = (u64::from(baud_rate) / 10).max(1);
    let window_bytes = bytes_per_second.saturating_mul(EGRESS_BACKLOG_WINDOW_MS) / 1_000;
    usize::try_from(window_bytes)
        .unwrap_or(usize::MAX)
        .clamp(EGRESS_MIN_BYTES, EGRESS_MAX_BYTES)
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

fn fan_out_physical_chunk(targets: &[EgressTarget], data: Vec<u8>) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }
    let data: Arc<[u8]> = data.into();
    for target in targets {
        target.enqueue(data.clone())?;
    }
    Ok(())
}

fn physical_subscription_pump(
    subscription: crate::transport::DataPlaneSubscription,
    targets: Vec<EgressTarget>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    while !cancel.load(Ordering::SeqCst) {
        match subscription.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
            Ok(DataPlaneEvent::Data(data)) => fan_out_physical_chunk(&targets, data)?,
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

fn drain_egress_queue(receiver: &mpsc::Receiver<Arc<[u8]>>, shared: &EndpointShared) {
    while let Ok(data) = receiver.try_recv() {
        shared.release_bytes(data.len());
    }
}

enum WriteOutcome {
    Delivered,
    PeerUnavailable,
    Backpressured,
    Cancelled,
}

fn physical_to_virtual_writer_loop(
    mut endpoint: BridgeEndpoint,
    receiver: mpsc::Receiver<Arc<[u8]>>,
    shared: &EndpointShared,
    cancel: &AtomicBool,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut reopen_armed = false;

    while !cancel.load(Ordering::SeqCst) {
        #[cfg(target_os = "windows")]
        {
            let peer_open = match peer_is_open(&mut endpoint) {
                Ok(open) => open,
                Err(error) => {
                    shared.mark_backpressured(error);
                    drain_egress_queue(&receiver, shared);
                    std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                    continue;
                }
            };
            shared.set_peer_open(peer_open);
            if shared.is_backpressured() {
                drain_egress_queue(&receiver, shared);
                if !peer_open {
                    reopen_armed = true;
                } else if reopen_armed {
                    shared.recover_after_reopen();
                    reopen_armed = false;
                }
                std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                continue;
            }
            if !peer_open {
                drain_egress_queue(&receiver, shared);
                std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                continue;
            }
        }

        #[cfg(not(target_os = "windows"))]
        if shared.is_backpressured() {
            drain_egress_queue(&receiver, shared);
            std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
            continue;
        }

        let data = match receiver.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
            Ok(data) => data,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                return Err(format!(
                    "virtual peer {} egress queue disconnected unexpectedly",
                    endpoint.external_path
                ));
            }
        };
        shared.release_bytes(data.len());

        match write_chunk_to_peer(&mut endpoint, &data, shared, cancel)? {
            WriteOutcome::Delivered => {}
            WriteOutcome::PeerUnavailable => {
                drain_egress_queue(&receiver, shared);
                std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
            }
            WriteOutcome::Backpressured => {
                drain_egress_queue(&receiver, shared);
            }
            WriteOutcome::Cancelled => return Ok(()),
        }
    }
    Ok(())
}

fn write_chunk_to_peer(
    endpoint: &mut BridgeEndpoint,
    data: &[u8],
    shared: &EndpointShared,
    cancel: &AtomicBool,
) -> Result<WriteOutcome, String> {
    let mut offset = 0usize;
    let mut last_progress = Instant::now();

    while offset < data.len() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(WriteOutcome::Cancelled);
        }
        if shared.is_backpressured() {
            return Ok(WriteOutcome::Backpressured);
        }

        match endpoint.port.write(&data[offset..]) {
            Ok(0) => {}
            Ok(written) => {
                offset += written;
                last_progress = Instant::now();
                shared.record_progress();
                continue;
            }
            Err(error) => {
                #[cfg(target_os = "windows")]
                {
                    match peer_is_open(endpoint) {
                        Ok(false) => {
                            shared.set_peer_open(false);
                            return Ok(WriteOutcome::PeerUnavailable);
                        }
                        Ok(true) => {
                            if error.kind() != std::io::ErrorKind::TimedOut
                                && error.kind() != std::io::ErrorKind::WouldBlock
                            {
                                shared.mark_backpressured(format!(
                                    "write failed while peer remained open: {error}"
                                ));
                                return Ok(WriteOutcome::Backpressured);
                            }
                        }
                        Err(status_error) => {
                            shared.mark_backpressured(format!(
                                "write failed and peer presence could not be verified: {error}; {status_error}"
                            ));
                            return Ok(WriteOutcome::Backpressured);
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    log::trace!(
                        "Virtual peer {} is currently unavailable: {}",
                        endpoint.external_path,
                        error
                    );
                    return Ok(WriteOutcome::PeerUnavailable);
                }
            }
        }

        if last_progress.elapsed() >= WRITE_STALL_DEADLINE {
            #[cfg(target_os = "windows")]
            {
                match peer_is_open(endpoint) {
                    Ok(false) => {
                        shared.set_peer_open(false);
                        return Ok(WriteOutcome::PeerUnavailable);
                    }
                    Ok(true) => {
                        shared.mark_backpressured(format!(
                            "no write progress for {} ms while peer remained open",
                            WRITE_STALL_DEADLINE.as_millis()
                        ));
                        return Ok(WriteOutcome::Backpressured);
                    }
                    Err(error) => {
                        shared.mark_backpressured(format!(
                            "write stalled and peer presence query failed: {error}"
                        ));
                        return Ok(WriteOutcome::Backpressured);
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            return Ok(WriteOutcome::PeerUnavailable);
        }

        std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
    }

    Ok(WriteOutcome::Delivered)
}

fn virtual_to_physical_loop(
    mut endpoint: BridgeEndpoint,
    io: Arc<SessionIo>,
    shared: &EndpointShared,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut read_buf = [0u8; 4096];
    let mut pending_write: Option<Vec<u8>> = None;

    while !cancel.load(Ordering::SeqCst) {
        if io.is_exclusive() || shared.is_backpressured() {
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

        #[cfg(target_os = "windows")]
        {
            let open = match peer_is_open(&mut endpoint) {
                Ok(open) => open,
                Err(error) => {
                    shared.mark_backpressured(error);
                    std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                    continue;
                }
            };
            shared.set_peer_open(open);
            if !open {
                std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                continue;
            }
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
                #[cfg(target_os = "windows")]
                {
                    match peer_is_open(&mut endpoint) {
                        Ok(false) => {
                            shared.set_peer_open(false);
                            std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                        }
                        Ok(true) => {
                            shared.mark_backpressured(format!(
                                "read failed while peer remained open: {error}"
                            ));
                        }
                        Err(status_error) => {
                            shared.mark_backpressured(format!(
                                "read failed and peer presence could not be verified: {error}; {status_error}"
                            ));
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    log::trace!(
                        "Virtual peer {} is currently unavailable: {}",
                        endpoint.external_path,
                        error
                    );
                    std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn backlog_budget_is_bounded_and_scales_with_line_rate() {
        let slow = egress_backlog_limit_bytes(115_200);
        let faster = egress_backlog_limit_bytes(2_000_000);
        let fastest = egress_backlog_limit_bytes(10_000_000);
        assert_eq!(slow, EGRESS_MIN_BYTES);
        assert!(faster > slow);
        assert_eq!(fastest, EGRESS_MAX_BYTES);
    }

    #[test]
    fn one_backpressured_endpoint_does_not_block_siblings() {
        let (event_tx, event_rx) = mpsc::channel();
        let a = Arc::new(EndpointShared::new(
            "COM21".into(),
            4,
            true,
            event_tx.clone(),
        ));
        let b = Arc::new(EndpointShared::new("COM23".into(), 16, true, event_tx));
        let (a_tx, a_rx) = mpsc::sync_channel(8);
        let (b_tx, b_rx) = mpsc::sync_channel(8);
        let targets = vec![
            EgressTarget {
                sender: a_tx,
                shared: a.clone(),
            },
            EgressTarget {
                sender: b_tx,
                shared: b.clone(),
            },
        ];

        fan_out_physical_chunk(&targets, vec![1, 2, 3, 4]).unwrap();
        fan_out_physical_chunk(&targets, vec![5]).unwrap();

        assert!(a.is_backpressured());
        assert!(!b.is_backpressured());
        assert_eq!(&*a_rx.try_recv().unwrap(), &[1, 2, 3, 4]);
        assert_eq!(&*b_rx.try_recv().unwrap(), &[1, 2, 3, 4]);
        assert_eq!(&*b_rx.try_recv().unwrap(), &[5]);

        match event_rx.try_recv().unwrap() {
            VirtualPortBridgeEvent::EndpointBackpressured {
                external_path,
                backlog_limit_bytes,
                ..
            } => {
                assert_eq!(external_path, "COM21");
                assert_eq!(backlog_limit_bytes, 4);
            }
            other => panic!("unexpected bridge event: {other:?}"),
        }
    }

    #[test]
    fn endpoint_recovery_requires_explicit_reopen_transition() {
        let (event_tx, event_rx) = mpsc::channel();
        let shared = EndpointShared::new("COM21".into(), 64, true, event_tx);
        shared.mark_backpressured("test stall");
        assert!(shared.is_backpressured());
        shared.recover_after_reopen();
        assert!(!shared.is_backpressured());

        assert!(matches!(
            event_rx.try_recv().unwrap(),
            VirtualPortBridgeEvent::EndpointBackpressured { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            VirtualPortBridgeEvent::EndpointRecovered { .. }
        ));
    }
}
