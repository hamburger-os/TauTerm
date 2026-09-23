//! Virtual serial endpoint bridge.
//!
//! Physical serial ownership remains inside DataPlaneRuntime. Each virtual endpoint is owned by one
//! actor and one serial handle for its entire lifetime. The DataPlane pump only performs bounded,
//! non-blocking fan-out; endpoint actors serialize peer-presence checks, physical -> virtual writes,
//! and virtual -> physical reads so a driver handle is never touched concurrently by cloned workers.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::session::SessionIo;
use crate::transport::DataPlaneEvent;
use crate::virtual_port::backend::VirtualEndpoint;

const VPORT_IO_TIMEOUT_MS: u64 = 5;
const PEER_RETRY_DELAY_MS: u64 = 10;
const PEER_STATUS_POLL_MS: u64 = 50;
const WORKER_POLL_MS: u64 = 20;
const EGRESS_QUEUE_MESSAGES: usize = 1024;
const EGRESS_BACKLOG_WINDOW_MS: u64 = 2_000;
const EGRESS_MIN_BYTES: usize = 64 * 1024;
const EGRESS_MAX_BYTES: usize = 1024 * 1024;
const VPORT_SUBSCRIPTION_MIN_MESSAGES: usize = 4 * 1024;
const VPORT_SUBSCRIPTION_MAX_MESSAGES: usize = 64 * 1024;
const VPORT_SUBSCRIPTION_WINDOW_MS: u64 = 3_000;
const WRITE_STALL_DEADLINE: Duration = Duration::from_secs(2);

const ENDPOINT_READY: u8 = 0;
const ENDPOINT_DEGRADED: u8 = 1;
const ENDPOINT_BACKPRESSURED: u8 = 2;

type BridgeEventHandler = Box<dyn Fn(VirtualPortBridgeEvent) + Send + 'static>;

#[derive(Debug, Clone)]
pub enum VirtualPortBridgeEvent {
    EndpointDegraded {
        external_path: String,
        reason: String,
    },
    EndpointBackpressured {
        external_path: String,
        reason: String,
        queued_bytes: usize,
        backlog_limit_bytes: usize,
        stalled_for_ms: u64,
    },
    EndpointReady {
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
    peer_known: AtomicBool,
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
        peer_known: bool,
        peer_open: bool,
        events: mpsc::Sender<VirtualPortBridgeEvent>,
    ) -> Self {
        Self {
            external_path,
            peer_known: AtomicBool::new(peer_known),
            peer_open: AtomicBool::new(peer_open),
            state: AtomicU8::new(ENDPOINT_READY),
            queued_bytes: AtomicUsize::new(0),
            backlog_limit_bytes,
            last_progress: Mutex::new(Instant::now()),
            events,
        }
    }

    fn is_backpressured(&self) -> bool {
        self.state.load(Ordering::Acquire) == ENDPOINT_BACKPRESSURED
    }

    fn is_degraded(&self) -> bool {
        self.state.load(Ordering::Acquire) == ENDPOINT_DEGRADED
    }

    fn can_enqueue(&self) -> bool {
        self.state.load(Ordering::Acquire) == ENDPOINT_READY
            && self.peer_known.load(Ordering::Acquire)
            && self.peer_open.load(Ordering::Acquire)
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

    #[cfg(any(target_os = "windows", test))]
    fn observe_peer(&self, open: bool) {
        let was_known = self.peer_known.swap(true, Ordering::AcqRel);
        let previous = self.peer_open.swap(open, Ordering::AcqRel);
        if !was_known || previous != open {
            if open {
                log::info!("Virtual peer {} opened", self.external_path);
            } else {
                log::info!("Virtual peer {} closed", self.external_path);
            }
        }

        if self
            .state
            .compare_exchange(
                ENDPOINT_DEGRADED,
                ENDPOINT_READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.record_progress();
            log::info!(
                "Virtual peer {} presence monitoring recovered",
                self.external_path
            );
            let _ = self.events.send(VirtualPortBridgeEvent::EndpointReady {
                external_path: self.external_path.clone(),
            });
        }
    }

    fn mark_degraded(&self, reason: impl Into<String>) {
        self.peer_known.store(false, Ordering::Release);
        if self
            .state
            .compare_exchange(
                ENDPOINT_READY,
                ENDPOINT_DEGRADED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }

        let reason = reason.into();
        log::warn!(
            "Virtual peer {} presence degraded: {}",
            self.external_path,
            reason
        );
        let _ = self.events.send(VirtualPortBridgeEvent::EndpointDegraded {
            external_path: self.external_path.clone(),
            reason,
        });
    }

    fn mark_backpressured(&self, reason: impl Into<String>) {
        let previous = self.state.swap(ENDPOINT_BACKPRESSURED, Ordering::AcqRel);
        if previous == ENDPOINT_BACKPRESSURED {
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

    #[cfg(any(target_os = "windows", test))]
    fn recover_after_reopen(&self) {
        if self
            .state
            .compare_exchange(
                ENDPOINT_BACKPRESSURED,
                ENDPOINT_READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        self.queued_bytes.store(0, Ordering::Release);
        self.peer_known.store(true, Ordering::Release);
        self.peer_open.store(true, Ordering::Release);
        self.record_progress();
        log::info!(
            "Virtual peer {} recovered after close/reopen; starting a fresh stream",
            self.external_path
        );
        let _ = self.events.send(VirtualPortBridgeEvent::EndpointReady {
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
        if !self.shared.can_enqueue() {
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
                self.shared
                    .mark_backpressured("egress message queue exhausted while peer remained open");
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(data)) => {
                self.shared.release_bytes(data.len());
                Err(format!(
                    "virtual peer {} endpoint actor stopped unexpectedly",
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

        let backlog_limit_bytes = egress_backlog_limit_bytes(baud_rate);
        let subscription_capacity_messages = vport_subscription_capacity_messages(baud_rate);
        let subscription = io
            .subscribe_with_capacity("virtual-port-bridge", subscription_capacity_messages)
            .map_err(|error| error.to_string())?;

        // Open every bridge endpoint before starting any worker. This keeps startup atomic and,
        // critically, creates exactly one handle owner per endpoint instead of cloning a Windows
        // COM handle into concurrent reader/writer threads.
        let mut prepared = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints {
            prepared.push(BridgeEndpoint {
                external_path: endpoint.external_path,
                port: open_bridge_endpoint(&endpoint.bridge_path, baud_rate)?,
            });
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (event_tx, event_rx) = mpsc::channel::<VirtualPortBridgeEvent>();
        let mut worker_threads = Vec::with_capacity(prepared.len() + 1);
        let mut egress_targets = Vec::with_capacity(prepared.len());

        for endpoint in prepared {
            let external_path = endpoint.external_path.clone();
            #[cfg(target_os = "windows")]
            let (peer_known, peer_open) = (false, false);
            #[cfg(not(target_os = "windows"))]
            let (peer_known, peer_open) = (true, true);

            let shared = Arc::new(EndpointShared::new(
                external_path.clone(),
                backlog_limit_bytes,
                peer_known,
                peer_open,
                event_tx.clone(),
            ));
            let (egress_tx, egress_rx) = mpsc::sync_channel(EGRESS_QUEUE_MESSAGES);
            egress_targets.push(EgressTarget {
                sender: egress_tx,
                shared: shared.clone(),
            });

            let cancel = cancel_flag.clone();
            let events = event_tx.clone();
            let actor_io = io.clone();
            worker_threads.push(std::thread::spawn(move || {
                if let Err(reason) =
                    endpoint_actor_loop(endpoint, egress_rx, actor_io, &shared, &cancel)
                {
                    let _ = events.send(VirtualPortBridgeEvent::Fatal { reason });
                }
            }));

            log::info!(
                "Virtual bridge attached for external endpoint {} (actor_owner=single, egress_limit={} bytes, subscription_capacity={} messages)",
                external_path,
                backlog_limit_bytes,
                subscription_capacity_messages
            );
        }

        {
            let cancel = cancel_flag.clone();
            let events = event_tx.clone();
            worker_threads.push(std::thread::spawn(move || {
                if let Err(reason) =
                    physical_subscription_pump(subscription, egress_targets, &cancel)
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
        self.shutdown_inner();
    }

    fn shutdown_inner(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);

        #[cfg(target_os = "windows")]
        cancel_windows_worker_io(&self.worker_threads);

        for (index, thread) in self.worker_threads.drain(..).enumerate() {
            join_bridge_thread(&format!("worker-{index}"), thread);
        }
        if let Some(thread) = self.supervisor_thread.take() {
            join_bridge_thread("supervisor", thread);
        }
    }
}

impl Drop for VirtualPortBridge {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

fn egress_backlog_limit_bytes(baud_rate: u32) -> usize {
    let bytes_per_second = (u64::from(baud_rate) / 10).max(1);
    let window_bytes = bytes_per_second.saturating_mul(EGRESS_BACKLOG_WINDOW_MS) / 1_000;
    usize::try_from(window_bytes)
        .unwrap_or(usize::MAX)
        .clamp(EGRESS_MIN_BYTES, EGRESS_MAX_BYTES)
}

fn vport_subscription_capacity_messages(baud_rate: u32) -> usize {
    // DataPlane events may be very small on Windows serial drivers. Size the upstream queue from
    // a worst-case one-byte event rate over a finite scheduling-jitter window, then cap memory.
    let bytes_per_second = (u64::from(baud_rate) / 10).max(1);
    let messages = bytes_per_second.saturating_mul(VPORT_SUBSCRIPTION_WINDOW_MS) / 1_000;
    usize::try_from(messages).unwrap_or(usize::MAX).clamp(
        VPORT_SUBSCRIPTION_MIN_MESSAGES,
        VPORT_SUBSCRIPTION_MAX_MESSAGES,
    )
}

#[cfg(target_os = "windows")]
fn cancel_windows_worker_io(threads: &[JoinHandle<()>]) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::CancelSynchronousIo;

    for thread in threads {
        if thread.is_finished() {
            continue;
        }
        unsafe {
            let _ = CancelSynchronousIo(thread.as_raw_handle() as _);
        }
    }
}

fn join_bridge_thread(name: &str, thread: JoinHandle<()>) {
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
        .timeout(Duration::from_millis(VPORT_IO_TIMEOUT_MS))
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
                let detail = subscription
                    .disconnect_reason()
                    .map(|reason| reason.to_string())
                    .unwrap_or_else(|| "channel closed without a recorded detach reason".into());
                return Err(format!("data-plane bridge subscription ended: {detail}"));
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

fn endpoint_actor_loop(
    mut endpoint: BridgeEndpoint,
    receiver: mpsc::Receiver<Arc<[u8]>>,
    io: Arc<SessionIo>,
    shared: &EndpointShared,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut read_buf = [0u8; 4096];
    let mut pending_writeback: Option<Vec<u8>> = None;

    #[cfg(target_os = "windows")]
    let mut next_peer_probe = Instant::now();
    #[cfg(target_os = "windows")]
    let mut reopen_armed = false;

    while !cancel.load(Ordering::SeqCst) {
        #[cfg(target_os = "windows")]
        {
            if Instant::now() >= next_peer_probe {
                next_peer_probe =
                    Instant::now() + Duration::from_millis(PEER_STATUS_POLL_MS);
                match peer_is_open(&mut endpoint) {
                    Ok(open) => {
                        shared.observe_peer(open);
                        if shared.is_backpressured() {
                            drain_egress_queue(&receiver, shared);
                            if !open {
                                reopen_armed = true;
                            } else if reopen_armed {
                                shared.recover_after_reopen();
                                reopen_armed = false;
                            }
                        }
                    }
                    Err(error) => {
                        shared.mark_degraded(error);
                        drain_egress_queue(&receiver, shared);
                        std::thread::sleep(Duration::from_millis(PEER_RETRY_DELAY_MS));
                        continue;
                    }
                }
            }

            if !shared.peer_known.load(Ordering::Acquire)
                || !shared.peer_open.load(Ordering::Acquire)
                || shared.is_backpressured()
                || shared.is_degraded()
            {
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

        match receiver.try_recv() {
            Ok(data) => {
                shared.release_bytes(data.len());
                match write_chunk_to_peer(&mut endpoint, &data, shared, cancel)? {
                    WriteOutcome::Delivered => {}
                    WriteOutcome::PeerUnavailable | WriteOutcome::Backpressured => {
                        drain_egress_queue(&receiver, shared);
                    }
                    WriteOutcome::Cancelled => return Ok(()),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                return Err(format!(
                    "virtual peer {} endpoint actor queue disconnected unexpectedly",
                    endpoint.external_path
                ));
            }
        }

        if io.is_exclusive() {
            std::thread::sleep(Duration::from_millis(VPORT_IO_TIMEOUT_MS));
            continue;
        }

        if let Some(data) = pending_writeback.take() {
            match io.send(&data) {
                Ok(()) => {}
                Err(_) if io.is_exclusive() => {
                    pending_writeback = Some(data);
                    std::thread::sleep(Duration::from_millis(VPORT_IO_TIMEOUT_MS));
                    continue;
                }
                Err(error) => return Err(format!("virtual endpoint writeback failed: {error}")),
            }
        }

        match endpoint.port.read(&mut read_buf) {
            Ok(n) if n > 0 => {
                let data = read_buf[..n].to_vec();
                match io.send(&data) {
                    Ok(()) => {
                        shared.record_progress();
                    }
                    Err(_) if io.is_exclusive() => {
                        pending_writeback = Some(data);
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
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }

                #[cfg(target_os = "windows")]
                {
                    match peer_is_open(&mut endpoint) {
                        Ok(false) => {
                            shared.observe_peer(false);
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

    drain_egress_queue(&receiver, shared);
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
        if shared.is_backpressured() || shared.is_degraded() {
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
                if cancel.load(Ordering::SeqCst) {
                    return Ok(WriteOutcome::Cancelled);
                }

                #[cfg(target_os = "windows")]
                {
                    match peer_is_open(endpoint) {
                        Ok(false) => {
                            shared.observe_peer(false);
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
                        shared.observe_peer(false);
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn ready_shared(
        external_path: &str,
        backlog_limit_bytes: usize,
        events: mpsc::Sender<VirtualPortBridgeEvent>,
    ) -> Arc<EndpointShared> {
        Arc::new(EndpointShared::new(
            external_path.into(),
            backlog_limit_bytes,
            true,
            true,
            events,
        ))
    }

    #[test]
    fn backlog_budget_is_bounded_and_scales_with_line_rate() {
        let slow = egress_backlog_limit_bytes(115_200);
        let faster = egress_backlog_limit_bytes(2_000_000);
        let fastest = egress_backlog_limit_bytes(10_000_000);
        assert_eq!(slow, EGRESS_MIN_BYTES);
        assert!(faster > slow);
        assert_eq!(fastest, EGRESS_MAX_BYTES);

        let serial_capacity = vport_subscription_capacity_messages(115_200);
        assert!(serial_capacity > 256);
        assert!(serial_capacity <= VPORT_SUBSCRIPTION_MAX_MESSAGES);
        assert_eq!(
            vport_subscription_capacity_messages(10_000_000),
            VPORT_SUBSCRIPTION_MAX_MESSAGES
        );
    }

    #[test]
    fn one_backpressured_endpoint_does_not_block_siblings() {
        let (event_tx, event_rx) = mpsc::channel();
        let a = ready_shared("COM21", 4, event_tx.clone());
        let b = ready_shared("COM23", 16, event_tx);
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
    fn sustained_stalled_endpoint_does_not_block_healthy_sibling() {
        let (event_tx, _event_rx) = mpsc::channel();
        let stalled = ready_shared("COM21", 8, event_tx.clone());
        let healthy = ready_shared("COM23", 32, event_tx);
        let (stalled_tx, _stalled_rx) = mpsc::sync_channel(8);
        let (healthy_tx, healthy_rx) = mpsc::sync_channel(8);
        let targets = vec![
            EgressTarget {
                sender: stalled_tx,
                shared: stalled.clone(),
            },
            EgressTarget {
                sender: healthy_tx,
                shared: healthy.clone(),
            },
        ];

        for sequence in 0..1_000u16 {
            let value = (sequence % 251) as u8;
            fan_out_physical_chunk(&targets, vec![value; 4]).unwrap();

            let delivered = healthy_rx
                .try_recv()
                .expect("healthy endpoint must keep up");
            assert_eq!(&*delivered, &[value; 4]);
            healthy.release_bytes(delivered.len());
        }

        assert!(stalled.is_backpressured());
        assert!(!healthy.is_backpressured());
        assert_eq!(healthy.queued_bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn presence_query_failure_is_degraded_not_data_gap() {
        let (event_tx, event_rx) = mpsc::channel();
        let endpoint = ready_shared("COM21", 64, event_tx);

        endpoint.mark_degraded("temporary modem-status query failure");
        assert!(endpoint.is_degraded());
        assert!(!endpoint.is_backpressured());
        assert!(!endpoint.can_enqueue());

        match event_rx.try_recv().unwrap() {
            VirtualPortBridgeEvent::EndpointDegraded { external_path, .. } => {
                assert_eq!(external_path, "COM21");
            }
            other => panic!("unexpected bridge event: {other:?}"),
        }

        endpoint.observe_peer(true);
        assert!(!endpoint.is_degraded());
        assert!(!endpoint.is_backpressured());
        assert!(endpoint.can_enqueue());
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            VirtualPortBridgeEvent::EndpointReady { .. }
        ));
    }

    #[test]
    fn backpressure_recovery_starts_a_fresh_stream() {
        let (event_tx, event_rx) = mpsc::channel();
        let endpoint = ready_shared("COM21", 64, event_tx);

        assert!(endpoint.reserve_bytes(12));
        endpoint.mark_backpressured("test gap");
        assert!(endpoint.is_backpressured());
        assert_eq!(endpoint.queued_bytes.load(Ordering::Acquire), 12);
        let _ = event_rx.try_recv().unwrap();

        endpoint.recover_after_reopen();
        assert!(!endpoint.is_backpressured());
        assert_eq!(endpoint.queued_bytes.load(Ordering::Acquire), 0);
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            VirtualPortBridgeEvent::EndpointReady { .. }
        ));
    }
}
