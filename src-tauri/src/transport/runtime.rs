use std::collections::VecDeque;
use std::io::{Read, Write};
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus, StreamCloseMetadata};

const COMMAND_CAPACITY: usize = 256;
const READ_BUFFER_SIZE: usize = 16 * 1024;
const STARTUP_BUFFER_LIMIT: usize = 64 * 1024;
const SUBSCRIPTION_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub enum DataPlaneEvent {
    Data(Vec<u8>),
    Closed(TransportCloseInfo),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlaneSubscriptionEnd {
    BacklogExceeded { capacity_messages: usize },
    RuntimeStopped,
}

impl std::fmt::Display for DataPlaneSubscriptionEnd {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BacklogExceeded { capacity_messages } => write!(
                formatter,
                "bounded backlog exceeded (capacity_messages={capacity_messages})"
            ),
            Self::RuntimeStopped => formatter.write_str("transport runtime stopped"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransportCloseInfo {
    pub kind: TransportErrorKind,
    pub reason: String,
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

impl TransportCloseInfo {
    fn from_driver(
        kind: TransportErrorKind,
        reason: impl Into<String>,
        metadata: StreamCloseMetadata,
    ) -> Self {
        Self {
            kind,
            reason: reason.into(),
            exit_code: metadata.exit_code,
            signal: metadata.signal,
        }
    }
}

#[derive(Clone)]
pub struct DataPlaneHandle {
    command_tx: mpsc::SyncSender<RuntimeCommand>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
    exclusive_active: Arc<AtomicBool>,
    async_command_order: Arc<tokio::sync::Mutex<()>>,
    terminal_control: bool,
}

pub struct DataPlaneSubscription {
    id: u64,
    command_tx: mpsc::SyncSender<RuntimeCommand>,
    receiver: mpsc::Receiver<DataPlaneEvent>,
    disconnect_reason: Arc<Mutex<Option<DataPlaneSubscriptionEnd>>>,
}

impl DataPlaneSubscription {
    pub fn disconnect_reason(&self) -> Option<DataPlaneSubscriptionEnd> {
        self.disconnect_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }
}

struct RuntimeSubscriber {
    id: u64,
    consumer: String,
    capacity_messages: usize,
    sender: mpsc::SyncSender<DataPlaneEvent>,
    disconnect_reason: Arc<Mutex<Option<DataPlaneSubscriptionEnd>>>,
}

impl Deref for DataPlaneSubscription {
    type Target = mpsc::Receiver<DataPlaneEvent>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl Drop for DataPlaneSubscription {
    fn drop(&mut self) {
        let _ = self
            .command_tx
            .try_send(RuntimeCommand::Unsubscribe { id: self.id });
    }
}

impl DataPlaneHandle {
    /// Confirmed synchronous write for worker/internal callers. Tauri/WebView commands must use the
    /// async counterpart so waiting for the actor/driver never blocks the application main thread.
    pub fn write(&self, data: &[u8]) -> Result<(), TransportError> {
        if self.exclusive_active.load(Ordering::Acquire) {
            return Err(TransportError::busy(
                "write",
                "data plane is exclusively owned",
            ));
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::Write {
                data: data.to_vec(),
                ack: CommandAck::Blocking(ack_tx),
            })
            .map_err(|_| runtime_closed("write"))?;
        ack_rx
            .recv()
            .map_err(|_| runtime_closed_before_ack("write"))?
    }

    /// Confirmed write for async Tauri/frontend paths. The ordering lock is held only while the
    /// command is enqueued; the actor acknowledgement is awaited after releasing it, so command
    /// producers remain concurrent while physical writes preserve actor queue order.
    pub async fn write_async(&self, data: Vec<u8>) -> Result<(), TransportError> {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        {
            let _order = self.async_command_order.lock().await;
            if self.exclusive_active.load(Ordering::Acquire) {
                return Err(TransportError::busy(
                    "write",
                    "data plane is exclusively owned",
                ));
            }
            self.try_send_async_command(
                RuntimeCommand::Write {
                    data,
                    ack: CommandAck::Async(ack_tx),
                },
                "write",
            )?;
        }
        ack_rx
            .await
            .map_err(|_| runtime_closed_before_ack("write"))?
    }

    fn try_send_async_command(
        &self,
        command: RuntimeCommand,
        operation: &'static str,
    ) -> Result<(), TransportError> {
        match self.command_tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(TransportError::busy(
                operation,
                "transport command queue is full",
            )),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(runtime_closed(operation)),
        }
    }

    pub fn subscribe(
        &self,
        consumer: impl Into<String>,
    ) -> Result<DataPlaneSubscription, TransportError> {
        self.subscribe_with_capacity(consumer, SUBSCRIPTION_CAPACITY)
    }

    pub fn subscribe_with_capacity(
        &self,
        consumer: impl Into<String>,
        capacity_messages: usize,
    ) -> Result<DataPlaneSubscription, TransportError> {
        if capacity_messages == 0 {
            return Err(TransportError::new(
                TransportErrorKind::InvalidConfiguration,
                "subscribe",
                "subscription capacity must be greater than zero",
            ));
        }
        let id = next_subscription_id();
        let consumer = consumer.into();
        let disconnect_reason = Arc::new(Mutex::new(None));
        let (event_tx, event_rx) = mpsc::sync_channel(capacity_messages);
        self.command_tx
            .send(RuntimeCommand::Subscribe {
                id,
                consumer,
                capacity_messages,
                subscriber: event_tx,
                disconnect_reason: disconnect_reason.clone(),
            })
            .map_err(|_| runtime_closed("subscribe"))?;
        Ok(DataPlaneSubscription {
            id,
            command_tx: self.command_tx.clone(),
            receiver: event_rx,
            disconnect_reason,
        })
    }

    /// Acquire the physical byte-stream driver for an ownership-sensitive inline protocol.
    ///
    /// The actor remains alive to serialize lifecycle/control commands, but it no longer proxies
    /// protocol I/O while the lease is active. Bytes already read by the actor but not yet delivered
    /// to a shared subscriber are handed to the lease before the physical driver so an inline
    /// protocol cannot lose a C/NAK/ZMODEM handshake at the ownership boundary.
    pub fn acquire_exclusive(
        &self,
        owner_name: impl Into<String>,
    ) -> Result<ExclusiveIo, TransportError> {
        self.exclusive_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                TransportError::busy(
                    "acquire_exclusive",
                    "data plane is already exclusively owned",
                )
            })?;

        let owner_id = next_owner_id();
        let (driver_tx, driver_rx) = mpsc::sync_channel(1);
        if self
            .command_tx
            .send(RuntimeCommand::AcquireExclusive {
                owner_id,
                owner_name: owner_name.into(),
                driver_tx,
            })
            .is_err()
        {
            self.exclusive_active.store(false, Ordering::Release);
            return Err(runtime_closed("acquire_exclusive"));
        }

        match driver_rx.recv() {
            Ok(Ok(payload)) => Ok(ExclusiveIo {
                owner_id,
                command_tx: self.command_tx.clone(),
                driver: Some(payload.driver),
                prefetched: payload.prefetched,
                released: false,
                exclusive_active: self.exclusive_active.clone(),
                tx_bytes: self.tx_bytes.clone(),
                rx_bytes: self.rx_bytes.clone(),
            }),
            Ok(Err(error)) => {
                self.exclusive_active.store(false, Ordering::Release);
                Err(error)
            }
            Err(_) => {
                self.exclusive_active.store(false, Ordering::Release);
                Err(runtime_closed_before_ack("acquire_exclusive"))
            }
        }
    }

    pub fn resize_terminal(&self, cols: u32, rows: u32) -> Result<(), TransportError> {
        if !self.terminal_control {
            return Err(TransportError::unsupported(
                "resize_terminal",
                "session has no terminal-control capability",
            ));
        }
        if self.exclusive_active.load(Ordering::Acquire) {
            return Err(TransportError::busy(
                "resize_terminal",
                "data plane is exclusively owned",
            ));
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::ResizeTerminal {
                cols,
                rows,
                ack: CommandAck::Blocking(ack_tx),
            })
            .map_err(|_| runtime_closed("resize_terminal"))?;
        ack_rx
            .recv()
            .map_err(|_| runtime_closed_before_ack("resize_terminal"))?
    }

    pub async fn resize_terminal_async(&self, cols: u32, rows: u32) -> Result<(), TransportError> {
        if !self.terminal_control {
            return Err(TransportError::unsupported(
                "resize_terminal",
                "session has no terminal-control capability",
            ));
        }
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        {
            let _order = self.async_command_order.lock().await;
            if self.exclusive_active.load(Ordering::Acquire) {
                return Err(TransportError::busy(
                    "resize_terminal",
                    "data plane is exclusively owned",
                ));
            }
            self.try_send_async_command(
                RuntimeCommand::ResizeTerminal {
                    cols,
                    rows,
                    ack: CommandAck::Async(ack_tx),
                },
                "resize_terminal",
            )?;
        }
        ack_rx
            .await
            .map_err(|_| runtime_closed_before_ack("resize_terminal"))?
    }

    pub fn shutdown(&self) -> Result<(), TransportError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        if self
            .command_tx
            .send(RuntimeCommand::Shutdown { ack: ack_tx })
            .is_err()
        {
            return Ok(());
        }
        ack_rx.recv().unwrap_or(Ok(()))
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn is_exclusive(&self) -> bool {
        self.exclusive_active.load(Ordering::Acquire)
    }

    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }

    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }

    pub fn supports_terminal_control(&self) -> bool {
        self.terminal_control
    }
}

fn runtime_closed(operation: &'static str) -> TransportError {
    TransportError::new(
        TransportErrorKind::RemoteClosed,
        operation,
        "transport runtime is closed",
    )
}

fn runtime_closed_before_ack(operation: &'static str) -> TransportError {
    TransportError::new(
        TransportErrorKind::RemoteClosed,
        operation,
        "transport runtime closed before acknowledging command",
    )
}

fn transport_error_to_io(error: TransportError) -> std::io::Error {
    let kind = match error.kind {
        TransportErrorKind::Timeout => std::io::ErrorKind::TimedOut,
        TransportErrorKind::PermissionDenied => std::io::ErrorKind::PermissionDenied,
        TransportErrorKind::DeviceNotFound => std::io::ErrorKind::NotFound,
        TransportErrorKind::RemoteClosed => std::io::ErrorKind::UnexpectedEof,
        TransportErrorKind::ConnectionReset => std::io::ErrorKind::ConnectionReset,
        TransportErrorKind::Busy => std::io::ErrorKind::WouldBlock,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(kind, error.to_string())
}

pub struct DataPlaneRuntime {
    pub handle: DataPlaneHandle,
    thread: Option<JoinHandle<()>>,
}

impl DataPlaneRuntime {
    pub fn spawn(driver: Box<dyn BlockingByteStream>) -> Self {
        let terminal_control = driver.supports_terminal_control();
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        let connected = Arc::new(AtomicBool::new(true));
        let exclusive_active = Arc::new(AtomicBool::new(false));
        let handle = DataPlaneHandle {
            command_tx,
            tx_bytes: tx_bytes.clone(),
            rx_bytes: rx_bytes.clone(),
            connected: connected.clone(),
            exclusive_active: exclusive_active.clone(),
            async_command_order: Arc::new(tokio::sync::Mutex::new(())),
            terminal_control,
        };
        let thread = std::thread::spawn(move || {
            run_blocking_runtime(
                driver,
                command_rx,
                tx_bytes,
                rx_bytes,
                connected,
                exclusive_active,
            )
        });
        Self {
            handle,
            thread: Some(thread),
        }
    }

    pub fn join(mut self) {
        let _ = self.handle.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    pub fn take_thread(&mut self) -> Option<JoinHandle<()>> {
        self.thread.take()
    }
}

impl Drop for DataPlaneRuntime {
    fn drop(&mut self) {
        let _ = self.handle.shutdown();
    }
}

struct ExclusiveLeasePayload {
    driver: Box<dyn BlockingByteStream>,
    prefetched: VecDeque<u8>,
}

pub struct ExclusiveIo {
    owner_id: u64,
    command_tx: mpsc::SyncSender<RuntimeCommand>,
    driver: Option<Box<dyn BlockingByteStream>>,
    prefetched: VecDeque<u8>,
    released: bool,
    exclusive_active: Arc<AtomicBool>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
}

impl ExclusiveIo {
    pub fn release(mut self) -> Result<(), TransportError> {
        self.release_inner()
    }

    fn driver_mut(&mut self) -> std::io::Result<&mut Box<dyn BlockingByteStream>> {
        self.driver.as_mut().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "exclusive transport driver already released",
            )
        })
    }

    fn release_inner(&mut self) -> Result<(), TransportError> {
        if self.released {
            return Ok(());
        }
        self.released = true;

        let Some(driver) = self.driver.take() else {
            self.exclusive_active.store(false, Ordering::Release);
            return Ok(());
        };
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let command = RuntimeCommand::ReturnExclusive {
            owner_id: self.owner_id,
            driver,
            prefetched: std::mem::take(&mut self.prefetched),
            ack: ack_tx,
        };

        if let Err(error) = self.command_tx.send(command) {
            if let RuntimeCommand::ReturnExclusive { mut driver, .. } = error.0 {
                let _ = driver.shutdown();
            }
            self.exclusive_active.store(false, Ordering::Release);
            return Err(runtime_closed("return_exclusive"));
        }

        let result = ack_rx
            .recv()
            .map_err(|_| runtime_closed_before_ack("return_exclusive"))?;
        self.exclusive_active.store(false, Ordering::Release);
        result
    }
}

impl Read for ExclusiveIo {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if !self.prefetched.is_empty() {
            let count = buf.len().min(self.prefetched.len());
            for slot in &mut buf[..count] {
                *slot = self
                    .prefetched
                    .pop_front()
                    .expect("prefetched length checked above");
            }
            // These bytes were already physically read and counted by the actor before handoff.
            return Ok(count);
        }

        let result = self
            .driver_mut()?
            .read(buf)
            .map_err(transport_error_to_io)?;
        match result {
            ReadStatus::Data(n) if n > 0 => {
                let n = n.min(buf.len());
                self.rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                Ok(n)
            }
            ReadStatus::Data(_) | ReadStatus::Idle => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "exclusive data-plane read timed out",
            )),
            ReadStatus::Eof => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "transport closed while exclusive lease was active",
            )),
        }
    }
}

impl Write for ExclusiveIo {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.driver_mut()?
            .write_all(buf)
            .map_err(transport_error_to_io)?;
        self.tx_bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.driver_mut()?.flush().map_err(transport_error_to_io)
    }
}

impl Drop for ExclusiveIo {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

enum CommandAck {
    Blocking(mpsc::SyncSender<Result<(), TransportError>>),
    Async(tokio::sync::oneshot::Sender<Result<(), TransportError>>),
}

impl CommandAck {
    fn send(self, result: Result<(), TransportError>) {
        match self {
            Self::Blocking(sender) => {
                let _ = sender.send(result);
            }
            Self::Async(sender) => {
                let _ = sender.send(result);
            }
        }
    }
}

enum RuntimeCommand {
    Write {
        data: Vec<u8>,
        ack: CommandAck,
    },
    Subscribe {
        id: u64,
        consumer: String,
        capacity_messages: usize,
        subscriber: mpsc::SyncSender<DataPlaneEvent>,
        disconnect_reason: Arc<Mutex<Option<DataPlaneSubscriptionEnd>>>,
    },
    Unsubscribe {
        id: u64,
    },
    AcquireExclusive {
        owner_id: u64,
        owner_name: String,
        driver_tx: mpsc::SyncSender<Result<ExclusiveLeasePayload, TransportError>>,
    },
    ReturnExclusive {
        owner_id: u64,
        driver: Box<dyn BlockingByteStream>,
        prefetched: VecDeque<u8>,
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
    ResizeTerminal {
        cols: u32,
        rows: u32,
        ack: CommandAck,
    },
    Shutdown {
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
}

struct ExclusiveState {
    owner_id: u64,
    owner_name: String,
}

struct RuntimeLoopState {
    subscribers: Vec<RuntimeSubscriber>,
    startup_buffer: VecDeque<Vec<u8>>,
    startup_buffer_bytes: usize,
    /// Bytes physically read while an exclusive acquisition was already requested but before the
    /// actor could process the AcquireExclusive command. They must follow the driver ownership.
    handoff_buffer: VecDeque<u8>,
    exclusive: Option<ExclusiveState>,
    shutdown_pending: bool,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    exclusive_active: Arc<AtomicBool>,
}

enum CommandOutcome {
    Continue,
    Shutdown,
    Close(TransportCloseInfo),
}

/// Fail-safe guard for externally visible runtime state. Normal runtime exit still clears these
/// flags explicitly; this Drop path additionally covers unwinding from a buggy driver.
struct RuntimeStateGuard {
    connected: Arc<AtomicBool>,
    exclusive_active: Arc<AtomicBool>,
}

impl Drop for RuntimeStateGuard {
    fn drop(&mut self) {
        self.exclusive_active.store(false, Ordering::Release);
        self.connected.store(false, Ordering::Release);
    }
}

fn apply_command_outcome(
    outcome: CommandOutcome,
    subscribers: &mut Vec<RuntimeSubscriber>,
    closing: &mut bool,
    driver_shutdown: &mut bool,
) {
    match outcome {
        CommandOutcome::Continue => {}
        CommandOutcome::Shutdown => {
            *closing = true;
            *driver_shutdown = true;
        }
        CommandOutcome::Close(info) => {
            broadcast_close(subscribers, info);
            *closing = true;
        }
    }
}

fn set_subscription_end(
    slot: &Arc<Mutex<Option<DataPlaneSubscriptionEnd>>>,
    reason: DataPlaneSubscriptionEnd,
) {
    if let Ok(mut current) = slot.lock() {
        if current.is_none() {
            *current = Some(reason);
        }
    }
}

fn publish_shared_data(state: &mut RuntimeLoopState, data: Vec<u8>) {
    if data.is_empty() {
        return;
    }
    if state.subscribers.is_empty() {
        state.startup_buffer_bytes += data.len();
        state.startup_buffer.push_back(data);
        while state.startup_buffer_bytes > STARTUP_BUFFER_LIMIT {
            if let Some(dropped) = state.startup_buffer.pop_front() {
                state.startup_buffer_bytes =
                    state.startup_buffer_bytes.saturating_sub(dropped.len());
            } else {
                break;
            }
        }
    } else {
        state.subscribers.retain(|subscriber| {
            match subscriber
                .sender
                .try_send(DataPlaneEvent::Data(data.clone()))
            {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(_)) => {
                    set_subscription_end(
                        &subscriber.disconnect_reason,
                        DataPlaneSubscriptionEnd::BacklogExceeded {
                            capacity_messages: subscriber.capacity_messages,
                        },
                    );
                    log::warn!(
                        "DataPlane subscriber {} ({}) exceeded bounded backlog (capacity_messages={}); detaching consumer",
                        subscriber.id,
                        subscriber.consumer,
                        subscriber.capacity_messages
                    );
                    false
                }
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
    }
}

fn publish_handoff_buffer(state: &mut RuntimeLoopState) {
    if state.handoff_buffer.is_empty() {
        return;
    }
    let data: Vec<u8> = state.handoff_buffer.drain(..).collect();
    publish_shared_data(state, data);
}

fn take_unconsumed_bytes(state: &mut RuntimeLoopState) -> VecDeque<u8> {
    let mut prefetched = VecDeque::new();
    while let Some(chunk) = state.startup_buffer.pop_front() {
        prefetched.extend(chunk);
    }
    state.startup_buffer_bytes = 0;
    prefetched.append(&mut state.handoff_buffer);
    prefetched
}

fn restore_unconsumed_bytes(state: &mut RuntimeLoopState, mut prefetched: VecDeque<u8>) {
    prefetched.append(&mut state.handoff_buffer);
    state.handoff_buffer = prefetched;
}

fn run_blocking_runtime(
    driver: Box<dyn BlockingByteStream>,
    commands: mpsc::Receiver<RuntimeCommand>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
    exclusive_active: Arc<AtomicBool>,
) {
    let mut driver = Some(driver);
    let mut state = RuntimeLoopState {
        subscribers: Vec::new(),
        startup_buffer: VecDeque::new(),
        startup_buffer_bytes: 0,
        handoff_buffer: VecDeque::new(),
        exclusive: None,
        shutdown_pending: false,
        tx_bytes,
        rx_bytes,
        exclusive_active: exclusive_active.clone(),
    };
    // Declare after state so panic unwinding clears externally visible state before subscriber
    // senders are dropped and the Session receive pump observes channel closure.
    let _state_guard = RuntimeStateGuard {
        connected: connected.clone(),
        exclusive_active: exclusive_active.clone(),
    };
    let mut read_buf = [0u8; READ_BUFFER_SIZE];
    let mut closing = false;
    let mut driver_shutdown = false;

    while !closing {
        // A physical driver lease means the actor owns no byte stream at all. It stays alive only
        // for lifecycle/subscription commands until the lease returns the same driver object.
        if state.exclusive.is_some() {
            match commands.recv() {
                Ok(command) => {
                    let outcome = handle_command(command, &mut driver, &mut state);
                    apply_command_outcome(
                        outcome,
                        &mut state.subscribers,
                        &mut closing,
                        &mut driver_shutdown,
                    );
                }
                Err(_) => closing = true,
            }
            continue;
        }

        // DataPlaneHandle marks the transition before enqueueing AcquireExclusive. Once observed,
        // the actor must stop starting new driver reads and wait for queued commands so the lease
        // boundary is deterministic even when a read was already in flight.
        if state.exclusive_active.load(Ordering::Acquire) {
            match commands.recv() {
                Ok(command) => {
                    let outcome = handle_command(command, &mut driver, &mut state);
                    apply_command_outcome(
                        outcome,
                        &mut state.subscribers,
                        &mut closing,
                        &mut driver_shutdown,
                    );
                }
                Err(_) => closing = true,
            }
            continue;
        }

        publish_handoff_buffer(&mut state);

        loop {
            match commands.try_recv() {
                Ok(command) => {
                    let outcome = handle_command(command, &mut driver, &mut state);
                    apply_command_outcome(
                        outcome,
                        &mut state.subscribers,
                        &mut closing,
                        &mut driver_shutdown,
                    );
                    if closing || state.exclusive.is_some() {
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    closing = true;
                    break;
                }
            }
        }
        if closing {
            break;
        }
        if state.exclusive.is_some() || state.exclusive_active.load(Ordering::Acquire) {
            continue;
        }

        let Some(active_driver) = driver.as_mut() else {
            broadcast_close(
                &mut state.subscribers,
                TransportCloseInfo::from_driver(
                    TransportErrorKind::Io,
                    "transport actor lost its driver without an active exclusive lease",
                    StreamCloseMetadata::default(),
                ),
            );
            break;
        };

        match active_driver.read(&mut read_buf) {
            Ok(ReadStatus::Data(n)) if n > 0 => {
                let n = n.min(read_buf.len());
                state.rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                let data = read_buf[..n].to_vec();
                if state.exclusive_active.load(Ordering::Acquire) {
                    state.handoff_buffer.extend(data);
                } else {
                    publish_shared_data(&mut state, data);
                }
            }
            Ok(ReadStatus::Data(_)) | Ok(ReadStatus::Idle) => {}
            Ok(ReadStatus::Eof) => {
                broadcast_close(
                    &mut state.subscribers,
                    TransportCloseInfo::from_driver(
                        TransportErrorKind::RemoteClosed,
                        "remote endpoint closed the stream",
                        active_driver.close_metadata(),
                    ),
                );
                break;
            }
            Err(error) => {
                broadcast_close(
                    &mut state.subscribers,
                    TransportCloseInfo::from_driver(
                        error.kind,
                        error.to_string(),
                        active_driver.close_metadata(),
                    ),
                );
                break;
            }
        }
    }

    for subscriber in &state.subscribers {
        set_subscription_end(
            &subscriber.disconnect_reason,
            DataPlaneSubscriptionEnd::RuntimeStopped,
        );
    }
    exclusive_active.store(false, Ordering::Release);
    connected.store(false, Ordering::Release);
    if !driver_shutdown {
        if let Some(active_driver) = driver.as_mut() {
            let _ = active_driver.shutdown();
        }
    }
}

fn handle_command(
    command: RuntimeCommand,
    driver: &mut Option<Box<dyn BlockingByteStream>>,
    state: &mut RuntimeLoopState,
) -> CommandOutcome {
    match command {
        RuntimeCommand::Write { data, ack } => {
            if let Some(active) = state.exclusive.as_ref() {
                ack.send(Err(TransportError::busy(
                    "write",
                    format!("data plane is exclusively owned by {}", active.owner_name),
                )));
                return CommandOutcome::Continue;
            }

            let Some(active_driver) = driver.as_mut() else {
                ack.send(Err(TransportError::new(
                    TransportErrorKind::Io,
                    "write",
                    "transport actor has no active driver",
                )));
                return CommandOutcome::Continue;
            };
            let result = active_driver
                .write_all(&data)
                .and_then(|_| active_driver.flush());
            if result.is_ok() {
                state
                    .tx_bytes
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
            }
            let close_info = result.as_ref().err().map(|error| {
                TransportCloseInfo::from_driver(
                    error.kind,
                    error.to_string(),
                    active_driver.close_metadata(),
                )
            });
            ack.send(result);
            close_info.map_or(CommandOutcome::Continue, CommandOutcome::Close)
        }
        RuntimeCommand::Subscribe {
            id,
            consumer,
            capacity_messages,
            subscriber,
            disconnect_reason,
        } => {
            // A new subscription has an empty bounded queue. Coalesce the bounded startup backlog
            // into one event so many tiny pre-subscribe reads cannot exhaust queue slots before the
            // consumer thread starts. The startup byte limit remains the memory bound.
            let startup = if state.startup_buffer.is_empty() {
                None
            } else {
                let mut data = Vec::with_capacity(state.startup_buffer_bytes);
                while let Some(chunk) = state.startup_buffer.pop_front() {
                    data.extend_from_slice(&chunk);
                }
                state.startup_buffer_bytes = 0;
                Some(data)
            };
            let alive =
                startup.is_none_or(
                    |data| match subscriber.try_send(DataPlaneEvent::Data(data)) {
                        Ok(()) => true,
                        Err(mpsc::TrySendError::Full(_)) => {
                            set_subscription_end(
                                &disconnect_reason,
                                DataPlaneSubscriptionEnd::BacklogExceeded { capacity_messages },
                            );
                            false
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => false,
                    },
                );
            if alive {
                state.subscribers.push(RuntimeSubscriber {
                    id,
                    consumer,
                    capacity_messages,
                    sender: subscriber,
                    disconnect_reason,
                });
            }
            CommandOutcome::Continue
        }
        RuntimeCommand::Unsubscribe { id } => {
            state.subscribers.retain(|subscriber| subscriber.id != id);
            CommandOutcome::Continue
        }
        RuntimeCommand::AcquireExclusive {
            owner_id,
            owner_name,
            driver_tx,
        } => {
            if let Some(active) = state.exclusive.as_ref() {
                let _ = driver_tx.send(Err(TransportError::busy(
                    "acquire_exclusive",
                    format!("data plane is already owned by {}", active.owner_name),
                )));
                state.exclusive_active.store(false, Ordering::Release);
                return CommandOutcome::Continue;
            }

            let Some(leased_driver) = driver.take() else {
                let _ = driver_tx.send(Err(TransportError::new(
                    TransportErrorKind::Io,
                    "acquire_exclusive",
                    "transport actor has no driver to lease",
                )));
                state.exclusive_active.store(false, Ordering::Release);
                return CommandOutcome::Continue;
            };

            let prefetched = take_unconsumed_bytes(state);
            state.exclusive = Some(ExclusiveState {
                owner_id,
                owner_name,
            });
            let payload = ExclusiveLeasePayload {
                driver: leased_driver,
                prefetched,
            };
            if let Err(error) = driver_tx.send(Ok(payload)) {
                if let Ok(returned) = error.0 {
                    *driver = Some(returned.driver);
                    restore_unconsumed_bytes(state, returned.prefetched);
                }
                state.exclusive = None;
                state.exclusive_active.store(false, Ordering::Release);
            }
            CommandOutcome::Continue
        }
        RuntimeCommand::ReturnExclusive {
            owner_id,
            driver: mut returned_driver,
            prefetched,
            ack,
        } => {
            let owner_matches = state
                .exclusive
                .as_ref()
                .is_some_and(|lease| lease.owner_id == owner_id);
            if !owner_matches {
                let _ = returned_driver.shutdown();
                let _ = ack.send(Err(TransportError::busy(
                    "return_exclusive",
                    "exclusive driver owner mismatch",
                )));
                return CommandOutcome::Continue;
            }
            if driver.is_some() {
                let _ = returned_driver.shutdown();
                let _ = ack.send(Err(TransportError::new(
                    TransportErrorKind::Io,
                    "return_exclusive",
                    "transport actor already owns a driver",
                )));
                return CommandOutcome::Continue;
            }

            *driver = Some(returned_driver);
            restore_unconsumed_bytes(state, prefetched);
            state.exclusive = None;
            state.exclusive_active.store(false, Ordering::Release);

            if state.shutdown_pending {
                let result = driver
                    .as_mut()
                    .expect("returned driver just installed")
                    .shutdown();
                let ack_result = result.clone();
                let _ = ack.send(ack_result);
                CommandOutcome::Shutdown
            } else {
                let _ = ack.send(Ok(()));
                CommandOutcome::Continue
            }
        }
        RuntimeCommand::ResizeTerminal { cols, rows, ack } => {
            if let Some(active) = state.exclusive.as_ref() {
                ack.send(Err(TransportError::busy(
                    "resize_terminal",
                    format!("data plane is exclusively owned by {}", active.owner_name),
                )));
                return CommandOutcome::Continue;
            }
            let Some(active_driver) = driver.as_mut() else {
                ack.send(Err(TransportError::new(
                    TransportErrorKind::Io,
                    "resize_terminal",
                    "transport actor has no active driver",
                )));
                return CommandOutcome::Continue;
            };
            let result = active_driver.resize_terminal(cols, rows);
            let close_info = result.as_ref().err().map(|error| {
                TransportCloseInfo::from_driver(
                    error.kind,
                    error.to_string(),
                    active_driver.close_metadata(),
                )
            });
            ack.send(result);
            close_info.map_or(CommandOutcome::Continue, CommandOutcome::Close)
        }
        RuntimeCommand::Shutdown { ack } => {
            if state.exclusive.is_some() {
                state.shutdown_pending = true;
                let _ = ack.send(Ok(()));
                return CommandOutcome::Continue;
            }

            let result = driver
                .as_mut()
                .map_or(Ok(()), |active_driver| active_driver.shutdown());
            let _ = ack.send(result);
            CommandOutcome::Shutdown
        }
    }
}

fn broadcast_close(subscribers: &mut Vec<RuntimeSubscriber>, info: TransportCloseInfo) {
    subscribers.retain(|subscriber| {
        match subscriber
            .sender
            .try_send(DataPlaneEvent::Closed(info.clone()))
        {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                set_subscription_end(
                    &subscriber.disconnect_reason,
                    DataPlaneSubscriptionEnd::BacklogExceeded {
                        capacity_messages: subscriber.capacity_messages,
                    },
                );
                log::warn!(
                    "DataPlane subscriber {} ({}) backlog was full while closing (capacity_messages={}); detaching consumer",
                    subscriber.id,
                    subscriber.consumer,
                    subscriber.capacity_messages
                );
                false
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        }
    });
}

fn next_owner_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn next_subscription_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;
    use std::time::Duration;

    struct MockStream {
        reads: VecDeque<ReadStatus>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl BlockingByteStream for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::thread::sleep(Duration::from_millis(1));
            match self.reads.pop_front().unwrap_or(ReadStatus::Idle) {
                ReadStatus::Data(n) => {
                    for (index, byte) in buf.iter_mut().take(n).enumerate() {
                        *byte = index as u8;
                    }
                    Ok(ReadStatus::Data(n))
                }
                other => Ok(other),
            }
        }

        fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct GatedStream {
        read_gate: mpsc::Receiver<()>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        fail_write: bool,
    }

    impl BlockingByteStream for GatedStream {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            let _ = self.read_gate.recv_timeout(Duration::from_millis(250));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
            if self.fail_write {
                Err(TransportError::new(
                    TransportErrorKind::Io,
                    "test_write",
                    "injected write failure",
                ))
            } else {
                self.writes.lock().unwrap().push(data.to_vec());
                Ok(())
            }
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct HandoffStream {
        read_started: Option<mpsc::Sender<()>>,
        read_release: mpsc::Receiver<()>,
        delivered: bool,
    }

    impl BlockingByteStream for HandoffStream {
        fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            if self.delivered {
                std::thread::sleep(Duration::from_millis(1));
                return Ok(ReadStatus::Idle);
            }
            if let Some(started) = self.read_started.take() {
                let _ = started.send(());
            }
            let _ = self.read_release.recv_timeout(Duration::from_secs(1));
            buf[0] = 0x43; // 'C' / WANTCRC
            buf[1] = 0x15; // NAK
            self.delivered = true;
            Ok(ReadStatus::Data(2))
        }

        fn write_all(&mut self, _data: &[u8]) -> Result<(), TransportError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct CountingStream {
        reads: Arc<AtomicU64>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl BlockingByteStream for CountingStream {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct ThreadRecordingStream {
        write_threads: Arc<Mutex<Vec<ThreadId>>>,
    }

    impl BlockingByteStream for ThreadRecordingStream {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, _data: &[u8]) -> Result<(), TransportError> {
            self.write_threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct FailOnceStream {
        failed: bool,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl BlockingByteStream for FailOnceStream {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
            if !self.failed {
                self.failed = true;
                return Err(TransportError::new(
                    TransportErrorKind::Io,
                    "test_exclusive_write",
                    "injected exclusive write failure",
                ));
            }
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn acquire_while_read_is_in_flight() -> (DataPlaneRuntime, DataPlaneSubscription, ExclusiveIo) {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let runtime = DataPlaneRuntime::spawn(Box::new(HandoffStream {
            read_started: Some(started_tx),
            read_release: release_rx,
            delivered: false,
        }));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("actor should enter the physical read");
        let subscription = runtime.handle.subscribe("test").unwrap();

        let handle = runtime.handle.clone();
        let (lease_tx, lease_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = lease_tx.send(handle.acquire_exclusive("handoff-test"));
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !runtime.handle.exclusive_active.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "exclusive request not observed"
            );
            std::thread::yield_now();
        }
        release_tx.send(()).unwrap();
        let lease = lease_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("exclusive acquisition should finish")
            .expect("exclusive acquisition should succeed");
        (runtime, subscription, lease)
    }

    #[test]
    fn slow_subscriber_is_detached_instead_of_blocking_or_growing_unbounded() {
        let (subscriber_tx, subscriber_rx) = mpsc::sync_channel(1);
        let mut state = RuntimeLoopState {
            subscribers: vec![RuntimeSubscriber {
                id: 42,
                consumer: "slow-test".into(),
                capacity_messages: 1,
                sender: subscriber_tx,
                disconnect_reason: Arc::new(Mutex::new(None)),
            }],
            startup_buffer: VecDeque::new(),
            startup_buffer_bytes: 0,
            handoff_buffer: VecDeque::new(),
            exclusive: None,
            shutdown_pending: false,
            tx_bytes: Arc::new(AtomicU64::new(0)),
            rx_bytes: Arc::new(AtomicU64::new(0)),
            exclusive_active: Arc::new(AtomicBool::new(false)),
        };

        publish_shared_data(&mut state, vec![1]);
        assert_eq!(state.subscribers.len(), 1);
        let reason = state.subscribers[0].disconnect_reason.clone();
        publish_shared_data(&mut state, vec![2]);
        assert!(state.subscribers.is_empty());
        assert_eq!(
            reason.lock().unwrap().clone(),
            Some(DataPlaneSubscriptionEnd::BacklogExceeded {
                capacity_messages: 1
            })
        );
        match subscriber_rx.try_recv().unwrap() {
            DataPlaneEvent::Data(data) => assert_eq!(data, vec![1]),
            DataPlaneEvent::Closed(info) => panic!("unexpected close: {}", info.reason),
        }
    }

    #[test]
    fn exclusive_handoff_preserves_bytes_read_during_acquisition() {
        let (runtime, subscription, mut lease) = acquire_while_read_is_in_flight();
        let mut buf = [0u8; 2];
        assert_eq!(lease.read(&mut buf).unwrap(), 2);
        assert_eq!(buf, [0x43, 0x15]);
        assert!(subscription
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        drop(lease);
        runtime.join();
    }

    #[test]
    fn unread_handoff_bytes_return_to_shared_subscriber() {
        let (runtime, subscription, mut lease) = acquire_while_read_is_in_flight();
        let mut first = [0u8; 1];
        assert_eq!(lease.read(&mut first).unwrap(), 1);
        assert_eq!(first, [0x43]);
        drop(lease);

        match subscription.recv_timeout(Duration::from_secs(1)).unwrap() {
            DataPlaneEvent::Data(data) => assert_eq!(data, vec![0x15]),
            DataPlaneEvent::Closed(info) => panic!("unexpected close: {}", info.reason),
        }
        runtime.join();
    }

    #[test]
    fn exclusive_lease_blocks_normal_writes_and_restores_shared_mode() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes: writes.clone(),
        }));
        let mut lease = runtime.handle.acquire_exclusive("test").unwrap();
        assert_eq!(
            runtime.handle.write(b"blocked").unwrap_err().kind,
            TransportErrorKind::Busy
        );
        lease.write_all(b"owned").unwrap();
        lease.flush().unwrap();
        lease.release().unwrap();
        runtime.handle.write(b"shared").unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            &[b"owned".to_vec(), b"shared".to_vec()]
        );
        runtime.join();
    }

    #[test]
    fn exclusive_read_times_out_while_transport_is_idle() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes,
        }));
        let mut lease = runtime.handle.acquire_exclusive("test-timeout").unwrap();
        let mut buf = [0u8; 8];
        let error = lease.read(&mut buf).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(lease);
        runtime.join();
    }

    #[test]
    fn exclusive_mode_stops_background_driver_reads() {
        let reads = Arc::new(AtomicU64::new(0));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(CountingStream {
            reads: reads.clone(),
            writes,
        }));
        let mut lease = runtime
            .handle
            .acquire_exclusive("physical-driver-lease")
            .unwrap();
        let after_acquire = reads.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(reads.load(Ordering::SeqCst), after_acquire);

        let mut buf = [0u8; 1];
        let error = lease.read(&mut buf).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(reads.load(Ordering::SeqCst), after_acquire + 1);
        drop(lease);
        runtime.join();
    }

    #[test]
    fn exclusive_driver_runs_on_owner_thread_and_returns_to_actor_thread() {
        let write_threads = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(ThreadRecordingStream {
            write_threads: write_threads.clone(),
        }));

        runtime.handle.write(b"shared-before").unwrap();
        let actor_thread = write_threads.lock().unwrap()[0];
        let owner_thread = std::thread::current().id();
        assert_ne!(actor_thread, owner_thread);

        let mut lease = runtime.handle.acquire_exclusive("owner-thread").unwrap();
        lease.write_all(b"exclusive").unwrap();
        lease.flush().unwrap();
        lease.release().unwrap();
        runtime.handle.write(b"shared-after").unwrap();

        let threads = write_threads.lock().unwrap().clone();
        assert_eq!(threads.len(), 3);
        assert_eq!(threads[0], actor_thread);
        assert_eq!(threads[1], owner_thread);
        assert_eq!(threads[2], actor_thread);
        runtime.join();
    }

    #[test]
    fn exclusive_io_error_does_not_close_shared_runtime() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(FailOnceStream {
            failed: false,
            writes: writes.clone(),
        }));

        let mut lease = runtime
            .handle
            .acquire_exclusive("recover-after-error")
            .unwrap();
        assert!(lease.write_all(b"fail-once").is_err());
        lease.release().unwrap();

        assert!(runtime.handle.is_connected());
        runtime.handle.write(b"shared-recovers").unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            &[b"shared-recovers".to_vec()]
        );
        runtime.join();
    }

    #[test]
    fn shutdown_waits_for_exclusive_driver_return_without_deadlocking_request() {
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes: Arc::new(Mutex::new(Vec::new())),
        }));
        let lease = runtime.handle.acquire_exclusive("shutdown-test").unwrap();
        let handle = runtime.handle.clone();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_tx.send(handle.shutdown());
        });
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("shutdown request must be acknowledged while driver is leased")
            .unwrap();
        drop(lease);
        runtime.join();
    }

    #[test]
    fn synchronous_write_waits_for_driver_ack_on_worker_paths() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let (gate_tx, gate_rx) = mpsc::channel();
        let runtime = DataPlaneRuntime::spawn(Box::new(GatedStream {
            read_gate: gate_rx,
            writes: writes.clone(),
            fail_write: false,
        }));
        std::thread::sleep(Duration::from_millis(10));

        let handle = runtime.handle.clone();
        let (result_tx, result_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = result_tx.send(handle.write(b"confirmed"));
        });
        assert!(result_rx.recv_timeout(Duration::from_millis(50)).is_err());
        gate_tx.send(()).unwrap();
        result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("confirmed write should complete after actor services it")
            .unwrap();
        assert_eq!(writes.lock().unwrap().as_slice(), &[b"confirmed".to_vec()]);
        runtime.join();
    }

    #[test]
    fn async_write_awaits_ack_without_requiring_an_ambient_driver_runtime() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes: writes.clone(),
        }));
        let handle = runtime.handle.clone();
        let async_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        async_runtime.block_on(async {
            handle.write_async(vec![1, 2, 3]).await.unwrap();
            handle.write_async(vec![4, 5]).await.unwrap();
        });
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            &[vec![1, 2, 3], vec![4, 5]]
        );
        runtime.join();
    }

    #[test]
    fn confirmed_write_failure_is_published_as_close_event() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let (gate_tx, gate_rx) = mpsc::channel();
        let runtime = DataPlaneRuntime::spawn(Box::new(GatedStream {
            read_gate: gate_rx,
            writes,
            fail_write: true,
        }));
        let subscription = runtime.handle.subscribe("test").unwrap();
        let handle = runtime.handle.clone();
        let writer = std::thread::spawn(move || handle.write(b"fail"));
        gate_tx.send(()).unwrap();
        let error = writer.join().unwrap().unwrap_err();
        assert_eq!(error.kind, TransportErrorKind::Io);

        match subscription.recv_timeout(Duration::from_secs(1)).unwrap() {
            DataPlaneEvent::Closed(info) => {
                assert_eq!(info.kind, TransportErrorKind::Io);
                assert!(info.reason.contains("injected write failure"));
            }
            DataPlaneEvent::Data(data) => panic!("unexpected data: {data:?}"),
        }
        runtime.join();
    }

    #[test]
    fn queued_commands_are_not_consumed_by_disconnect_probe() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes: writes.clone(),
        }));
        for value in 0u8..32 {
            runtime.handle.write(&[value]).unwrap();
        }
        assert_eq!(writes.lock().unwrap().len(), 32);
        runtime.join();
    }

    #[test]
    fn data_received_before_first_subscription_is_delivered_once() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::from([ReadStatus::Data(3), ReadStatus::Idle]),
            writes,
        }));
        std::thread::sleep(Duration::from_millis(10));
        let subscription = runtime.handle.subscribe("test").unwrap();
        match subscription.recv_timeout(Duration::from_secs(1)).unwrap() {
            DataPlaneEvent::Data(data) => assert_eq!(data, vec![0, 1, 2]),
            DataPlaneEvent::Closed(info) => panic!("unexpected close: {}", info.reason),
        }
        drop(subscription);
        runtime.join();
    }

    #[test]
    fn dropped_subscription_unregisters_without_waiting_for_data() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes,
        }));
        let subscription = runtime.handle.subscribe("test").unwrap();
        drop(subscription);
        runtime.handle.write(b"wake").unwrap();
        runtime.join();
    }
}
