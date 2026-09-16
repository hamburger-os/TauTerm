use super::config::RttConfig;
use super::error::{RttError, RttErrorCode};
use super::model::{RttChunkDto, RttHistoryResponse, RttPhase, RttSnapshot, StoredRttChunk};
use super::worker::{self, WorkerCommand};
use crate::kernel::plugin_adapter::SessionService;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const HISTORY_PER_CHANNEL_BYTES: usize = 256 * 1024;
const HISTORY_PER_SESSION_BYTES: usize = 2 * 1024 * 1024;
const MAX_HISTORY_RESPONSE_CHUNKS: usize = 512;
const COMMAND_QUEUE_CAPACITY: usize = 64;

#[derive(Default)]
struct ChannelHistory {
    chunks: VecDeque<StoredRttChunk>,
    bytes: usize,
    dropped_chunks: u64,
    dropped_bytes: u64,
}

#[derive(Default)]
struct HistoryStore {
    channels: BTreeMap<u32, ChannelHistory>,
    total_bytes: usize,
}

impl HistoryStore {
    fn push(&mut self, chunk: StoredRttChunk) -> (u64, u64) {
        let channel_index = chunk.channel_index;
        let chunk_len = chunk.data.len();
        let history = self.channels.entry(channel_index).or_default();
        history.bytes += chunk_len;
        self.total_bytes += chunk_len;
        history.chunks.push_back(chunk);

        while self
            .channels
            .get(&channel_index)
            .is_some_and(|history| history.bytes > HISTORY_PER_CHANNEL_BYTES)
        {
            self.drop_front(channel_index);
        }
        while self.total_bytes > HISTORY_PER_SESSION_BYTES {
            let oldest_channel = self
                .channels
                .iter()
                .filter_map(|(channel, history)| {
                    history
                        .chunks
                        .front()
                        .map(|chunk| (*channel, chunk.sequence))
                })
                .min_by_key(|(_, sequence)| *sequence)
                .map(|(channel, _)| channel);
            let Some(channel) = oldest_channel else {
                break;
            };
            self.drop_front(channel);
        }
        self.loss_totals()
    }

    fn drop_front(&mut self, channel_index: u32) {
        let Some(history) = self.channels.get_mut(&channel_index) else {
            return;
        };
        let Some(chunk) = history.chunks.pop_front() else {
            return;
        };
        let len = chunk.data.len();
        history.bytes = history.bytes.saturating_sub(len);
        history.dropped_chunks = history.dropped_chunks.saturating_add(1);
        history.dropped_bytes = history.dropped_bytes.saturating_add(len as u64);
        self.total_bytes = self.total_bytes.saturating_sub(len);
    }

    fn loss_totals(&self) -> (u64, u64) {
        self.channels
            .values()
            .fold((0, 0), |(chunks, bytes), item| {
                (
                    chunks.saturating_add(item.dropped_chunks),
                    bytes.saturating_add(item.dropped_bytes),
                )
            })
    }

    fn response(&self, channel_index: u32, after_sequence: Option<u64>) -> RttHistoryResponse {
        let history = self.channels.get(&channel_index);
        let chunks = match (history, after_sequence) {
            (Some(history), Some(sequence)) => history
                .chunks
                .iter()
                .filter(|chunk| chunk.sequence > sequence)
                .take(MAX_HISTORY_RESPONSE_CHUNKS)
                .collect::<Vec<_>>(),
            (Some(history), None) => history
                .chunks
                .iter()
                .skip(history.chunks.len().saturating_sub(MAX_HISTORY_RESPONSE_CHUNKS))
                .collect::<Vec<_>>(),
            (None, _) => Vec::new(),
        }
        .into_iter()
        .map(|chunk| RttChunkDto {
            sequence: chunk.sequence,
            timestamp_ms: chunk.timestamp_ms,
            channel_index: chunk.channel_index,
            data_b64: BASE64.encode(&chunk.data),
        })
        .collect();
        RttHistoryResponse {
            chunks,
            oldest_sequence: history
                .and_then(|history| history.chunks.front().map(|chunk| chunk.sequence)),
            newest_sequence: history
                .and_then(|history| history.chunks.back().map(|chunk| chunk.sequence)),
            dropped_chunks: history.map_or(0, |history| history.dropped_chunks),
            dropped_bytes: history.map_or(0, |history| history.dropped_bytes),
        }
    }
}

pub(super) struct RttShared {
    snapshot: Mutex<RttSnapshot>,
    history: Mutex<HistoryStore>,
    next_sequence: AtomicU64,
}

impl RttShared {
    fn new() -> Self {
        Self {
            snapshot: Mutex::new(RttSnapshot::default()),
            history: Mutex::new(HistoryStore::default()),
            next_sequence: AtomicU64::new(1),
        }
    }

    pub(super) fn set_phase(&self, phase: RttPhase) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = phase;
        }
    }

    pub(super) fn set_running(
        &self,
        descriptor: super::model::RttBackendDescriptor,
        channels: Vec<super::model::RttChannelInfo>,
    ) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Running;
            snapshot.backend = Some(descriptor);
            snapshot.channels = channels;
            snapshot.last_error = None;
        }
    }

    pub(super) fn update_channels(&self, channels: Vec<super::model::RttChannelInfo>) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.channels = channels;
        }
    }

    pub(super) fn record_rx(&self, channel_index: u32, data: Vec<u8>) -> StoredRttChunk {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let chunk = StoredRttChunk {
            sequence,
            timestamp_ms: now_ms(),
            channel_index,
            data,
        };
        let loss = self
            .history
            .lock()
            .map(|mut history| history.push(chunk.clone()))
            .unwrap_or((0, 0));
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.rx_bytes = snapshot.rx_bytes.saturating_add(chunk.data.len() as u64);
            snapshot.dropped_history_chunks = loss.0;
            snapshot.dropped_history_bytes = loss.1;
        }
        chunk
    }

    pub(super) fn record_tx(&self, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.tx_bytes = snapshot.tx_bytes.saturating_add(bytes as u64);
        }
    }

    pub(super) fn set_error(&self, error: RttError) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Faulted;
            snapshot.last_error = Some(error);
        }
    }

    pub(super) fn snapshot(&self) -> RttSnapshot {
        self.snapshot
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    fn history(&self, channel_index: u32, after_sequence: Option<u64>) -> RttHistoryResponse {
        self.history
            .lock()
            .map(|history| history.response(channel_index, after_sequence))
            .unwrap_or_else(|_| RttHistoryResponse {
                chunks: Vec::new(),
                oldest_sequence: None,
                newest_sequence: None,
                dropped_chunks: 0,
                dropped_bytes: 0,
            })
    }
}

pub struct RttRuntime {
    config: RttConfig,
    shared: Arc<RttShared>,
    command_tx: Mutex<Option<mpsc::SyncSender<WorkerCommand>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    shutting_down: Arc<AtomicBool>,
    worker_exited: Arc<AtomicBool>,
    closed: AtomicBool,
}

impl RttRuntime {
    pub fn new(config: RttConfig) -> Self {
        Self {
            config,
            shared: Arc::new(RttShared::new()),
            command_tx: Mutex::new(None),
            worker: Mutex::new(None),
            shutting_down: Arc::new(AtomicBool::new(false)),
            worker_exited: Arc::new(AtomicBool::new(false)),
            closed: AtomicBool::new(false),
        }
    }

    pub fn start(&self, app: AppHandle, session_id: &str) -> Result<(), RttError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(RttError::new(RttErrorCode::Cancelled, "RTT 会话已关闭"));
        }
        self.shutting_down.store(false, Ordering::Release);
        self.worker_exited.store(false, Ordering::Release);
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let config = self.config.clone();
        let shared = Arc::clone(&self.shared);
        let shutting_down = Arc::clone(&self.shutting_down);
        let worker_exited = Arc::clone(&self.worker_exited);
        let worker_session_id = session_id.to_string();
        let handle = std::thread::Builder::new()
            .name(format!("rtt-{session_id}"))
            .spawn(move || {
                worker::run(
                    config,
                    app,
                    worker_session_id,
                    shared,
                    shutting_down,
                    worker_exited,
                    command_rx,
                    startup_tx,
                );
            })
            .map_err(|error| RttError::backend(format!("启动 RTT worker 失败: {error}")))?;
        *self
            .command_tx
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? = Some(command_tx);
        *self
            .worker
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? = Some(handle);
        let startup_timeout = self
            .config
            .attach_timeout
            .saturating_add(Duration::from_secs(5));
        match startup_rx.recv_timeout(startup_timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.shutdown_inner(false);
                Err(RttError::new(
                    RttErrorCode::TargetAttachFailed,
                    "RTT backend 启动超时",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.shutdown_inner(false);
                Err(RttError::backend("RTT worker 在完成启动前退出"))
            }
        }
    }

    pub fn snapshot(&self) -> RttSnapshot {
        self.shared.snapshot()
    }

    pub fn history(&self, channel_index: u32, after_sequence: Option<u64>) -> RttHistoryResponse {
        self.shared.history(channel_index, after_sequence)
    }

    pub fn write(&self, channel_index: u32, data: Vec<u8>) -> Result<usize, RttError> {
        if data.is_empty() {
            return Ok(0);
        }
        let tx = self
            .command_tx
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .clone()
            .ok_or_else(|| RttError::new(RttErrorCode::Cancelled, "RTT worker 未运行"))?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        tx.try_send(WorkerCommand::Write {
            channel_index,
            data,
            reply: reply_tx,
        })
        .map_err(|error| RttError::backend(format!("RTT 命令队列不可用: {error}")))?;
        reply_rx
            .recv_timeout(
                self.config
                    .write_timeout
                    .saturating_add(Duration::from_secs(1)),
            )
            .map_err(|_| RttError::new(RttErrorCode::RttWriteTimeout, "等待 RTT 写入结果超时"))?
    }

    pub fn refresh_channels(&self) -> Result<Vec<super::model::RttChannelInfo>, RttError> {
        let tx = self
            .command_tx
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .clone()
            .ok_or_else(|| RttError::new(RttErrorCode::Cancelled, "RTT worker 未运行"))?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        tx.try_send(WorkerCommand::RefreshChannels { reply: reply_tx })
            .map_err(|error| RttError::backend(format!("RTT 命令队列不可用: {error}")))?;
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| RttError::backend("刷新 RTT Channel 超时"))?
    }

    fn shutdown_inner(&self, permanent: bool) {
        if permanent {
            self.closed.store(true, Ordering::Release);
        }
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }
        self.shared.set_phase(RttPhase::Stopping);
        if let Ok(mut tx_slot) = self.command_tx.lock() {
            if let Some(tx) = tx_slot.take() {
                if !self.worker_exited.load(Ordering::Acquire) {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx.try_send(WorkerCommand::Shutdown { reply: reply_tx }).is_ok() {
                        let _ = reply_rx.recv_timeout(Duration::from_secs(2));
                    }
                }
            }
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                if handle.thread().id() != std::thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
        self.shared.set_phase(RttPhase::Idle);
    }
}

impl SessionService for RttRuntime {
    fn shutdown(&self) {
        self.shutdown_inner(true);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_reports_loss() {
        let mut history = HistoryStore::default();
        for sequence in 1..=300u64 {
            history.push(StoredRttChunk {
                sequence,
                timestamp_ms: sequence,
                channel_index: 0,
                data: vec![0x55; 1024],
            });
        }
        let response = history.response(0, None);
        assert!(response.dropped_chunks > 0);
        assert!(response.dropped_bytes > 0);
        assert!(response.oldest_sequence.unwrap_or_default() > 1);
    }

    #[test]
    fn initial_history_response_prefers_latest_chunks() {
        let mut history = HistoryStore::default();
        for sequence in 1..=600u64 {
            history.push(StoredRttChunk {
                sequence,
                timestamp_ms: sequence,
                channel_index: 0,
                data: vec![0x11],
            });
        }
        let response = history.response(0, None);
        assert_eq!(response.chunks.len(), MAX_HISTORY_RESPONSE_CHUNKS);
        assert_eq!(response.chunks.first().map(|chunk| chunk.sequence), Some(89));
        assert_eq!(response.chunks.last().map(|chunk| chunk.sequence), Some(600));
    }
}
