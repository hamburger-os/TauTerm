use std::collections::VecDeque;
use std::io::{Read, Write};
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus, StreamCloseMetadata};

const COMMAND_CAPACITY: usize = 256;
const EXCLUSIVE_RX_CAPACITY: usize = 256;
const EXCLUSIVE_READ_SLICE: std::time::Duration = std::time::Duration::from_millis(20);
const READ_BUFFER_SIZE: usize = 16 * 1024;
const STARTUP_BUFFER_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub enum DataPlaneEvent {
    Data(Vec<u8>),
    Closed(TransportCloseInfo),
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
    terminal_control: bool,
}

pub struct DataPlaneSubscription {
    id: u64,
    command_tx: mpsc::SyncSender<RuntimeCommand>,
    receiver: mpsc::Receiver<DataPlaneEvent>,
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
    pub fn write(&self, data: &[u8]) -> Result<(), TransportError> {
        self.write_owned(None, data)
    }

    fn write_owned(&self, owner: Option<u64>, data: &[u8]) -> Result<(), TransportError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::Write {
                owner,
                data: data.to_vec(),
                ack: ack_tx,
            })
            .map_err(|_| {
                TransportError::new(
                    TransportErrorKind::RemoteClosed,
                    "write",
                    "transport runtime is closed",
                )
            })?;
        ack_rx.recv().map_err(|_| {
            TransportError::new(
                TransportErrorKind::RemoteClosed,
                "write",
                "transport runtime closed before acknowledging write",
            )
        })?
    }

    pub fn subscribe(&self) -> Result<DataPlaneSubscription, TransportError> {
        let id = next_subscription_id();
        let (event_tx, event_rx) = mpsc::channel();
        self.command_tx
            .send(RuntimeCommand::Subscribe {
                id,
                subscriber: event_tx,
            })
            .map_err(|_| {
                TransportError::new(
                    TransportErrorKind::RemoteClosed,
                    "subscribe",
                    "transport runtime is closed",
                )
            })?;
        Ok(DataPlaneSubscription {
            id,
            command_tx: self.command_tx.clone(),
            receiver: event_rx,
        })
    }

    pub fn acquire_exclusive(
        &self,
        owner_name: impl Into<String>,
        purge_input: bool,
    ) -> Result<ExclusiveIo, TransportError> {
        let owner_id = next_owner_id();
        let (data_tx, data_rx) = mpsc::sync_channel(EXCLUSIVE_RX_CAPACITY);
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::AcquireExclusive {
                owner_id,
                owner_name: owner_name.into(),
                data_tx,
                purge_input,
                ack: ack_tx,
            })
            .map_err(|_| {
                TransportError::new(
                    TransportErrorKind::RemoteClosed,
                    "acquire_exclusive",
                    "transport runtime is closed",
                )
            })?;
        ack_rx.recv().map_err(|_| {
            TransportError::new(
                TransportErrorKind::RemoteClosed,
                "acquire_exclusive",
                "transport runtime closed before granting lease",
            )
        })??;
        Ok(ExclusiveIo {
            owner_id,
            command_tx: self.command_tx.clone(),
            data_rx,
            read_buf: VecDeque::new(),
            released: false,
        })
    }

    pub fn resize_terminal(&self, cols: u32, rows: u32) -> Result<(), TransportError> {
        if !self.terminal_control {
            return Err(TransportError::unsupported(
                "resize_terminal",
                "session has no terminal-control capability",
            ));
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::ResizeTerminal {
                cols,
                rows,
                ack: ack_tx,
            })
            .map_err(|_| {
                TransportError::new(
                    TransportErrorKind::RemoteClosed,
                    "resize_terminal",
                    "transport runtime is closed",
                )
            })?;
        ack_rx.recv().map_err(|_| {
            TransportError::new(
                TransportErrorKind::RemoteClosed,
                "resize_terminal",
                "transport runtime closed before resize acknowledgement",
            )
        })?
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
        let handle = DataPlaneHandle {
            command_tx,
            tx_bytes: tx_bytes.clone(),
            rx_bytes: rx_bytes.clone(),
            connected: connected.clone(),
            terminal_control,
        };
        let thread = std::thread::spawn(move || {
            run_blocking_runtime(driver, command_rx, tx_bytes, rx_bytes, connected)
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

pub struct ExclusiveIo {
    owner_id: u64,
    command_tx: mpsc::SyncSender<RuntimeCommand>,
    data_rx: mpsc::Receiver<Vec<u8>>,
    read_buf: VecDeque<u8>,
    released: bool,
}

impl ExclusiveIo {
    pub fn release(mut self) -> Result<(), TransportError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), TransportError> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        self.command_tx
            .send(RuntimeCommand::ReleaseExclusive {
                owner_id: self.owner_id,
            })
            .map_err(|_| {
                TransportError::new(
                    TransportErrorKind::RemoteClosed,
                    "release_exclusive",
                    "transport runtime is closed",
                )
            })
    }
}

impl Read for ExclusiveIo {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.read_buf.is_empty() {
            match self.data_rx.recv_timeout(EXCLUSIVE_READ_SLICE) {
                Ok(chunk) => self.read_buf.extend(chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "exclusive data-plane read timed out",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "transport closed while exclusive lease was active",
                    ));
                }
            }
        }
        let n = buf.len().min(self.read_buf.len());
        for slot in &mut buf[..n] {
            *slot = self.read_buf.pop_front().expect("length checked");
        }
        Ok(n)
    }
}

impl Write for ExclusiveIo {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.command_tx
            .send(RuntimeCommand::Write {
                owner: Some(self.owner_id),
                data: buf.to_vec(),
                ack: ack_tx,
            })
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "transport closed"))?;
        ack_rx
            .recv()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "transport closed"))?
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for ExclusiveIo {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

enum RuntimeCommand {
    Write {
        owner: Option<u64>,
        data: Vec<u8>,
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
    Subscribe {
        id: u64,
        subscriber: mpsc::Sender<DataPlaneEvent>,
    },
    Unsubscribe {
        id: u64,
    },
    AcquireExclusive {
        owner_id: u64,
        owner_name: String,
        data_tx: mpsc::SyncSender<Vec<u8>>,
        purge_input: bool,
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
    ReleaseExclusive {
        owner_id: u64,
    },
    ResizeTerminal {
        cols: u32,
        rows: u32,
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
    Shutdown {
        ack: mpsc::SyncSender<Result<(), TransportError>>,
    },
}

struct ExclusiveState {
    owner_id: u64,
    owner_name: String,
    data_tx: mpsc::SyncSender<Vec<u8>>,
}

fn run_blocking_runtime(
    mut driver: Box<dyn BlockingByteStream>,
    commands: mpsc::Receiver<RuntimeCommand>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
) {
    let mut subscribers: Vec<(u64, mpsc::Sender<DataPlaneEvent>)> = Vec::new();
    let mut startup_buffer: VecDeque<Vec<u8>> = VecDeque::new();
    let mut startup_buffer_bytes = 0usize;
    let mut exclusive: Option<ExclusiveState> = None;
    let mut read_buf = [0u8; READ_BUFFER_SIZE];
    let mut closing = false;

    while !closing {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    closing = handle_command(
                        command,
                        &mut *driver,
                        &mut subscribers,
                        &mut startup_buffer,
                        &mut startup_buffer_bytes,
                        &mut exclusive,
                        &tx_bytes,
                    );
                    if closing {
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

        match driver.read(&mut read_buf) {
            Ok(ReadStatus::Data(n)) if n > 0 => {
                rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                let data = read_buf[..n].to_vec();
                if let Some(lease) = exclusive.as_ref() {
                    if lease.data_tx.send(data).is_err() {
                        exclusive = None;
                    }
                } else if subscribers.is_empty() {
                    startup_buffer_bytes += data.len();
                    startup_buffer.push_back(data);
                    while startup_buffer_bytes > STARTUP_BUFFER_LIMIT {
                        if let Some(dropped) = startup_buffer.pop_front() {
                            startup_buffer_bytes =
                                startup_buffer_bytes.saturating_sub(dropped.len());
                        } else {
                            break;
                        }
                    }
                } else {
                    subscribers.retain(|(_, subscriber)| {
                        subscriber.send(DataPlaneEvent::Data(data.clone())).is_ok()
                    });
                }
            }
            Ok(ReadStatus::Data(_)) | Ok(ReadStatus::Idle) => {}
            Ok(ReadStatus::Eof) => {
                broadcast_close(
                    &mut subscribers,
                    TransportCloseInfo::from_driver(
                        TransportErrorKind::RemoteClosed,
                        "remote endpoint closed the stream",
                        driver.close_metadata(),
                    ),
                );
                break;
            }
            Err(error) => {
                broadcast_close(
                    &mut subscribers,
                    TransportCloseInfo::from_driver(
                        error.kind,
                        error.to_string(),
                        driver.close_metadata(),
                    ),
                );
                break;
            }
        }
    }

    connected.store(false, Ordering::Release);
    let _ = driver.shutdown();
}

fn handle_command(
    command: RuntimeCommand,
    driver: &mut dyn BlockingByteStream,
    subscribers: &mut Vec<(u64, mpsc::Sender<DataPlaneEvent>)>,
    startup_buffer: &mut VecDeque<Vec<u8>>,
    startup_buffer_bytes: &mut usize,
    exclusive: &mut Option<ExclusiveState>,
    tx_bytes: &Arc<AtomicU64>,
) -> bool {
    match command {
        RuntimeCommand::Write { owner, data, ack } => {
            let allowed = match exclusive.as_ref() {
                None => owner.is_none(),
                Some(lease) => owner == Some(lease.owner_id),
            };
            let result = if !allowed {
                Err(TransportError::busy(
                    "write",
                    exclusive
                        .as_ref()
                        .map(|lease| {
                            format!("data plane is exclusively owned by {}", lease.owner_name)
                        })
                        .unwrap_or_else(|| "write owner mismatch".into()),
                ))
            } else {
                let result = driver.write_all(&data).and_then(|_| driver.flush());
                if result.is_ok() {
                    tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                }
                result
            };
            let _ = ack.send(result);
            false
        }
        RuntimeCommand::Subscribe { id, subscriber } => {
            let mut alive = true;
            while let Some(data) = startup_buffer.pop_front() {
                *startup_buffer_bytes = startup_buffer_bytes.saturating_sub(data.len());
                if subscriber.send(DataPlaneEvent::Data(data)).is_err() {
                    alive = false;
                    break;
                }
            }
            if alive {
                subscribers.push((id, subscriber));
            } else {
                startup_buffer.clear();
                *startup_buffer_bytes = 0;
            }
            false
        }
        RuntimeCommand::Unsubscribe { id } => {
            subscribers.retain(|(subscriber_id, _)| *subscriber_id != id);
            false
        }
        RuntimeCommand::AcquireExclusive {
            owner_id,
            owner_name,
            data_tx,
            purge_input,
            ack,
        } => {
            let result = if let Some(active) = exclusive.as_ref() {
                Err(TransportError::busy(
                    "acquire_exclusive",
                    format!("data plane is already owned by {}", active.owner_name),
                ))
            } else {
                let purge_result = if purge_input {
                    driver.purge_input()
                } else {
                    Ok(())
                };
                purge_result.map(|()| {
                    *exclusive = Some(ExclusiveState {
                        owner_id,
                        owner_name,
                        data_tx,
                    });
                })
            };
            let _ = ack.send(result);
            false
        }
        RuntimeCommand::ReleaseExclusive { owner_id } => {
            if exclusive
                .as_ref()
                .is_some_and(|lease| lease.owner_id == owner_id)
            {
                *exclusive = None;
            }
            false
        }
        RuntimeCommand::ResizeTerminal { cols, rows, ack } => {
            let _ = ack.send(driver.resize_terminal(cols, rows));
            false
        }
        RuntimeCommand::Shutdown { ack } => {
            let result = driver.shutdown();
            let _ = ack.send(result);
            true
        }
    }
}

fn broadcast_close(
    subscribers: &mut Vec<(u64, mpsc::Sender<DataPlaneEvent>)>,
    info: TransportCloseInfo,
) {
    subscribers.retain(|(_, subscriber)| {
        subscriber
            .send(DataPlaneEvent::Closed(info.clone()))
            .is_ok()
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

    #[test]
    fn exclusive_lease_blocks_normal_writes_and_restores_shared_mode() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {
            reads: VecDeque::new(),
            writes: writes.clone(),
        }));
        let mut lease = runtime.handle.acquire_exclusive("test", false).unwrap();
        assert_eq!(
            runtime.handle.write(b"blocked").unwrap_err().kind,
            TransportErrorKind::Busy
        );
        lease.write_all(b"owned").unwrap();
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
        let mut lease = runtime
            .handle
            .acquire_exclusive("test-timeout", false)
            .unwrap();
        let mut buf = [0u8; 8];
        let error = lease.read(&mut buf).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(lease);
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
        let subscription = runtime.handle.subscribe().unwrap();
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
        let subscription = runtime.handle.subscribe().unwrap();
        drop(subscription);
        runtime.handle.write(b"wake").unwrap();
        runtime.join();
    }
}
