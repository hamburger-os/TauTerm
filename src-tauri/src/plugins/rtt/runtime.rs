use super::config::RttConfig;
use super::error::{RttError, RttErrorCode};
use super::model::{RttChunkDto, RttHistoryResponse, RttPhase, RttSnapshot, StoredRttChunk};
use super::worker::{self, WorkerCommand, WorkerContext};
use crate::kernel::plugin_adapter::SessionService;
use crate::session::{AutomationIo, AutomationRx, AutomationRxEvent, SessionIoError};
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
const AUTOMATION_SUBSCRIPTION_CAPACITY: usize = 1024;
const CHANNEL_REFRESH_REPLY_TIMEOUT: Duration = Duration::from_secs(3);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

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

    fn response(
        &self,
        generation: u64,
        channel_index: u32,
        after_sequence: Option<u64>,
    ) -> RttHistoryResponse {
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
                .skip(
                    history
                        .chunks
                        .len()
                        .saturating_sub(MAX_HISTORY_RESPONSE_CHUNKS),
                )
                .collect::<Vec<_>>(),
            (None, _) => Vec::new(),
        }
        .into_iter()
        .map(RttChunkDto::from_stored)
        .collect();
        RttHistoryResponse {
            generation,
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
    channel_offsets: Mutex<BTreeMap<u32, u64>>,
    automation_channel: Mutex<Option<u32>>,
    automation_subscribers: Mutex<Vec<mpsc::SyncSender<Vec<u8>>>>,
}

impl RttShared {
    pub(super) fn new(generation: u64) -> Self {
        Self {
            snapshot: Mutex::new(RttSnapshot {
                generation,
                ..RttSnapshot::default()
            }),
            history: Mutex::new(HistoryStore::default()),
            next_sequence: AtomicU64::new(1),
            channel_offsets: Mutex::new(BTreeMap::new()),
            automation_channel: Mutex::new(None),
            automation_subscribers: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn generation(&self) -> u64 {
        self.snapshot()
            .generation
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
        if let Ok(mut selected) = self.automation_channel.lock() {
            let still_writable = selected.is_some_and(|index| {
                channels
                    .iter()
                    .any(|channel| channel.index == index && channel.down.is_some())
            });
            if !still_writable {
                *selected = channels
                    .iter()
                    .find(|channel| channel.index == 0 && channel.down.is_some())
                    .or_else(|| channels.iter().find(|channel| channel.down.is_some()))
                    .map(|channel| channel.index);
            }
        }
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Running;
            snapshot.backend = Some(descriptor);
            snapshot.channels = channels;
            snapshot.last_error = None;
        }
    }

    pub(super) fn record_rx(&self, channel_index: u32, data: Vec<u8>) -> StoredRttChunk {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let channel_offset = self
            .channel_offsets
            .lock()
            .map(|mut offsets| {
                let offset = offsets.entry(channel_index).or_insert(0);
                let current = *offset;
                *offset = offset.saturating_add(data.len() as u64);
                current
            })
            .unwrap_or(0);
        let chunk = StoredRttChunk {
            generation: self.generation(),
            sequence,
            timestamp_ms: now_ms(),
            channel_index,
            channel_offset,
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
        self.publish_automation(&chunk);
        chunk
    }

    fn publish_automation(&self, chunk: &StoredRttChunk) {
        let selected = self
            .automation_channel
            .lock()
            .ok()
            .and_then(|channel| *channel);
        if selected != Some(chunk.channel_index) {
            return;
        }
        if let Ok(mut subscribers) = self.automation_subscribers.lock() {
            subscribers.retain(|subscriber| match subscriber.try_send(chunk.data.clone()) {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(_)) | Err(mpsc::TrySendError::Disconnected(_)) => {
                    false
                }
            });
        }
    }

    pub(super) fn record_tx(&self, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.tx_bytes = snapshot.tx_bytes.saturating_add(bytes as u64);
        }
    }

    pub(super) fn record_presentation_drop(&self, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.dropped_presentation_chunks =
                snapshot.dropped_presentation_chunks.saturating_add(1);
            snapshot.dropped_presentation_bytes = snapshot
                .dropped_presentation_bytes
                .saturating_add(bytes as u64);
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
        let generation = self.generation();
        self.history
            .lock()
            .map(|history| history.response(generation, channel_index, after_sequence))
            .unwrap_or_else(|_| RttHistoryResponse {
                generation,
                chunks: Vec::new(),
                oldest_sequence: None,
                newest_sequence: None,
                dropped_chunks: 0,
                dropped_bytes: 0,
            })
    }

    fn set_automation_channel(&self, channel_index: u32) -> Result<(), RttError> {
        let snapshot = self.snapshot();
        let channel = snapshot
            .channels
            .iter()
            .find(|channel| channel.index == channel_index)
            .ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttChannelNotFound,
                    format!("RTT Channel {channel_index} 不存在"),
                )
            })?;
        if channel.down.is_none() {
            return Err(RttError::new(
                RttErrorCode::RttChannelNotFound,
                format!("RTT Channel {channel_index} 没有 Down 方向"),
            ));
        }
        *self
            .automation_channel
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? = Some(channel_index);
        Ok(())
    }

    fn automation_channel(&self) -> Result<u32, RttError> {
        self.automation_channel
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttChannelNotFound,
                    "当前 RTT 会话没有可写 Down Channel",
                )
            })
    }

    fn subscribe_automation(&self) -> Result<RttAutomationRx, SessionIoError> {
        let (tx, rx) = mpsc::sync_channel(AUTOMATION_SUBSCRIPTION_CAPACITY);
        self.automation_subscribers
            .lock()
            .map_err(|error| SessionIoError::Send(error.to_string()))?
            .push(tx);
        Ok(RttAutomationRx { receiver: rx })
    }

    fn close_automation(&self) {
        if let Ok(mut subscribers) = self.automation_subscribers.lock() {
            subscribers.clear();
        }
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
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        Self {
            config,
            shared: Arc::new(RttShared::new(generation)),
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
        let context = WorkerContext {
            config: self.config.clone(),
            app,
            session_id: session_id.to_string(),
            shared: Arc::clone(&self.shared),
            shutting_down: Arc::clone(&self.shutting_down),
            worker_exited: Arc::clone(&self.worker_exited),
        };
        let handle = std::thread::Builder::new()
            .name(format!("rtt-{session_id}"))
            .spawn(move || worker::run(context, command_rx, startup_tx))
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

    pub fn set_automation_channel(&self, channel_index: u32) -> Result<(), RttError> {
        self.shared.set_automation_channel(channel_index)
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
            .recv_timeout(CHANNEL_REFRESH_REPLY_TIMEOUT)
            .map_err(|_| RttError::backend("刷新 RTT Channel 超时"))?
    }

    fn shutdown_inner(&self, permanent: bool) {
        if permanent {
            self.closed.store(true, Ordering::Release);
        }
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }
        let preserve_faulted = self.worker_exited.load(Ordering::Acquire)
            && matches!(self.shared.snapshot().phase, RttPhase::Faulted);
        if !preserve_faulted {
            self.shared.set_phase(RttPhase::Stopping);
        }
        if let Ok(mut tx_slot) = self.command_tx.lock() {
            if let Some(tx) = tx_slot.take() {
                if !self.worker_exited.load(Ordering::Acquire) {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx
                        .try_send(WorkerCommand::Shutdown { reply: reply_tx })
                        .is_ok()
                    {
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
        self.shared.close_automation();
        if !preserve_faulted {
            self.shared.set_phase(RttPhase::Idle);
        }
    }
}

struct RttAutomationRx {
    receiver: mpsc::Receiver<Vec<u8>>,
}

impl AutomationRx for RttAutomationRx {
    fn try_recv(&mut self) -> Result<AutomationRxEvent, mpsc::TryRecvError> {
        self.receiver.try_recv().map(AutomationRxEvent::Data)
    }
}

impl AutomationIo for RttRuntime {
    fn send(&self, data: &[u8]) -> Result<(), SessionIoError> {
        let channel = self
            .shared
            .automation_channel()
            .map_err(|error| SessionIoError::Send(error.to_string()))?;
        self.write(channel, data.to_vec())
            .map(|_| ())
            .map_err(|error| SessionIoError::Send(error.to_string()))
    }

    fn send_text(&self, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        self.send(data)?;
        Ok(data.to_vec())
    }

    fn send_to(&self, target: &str, data: &[u8]) -> Result<(), SessionIoError> {
        let normalized = target.trim().strip_prefix("rtt:").unwrap_or(target.trim());
        let channel = normalized
            .parse::<u32>()
            .map_err(|_| SessionIoError::Send(format!("无效 RTT Down Channel: {target}")))?;
        self.write(channel, data.to_vec())
            .map(|_| ())
            .map_err(|error| SessionIoError::Send(error.to_string()))
    }

    fn send_to_text(&self, target: &str, data: &[u8]) -> Result<Vec<u8>, SessionIoError> {
        self.send_to(target, data)?;
        Ok(data.to_vec())
    }

    fn subscribe(&self) -> Result<Box<dyn AutomationRx>, SessionIoError> {
        Ok(Box::new(self.shared.subscribe_automation()?))
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

    fn chunk(sequence: u64, bytes: usize) -> StoredRttChunk {
        StoredRttChunk {
            generation: 7,
            sequence,
            timestamp_ms: sequence,
            channel_index: 0,
            channel_offset: sequence.saturating_sub(1) * bytes as u64,
            data: vec![0x55; bytes],
        }
    }

    #[test]
    fn history_is_bounded_and_reports_loss() {
        let mut history = HistoryStore::default();
        for sequence in 1..=300u64 {
            history.push(chunk(sequence, 1024));
        }
        let response = history.response(7, 0, None);
        assert_eq!(response.generation, 7);
        assert!(response.dropped_chunks > 0);
        assert!(response.dropped_bytes > 0);
        assert!(response.oldest_sequence.unwrap_or_default() > 1);
    }

    #[test]
    fn initial_history_response_prefers_latest_chunks() {
        let mut history = HistoryStore::default();
        for sequence in 1..=600u64 {
            history.push(chunk(sequence, 1));
        }
        let response = history.response(7, 0, None);
        assert_eq!(response.chunks.len(), MAX_HISTORY_RESPONSE_CHUNKS);
        assert_eq!(
            response.chunks.first().map(|chunk| chunk.sequence),
            Some(89)
        );
        assert_eq!(
            response.chunks.last().map(|chunk| chunk.sequence),
            Some(600)
        );
    }

    #[test]
    fn runtime_generation_changes_for_each_instance() {
        let config = RttConfig::from_params(&serde_json::json!({
            "backend": "jlink_existing"
        }))
        .unwrap();
        let a = RttRuntime::new(config.clone());
        let b = RttRuntime::new(config);
        assert_ne!(a.snapshot().generation, b.snapshot().generation);
    }
}
