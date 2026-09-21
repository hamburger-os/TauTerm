use super::config::RttConfig;
use super::error::{RttError, RttErrorCode};
use super::model::{
    RttChannelClaim, RttChunkDto, RttHistoryResponse, RttObserverInfo, RttPhase, RttSnapshot,
    StoredRttChunk,
};
use super::systemview::{
    SystemViewControl, SystemViewHistoryResponse, SystemViewRuntime, SystemViewSnapshot,
};
use super::worker::{self, WorkerCommand, WorkerContext};
use crate::embedded_debug::observation::{
    ObservationSequencer, ObservationSource, ObservationSubscription,
};
use crate::embedded_debug::runtime::EmbeddedDebugManager;
use crate::kernel::plugin_adapter::SessionService;
use crate::session::{AutomationIo, AutomationRx, AutomationRxEvent, SessionIoError};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::AppHandle;

const HISTORY_PER_CHANNEL_BYTES: usize = 256 * 1024;
const HISTORY_PER_SESSION_BYTES: usize = 2 * 1024 * 1024;
const MAX_HISTORY_RESPONSE_CHUNKS: usize = 512;
const COMMAND_QUEUE_CAPACITY: usize = 64;
const RAW_OBSERVATION_SUBSCRIPTION_CAPACITY: usize = 1024;
const CHANNEL_REFRESH_REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_REPLY_GRACE: Duration = Duration::from_secs(3);

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
    sequencer: ObservationSequencer,
    raw_source: ObservationSource<StoredRttChunk>,
    channel_offsets: Mutex<BTreeMap<u32, u64>>,
    automation_source_channel: Mutex<Option<u32>>,
    send_channel: Mutex<Option<u32>>,
    channel_claims: Mutex<BTreeMap<u32, String>>,
}

impl RttShared {
    pub(super) fn new() -> Self {
        let sequencer = ObservationSequencer::new();
        let generation = sequencer.generation();
        Self {
            snapshot: Mutex::new(RttSnapshot {
                generation,
                ..RttSnapshot::default()
            }),
            history: Mutex::new(HistoryStore::default()),
            sequencer,
            raw_source: ObservationSource::new(RAW_OBSERVATION_SUBSCRIPTION_CAPACITY),
            channel_offsets: Mutex::new(BTreeMap::new()),
            automation_source_channel: Mutex::new(None),
            send_channel: Mutex::new(None),
            channel_claims: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn generation(&self) -> u64 {
        self.snapshot().generation
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
        let automation_source_channel =
            self.automation_source_channel
                .lock()
                .ok()
                .and_then(|mut selected| {
                    let still_readable = selected.is_some_and(|index| {
                        channels
                            .iter()
                            .any(|channel| channel.index == index && channel.has_usable_up())
                    });
                    if !still_readable {
                        *selected = channels
                            .iter()
                            .find(|channel| channel.index == 0 && channel.has_usable_up())
                            .or_else(|| channels.iter().find(|channel| channel.has_usable_up()))
                            .map(|channel| channel.index);
                    }
                    *selected
                });
        let claimed = self
            .channel_claims
            .lock()
            .map(|claims| claims.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let is_writable = |channel: &super::model::RttChannelInfo| {
            channel.has_usable_down() && !claimed.contains(&channel.index)
        };
        let send_channel = self.send_channel.lock().ok().and_then(|mut selected| {
            let still_writable = selected.is_some_and(|index| {
                channels
                    .iter()
                    .any(|channel| channel.index == index && is_writable(channel))
            });
            if !still_writable {
                *selected = channels
                    .iter()
                    .find(|channel| channel.index == 0 && is_writable(channel))
                    .or_else(|| channels.iter().find(|channel| is_writable(channel)))
                    .map(|channel| channel.index);
            }
            *selected
        });
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Running;
            snapshot.backend = Some(descriptor);
            snapshot.channels = channels;
            snapshot.automation_source_channel = automation_source_channel;
            snapshot.send_channel = send_channel;
            snapshot.channel_claims = self.channel_claim_snapshot();
            snapshot.last_error = None;
        }
    }

    pub(super) fn record_rx(&self, channel_index: u32, data: Vec<u8>) -> StoredRttChunk {
        let stamp = self.sequencer.stamp();
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
            generation: stamp.generation,
            sequence: stamp.sequence,
            timestamp_ms: stamp.timestamp_ms,
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
        let _ = self.raw_source.publish(&chunk);
        chunk
    }

    pub(super) fn record_tx(&self, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.tx_bytes = snapshot.tx_bytes.saturating_add(bytes as u64);
        }
    }

    fn record_automation_drop(&self, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.dropped_automation_chunks =
                snapshot.dropped_automation_chunks.saturating_add(1);
            snapshot.dropped_automation_bytes = snapshot
                .dropped_automation_bytes
                .saturating_add(bytes as u64);
        }
    }

    pub(super) fn record_presentation_drop(&self, bytes: usize) {
        self.record_presentation_drop_batch(1, bytes);
    }

    pub(super) fn record_presentation_drop_batch(&self, chunks: usize, bytes: usize) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.dropped_presentation_chunks = snapshot
                .dropped_presentation_chunks
                .saturating_add(chunks as u64);
            snapshot.dropped_presentation_bytes = snapshot
                .dropped_presentation_bytes
                .saturating_add(bytes as u64);
        }
    }

    pub(super) fn record_runtime_pressure(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.runtime_pressure_events = snapshot.runtime_pressure_events.saturating_add(1);
        }
    }

    pub(super) fn set_error(&self, error: RttError) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Faulted;
            snapshot.last_error = Some(error);
        }
    }

    pub(super) fn set_stopped(&self) {
        if let Ok(mut selected) = self.automation_source_channel.lock() {
            *selected = None;
        }
        if let Ok(mut selected) = self.send_channel.lock() {
            *selected = None;
        }
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.phase = RttPhase::Idle;
            snapshot.automation_source_channel = None;
            snapshot.send_channel = None;
            snapshot.observers.clear();
            snapshot.channel_claims.clear();
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

    fn validate_channel_direction(
        &self,
        channel_index: u32,
        require_up: bool,
    ) -> Result<(), RttError> {
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
        let direction = if require_up {
            channel.up.as_ref()
        } else {
            channel.down.as_ref()
        };
        let direction_name = if require_up { "Up" } else { "Down" };
        let Some(direction) = direction else {
            return Err(RttError::new(
                RttErrorCode::RttChannelNotFound,
                format!("RTT Channel {channel_index} 没有 {direction_name} 方向"),
            ));
        };
        if !direction.usable {
            return Err(RttError::new(
                RttErrorCode::RttInvalidControlBlock,
                format!(
                    "RTT Channel {channel_index} {direction_name} 方向不可用: {}",
                    direction.issue.as_deref().unwrap_or("descriptor 无效")
                ),
            ));
        }
        Ok(())
    }

    fn set_automation_source_channel(&self, channel_index: u32) -> Result<(), RttError> {
        self.validate_channel_direction(channel_index, true)?;
        *self
            .automation_source_channel
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? = Some(channel_index);
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.automation_source_channel = Some(channel_index);
        }
        Ok(())
    }

    fn set_send_channel(&self, channel_index: u32) -> Result<(), RttError> {
        self.validate_channel_direction(channel_index, false)?;
        self.validate_user_write(channel_index)?;
        *self
            .send_channel
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? = Some(channel_index);
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.send_channel = Some(channel_index);
        }
        Ok(())
    }

    fn channel_claim_snapshot(&self) -> Vec<RttChannelClaim> {
        self.channel_claims
            .lock()
            .map(|claims| {
                claims
                    .iter()
                    .map(|(channel_index, owner)| RttChannelClaim {
                        channel_index: *channel_index,
                        owner: owner.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn validate_user_write(&self, channel_index: u32) -> Result<(), RttError> {
        if let Some(owner) = self
            .channel_claims
            .lock()
            .ok()
            .and_then(|claims| claims.get(&channel_index).cloned())
        {
            return Err(RttError::new(
                RttErrorCode::RttWriteFailed,
                format!(
                    "RTT Down Channel {channel_index} 正由 {owner} 使用，不能作为通用发送目标"
                ),
            ));
        }
        Ok(())
    }

    fn claim_down_channel(&self, channel_index: u32, owner: &str) -> Result<(), RttError> {
        self.validate_channel_direction(channel_index, false)?;
        {
            let mut claims = self
                .channel_claims
                .lock()
                .map_err(|error| RttError::backend(error.to_string()))?;
            if let Some(existing) = claims.get(&channel_index) {
                if existing != owner {
                    return Err(RttError::new(
                        RttErrorCode::RttWriteFailed,
                        format!(
                            "RTT Down Channel {channel_index} 已由 {existing} 占用，无法交给 {owner}"
                        ),
                    ));
                }
                return Ok(());
            }
            claims.insert(channel_index, owner.to_string());
        }

        let channels = self.snapshot().channels;
        let claims = self
            .channel_claims
            .lock()
            .map(|claims| claims.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let replacement = channels
            .iter()
            .find(|channel| {
                channel.index == 0
                    && channel.has_usable_down()
                    && !claims.contains(&channel.index)
            })
            .or_else(|| {
                channels.iter().find(|channel| {
                    channel.has_usable_down() && !claims.contains(&channel.index)
                })
            })
            .map(|channel| channel.index);
        if let Ok(mut selected) = self.send_channel.lock() {
            if selected.is_some_and(|current| current == channel_index) {
                *selected = replacement;
            }
        }
        if let Ok(mut snapshot) = self.snapshot.lock() {
            if snapshot.send_channel == Some(channel_index) {
                snapshot.send_channel = replacement;
            }
            snapshot.channel_claims = self.channel_claim_snapshot();
        }
        Ok(())
    }

    fn release_down_channel(&self, channel_index: u32, owner: &str) {
        let removed = self
            .channel_claims
            .lock()
            .map(|mut claims| {
                if claims.get(&channel_index).is_some_and(|value| value == owner) {
                    claims.remove(&channel_index);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if !removed {
            return;
        }
        let channels = self.snapshot().channels;
        let claims = self
            .channel_claims
            .lock()
            .map(|claims| claims.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut selected_guard = match self.send_channel.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if selected_guard.is_none() {
            *selected_guard = channels
                .iter()
                .find(|channel| {
                    channel.index == 0
                        && channel.has_usable_down()
                        && !claims.contains(&channel.index)
                })
                .or_else(|| {
                    channels.iter().find(|channel| {
                        channel.has_usable_down() && !claims.contains(&channel.index)
                    })
                })
                .map(|channel| channel.index);
        }
        let selected = *selected_guard;
        drop(selected_guard);
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.send_channel = selected;
            snapshot.channel_claims = self.channel_claim_snapshot();
        }
    }

    fn set_observers(&self, observers: Vec<RttObserverInfo>) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.observers = observers;
        }
    }

    fn subscribe_observer(
        self: &Arc<Self>,
        channel_index: u32,
    ) -> Result<(ObservationSubscription<StoredRttChunk>, Arc<AtomicU64>), RttError> {
        self.validate_channel_direction(channel_index, true)?;
        let drops = Arc::new(AtomicU64::new(0));
        let drop_counter = Arc::clone(&drops);
        let subscription = self.raw_source.subscribe_filtered(
            move |chunk| chunk.channel_index == channel_index,
            move |_| {
                drop_counter.fetch_add(1, Ordering::Relaxed);
            },
        );
        Ok((subscription, drops))
    }

    fn send_channel(&self) -> Result<u32, RttError> {
        self.send_channel
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttChannelNotFound,
                    "当前 RTT 会话没有可写 Down Channel",
                )
            })
    }

    fn subscribe_automation(
        self: &Arc<Self>,
        _consumer: &str,
    ) -> Result<RttAutomationRx, SessionIoError> {
        // Capture the source at subscription time. A running Auto Reply/Lua execution therefore
        // keeps an immutable Up Channel even if the user later browses another RTT channel.
        let source_channel = self
            .automation_source_channel
            .lock()
            .map_err(|error| SessionIoError::Send(error.to_string()))?
            .ok_or(SessionIoError::AutomationReceiveUnsupported)?;
        let weak = Arc::downgrade(self);
        let subscription = self.raw_source.subscribe_filtered(
            move |chunk| chunk.channel_index == source_channel,
            move |chunk| {
                if let Some(shared) = weak.upgrade() {
                    shared.record_automation_drop(chunk.data.len());
                }
            },
        );
        Ok(RttAutomationRx { subscription })
    }
}

pub struct RttRuntime {
    config: RttConfig,
    embedded_debug: Arc<EmbeddedDebugManager>,
    shared: Arc<RttShared>,
    command_tx: Mutex<Option<mpsc::SyncSender<WorkerCommand>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    shutting_down: Arc<AtomicBool>,
    worker_exited: Arc<AtomicBool>,
    systemview: Mutex<BTreeMap<u32, Arc<SystemViewRuntime>>>,
    observer_context: Mutex<Option<(AppHandle, String)>>,
    closed: AtomicBool,
}

impl RttRuntime {
    pub(crate) fn new(config: RttConfig, embedded_debug: Arc<EmbeddedDebugManager>) -> Self {
        Self {
            config,
            embedded_debug,
            shared: Arc::new(RttShared::new()),
            command_tx: Mutex::new(None),
            worker: Mutex::new(None),
            shutting_down: Arc::new(AtomicBool::new(false)),
            worker_exited: Arc::new(AtomicBool::new(false)),
            systemview: Mutex::new(BTreeMap::new()),
            observer_context: Mutex::new(None),
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
        *self
            .observer_context
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))? =
            Some((app.clone(), session_id.to_string()));
        let context = WorkerContext {
            config: self.config.clone(),
            embedded_debug: Arc::clone(&self.embedded_debug),
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
        // Native startup may first spend up to the shared target-open deadline before RTT attach
        // begins. Keep the outer lifecycle budget strictly larger than all nested startup stages
        // so a valid slow probe open is not misreported as an RTT attach timeout.
        let startup_timeout = self
            .config
            .attach_timeout
            .saturating_add(Duration::from_secs(15));
        match startup_rx.recv_timeout(startup_timeout) {
            Ok(Ok(())) => {
                if let Err(error) = self.sync_systemview_observers() {
                    log::warn!(
                        "RTT SystemView observer discovery failed: session={}, code={}, message={}",
                        session_id,
                        error.code.as_str(),
                        error.message
                    );
                }
                Ok(())
            }
            Ok(Err(error)) => Err(error),
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

    pub fn set_automation_source_channel(&self, channel_index: u32) -> Result<(), RttError> {
        self.shared.set_automation_source_channel(channel_index)
    }

    pub fn set_send_channel(&self, channel_index: u32) -> Result<(), RttError> {
        self.shared.set_send_channel(channel_index)
    }

    pub fn write(&self, channel_index: u32, data: Vec<u8>) -> Result<usize, RttError> {
        self.shared.validate_channel_direction(channel_index, false)?;
        self.shared.validate_user_write(channel_index)?;
        self.write_internal(channel_index, data)
    }

    fn write_internal(&self, channel_index: u32, data: Vec<u8>) -> Result<usize, RttError> {
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
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => RttError::new(
                RttErrorCode::RttWriteQueueFull,
                "RTT 写入队列繁忙，请降低发送速率",
            ),
            mpsc::TrySendError::Disconnected(_) => {
                RttError::new(RttErrorCode::Cancelled, "RTT worker 已停止")
            }
        })?;
        reply_rx
            .recv_timeout(self.config.write_timeout.saturating_add(WRITE_REPLY_GRACE))
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
        let channels = reply_rx
            .recv_timeout(CHANNEL_REFRESH_REPLY_TIMEOUT)
            .map_err(|_| RttError::backend("刷新 RTT Channel 超时"))??;
        if let Err(error) = self.sync_systemview_observers() {
            log::warn!(
                "RTT SystemView observer refresh failed: code={}, message={}",
                error.code.as_str(),
                error.message
            );
        }
        Ok(channels)
    }

    pub fn systemview_attach(&self, channel_index: u32) -> Result<(), RttError> {
        self.start_systemview_observer(channel_index)
    }

    pub fn systemview_snapshot(&self, channel_index: u32) -> Result<SystemViewSnapshot, RttError> {
        self.systemview_runtime(channel_index).map(|runtime| runtime.snapshot())
    }

    pub fn systemview_history(
        &self,
        channel_index: u32,
        limit: usize,
    ) -> Result<SystemViewHistoryResponse, RttError> {
        self.systemview_runtime(channel_index)
            .map(|runtime| runtime.history(limit))
    }

    pub fn systemview_control(
        &self,
        channel_index: u32,
        control: SystemViewControl,
    ) -> Result<(), RttError> {
        let runtime = self.systemview_runtime(channel_index)?;
        if !runtime.snapshot().control_available {
            return Err(RttError::new(
                RttErrorCode::RttChannelNotFound,
                format!(
                    "SystemView Channel {channel_index} 没有可用的同索引 Down Channel，无法发送控制命令"
                ),
            ));
        }
        self.write_internal(channel_index, control.bytes().to_vec())?;
        Ok(())
    }

    pub fn systemview_clear(&self, channel_index: u32) -> Result<(), RttError> {
        self.systemview_runtime(channel_index)?.clear();
        Ok(())
    }

    fn systemview_runtime(&self, channel_index: u32) -> Result<Arc<SystemViewRuntime>, RttError> {
        self.systemview
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .get(&channel_index)
            .cloned()
            .ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttChannelNotFound,
                    format!("RTT Channel {channel_index} 未启用 SystemView 观察器"),
                )
            })
    }

    fn sync_systemview_observers(&self) -> Result<(), RttError> {
        let snapshot = self.shared.snapshot();
        let existing = self
            .systemview
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for channel_index in existing {
            let still_available = snapshot
                .channels
                .iter()
                .any(|channel| channel.index == channel_index && channel.has_usable_up());
            if !still_available {
                self.stop_systemview_observer(channel_index);
            }
        }
        for channel in &snapshot.channels {
            let is_systemview = channel
                .name
                .as_deref()
                .map(|name| {
                    let normalized = name.trim().to_ascii_lowercase();
                    normalized.contains("sysview") || normalized.contains("systemview")
                })
                .unwrap_or(false);
            if is_systemview && channel.has_usable_up() {
                if let Err(error) = self.start_systemview_observer(channel.index) {
                    log::warn!(
                        "SystemView auto attach skipped: channel={}, code={}, message={}",
                        channel.index,
                        error.code.as_str(),
                        error.message
                    );
                }
            }
        }
        self.publish_observers();
        Ok(())
    }

    fn start_systemview_observer(&self, channel_index: u32) -> Result<(), RttError> {
        if self
            .systemview
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .contains_key(&channel_index)
        {
            return Ok(());
        }
        self.shared.validate_channel_direction(channel_index, true)?;
        let snapshot = self.shared.snapshot();
        let control_available = snapshot.channels.iter().any(|channel| {
            channel.index == channel_index && channel.has_usable_down()
        });
        let owner = "SystemView";
        if control_available {
            self.shared.claim_down_channel(channel_index, owner)?;
        }
        let (subscription, decoder_drops) = match self.shared.subscribe_observer(channel_index) {
            Ok(value) => value,
            Err(error) => {
                if control_available {
                    self.shared.release_down_channel(channel_index, owner);
                }
                return Err(error);
            }
        };
        let (app, session_id) = self
            .observer_context
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .clone()
            .ok_or_else(|| RttError::new(RttErrorCode::Cancelled, "RTT 观察器上下文不可用"))?;
        let runtime = match SystemViewRuntime::spawn(
            app,
            session_id,
            snapshot.generation,
            channel_index,
            control_available,
            subscription,
            decoder_drops,
        ) {
            Ok(runtime) => Arc::new(runtime),
            Err(error) => {
                if control_available {
                    self.shared.release_down_channel(channel_index, owner);
                }
                return Err(RttError::backend(error));
            }
        };
        self.systemview
            .lock()
            .map_err(|error| RttError::backend(error.to_string()))?
            .insert(channel_index, runtime);
        self.publish_observers();
        // Attaching a semantic observer is passive. Starting/stopping trace changes target
        // behavior and therefore remains an explicit user action through systemview_control.
        Ok(())
    }

    fn stop_systemview_observer(&self, channel_index: u32) {
        let runtime = self
            .systemview
            .lock()
            .ok()
            .and_then(|mut runtimes| runtimes.remove(&channel_index));
        if let Some(runtime) = runtime {
            runtime.shutdown();
        }
        self.shared.release_down_channel(channel_index, "SystemView");
        self.publish_observers();
    }

    fn publish_observers(&self) {
        let observers = self
            .systemview
            .lock()
            .map(|runtimes| {
                runtimes
                    .values()
                    .map(|runtime| {
                        let snapshot = runtime.snapshot();
                        RttObserverInfo {
                            kind: "systemview".to_string(),
                            channel_index: runtime.channel_index(),
                            control_channel_index: snapshot
                                .control_available
                                .then_some(runtime.channel_index()),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.shared.set_observers(observers);
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
        let observer_channels = self
            .systemview
            .lock()
            .map(|runtimes| runtimes.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for channel_index in observer_channels {
            self.stop_systemview_observer(channel_index);
        }
        if let Ok(mut tx_slot) = self.command_tx.lock() {
            if let Some(tx) = tx_slot.take() {
                if !self.worker_exited.load(Ordering::Acquire) {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    // SessionService shutdown runs outside the SessionStore lock. A blocking
                    // enqueue is intentional here: it guarantees the single-owner worker observes
                    // the lifecycle command even when ordinary RTT writes temporarily fill the
                    // bounded command queue.
                    if tx.send(WorkerCommand::Shutdown { reply: reply_tx }).is_ok() {
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
        self.shared.raw_source.close();
        if let Ok(mut context) = self.observer_context.lock() {
            *context = None;
        }
        if !preserve_faulted {
            self.shared.set_stopped();
        }
    }
}

struct RttAutomationRx {
    subscription: ObservationSubscription<StoredRttChunk>,
}

impl AutomationRx for RttAutomationRx {
    fn try_recv(&mut self) -> Result<AutomationRxEvent, mpsc::TryRecvError> {
        self.subscription
            .try_recv()
            .map(|chunk| AutomationRxEvent::Data(chunk.data))
    }
}

impl AutomationIo for RttRuntime {
    fn send(&self, data: &[u8]) -> Result<(), SessionIoError> {
        let channel = self
            .shared
            .send_channel()
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

    fn subscribe(&self, consumer: &str) -> Result<Box<dyn AutomationRx>, SessionIoError> {
        Ok(Box::new(self.shared.subscribe_automation(consumer)?))
    }
}

impl SessionService for RttRuntime {
    fn shutdown(&self) {
        self.shutdown_inner(true);
    }
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

    fn backend_descriptor() -> super::super::model::RttBackendDescriptor {
        super::super::model::RttBackendDescriptor {
            kind: "test".to_string(),
            display_name: "Test".to_string(),
            target: None,
            probe: None,
            control_block_address: None,
            capabilities: super::super::model::RttBackendCapabilities::default(),
        }
    }

    #[test]
    fn automation_source_and_send_target_are_independent_directions() {
        use super::super::model::{RttChannelDirectionInfo, RttChannelInfo};

        let shared = RttShared::new();
        shared.set_running(
            backend_descriptor(),
            vec![
                RttChannelInfo {
                    index: 1,
                    name: Some("up-only".to_string()),
                    up: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    down: None,
                    metadata_complete: true,
                },
                RttChannelInfo {
                    index: 2,
                    name: Some("down-only".to_string()),
                    up: None,
                    down: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    metadata_complete: true,
                },
            ],
        );

        assert!(shared.set_automation_source_channel(1).is_ok());
        assert!(shared.set_send_channel(2).is_ok());
        assert!(shared.set_automation_source_channel(2).is_err());
        assert!(shared.set_send_channel(1).is_err());
        assert_eq!(shared.send_channel().unwrap(), 2);
    }

    #[test]
    fn degraded_channels_are_never_selected_for_runtime_io() {
        use super::super::model::{RttChannelDirectionInfo, RttChannelInfo};

        let shared = RttShared::new();
        shared.set_running(
            backend_descriptor(),
            vec![
                RttChannelInfo {
                    index: 0,
                    name: Some("broken".to_string()),
                    up: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: false,
                        issue: Some("bad up descriptor".to_string()),
                    }),
                    down: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: false,
                        issue: Some("bad down descriptor".to_string()),
                    }),
                    metadata_complete: false,
                },
                RttChannelInfo {
                    index: 1,
                    name: Some("healthy".to_string()),
                    up: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    down: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    metadata_complete: true,
                },
            ],
        );

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.automation_source_channel, Some(1));
        assert_eq!(snapshot.send_channel, Some(1));
        assert_eq!(
            shared.set_automation_source_channel(0).unwrap_err().code,
            RttErrorCode::RttInvalidControlBlock
        );
        assert_eq!(
            shared.set_send_channel(0).unwrap_err().code,
            RttErrorCode::RttInvalidControlBlock
        );
    }

    #[test]
    fn stopped_runtime_clears_active_channel_authorities_but_keeps_metadata() {
        use super::super::model::{RttChannelDirectionInfo, RttChannelInfo};

        let shared = RttShared::new();
        shared.set_running(
            backend_descriptor(),
            vec![RttChannelInfo {
                index: 0,
                name: Some("Terminal".to_string()),
                up: Some(RttChannelDirectionInfo {
                    buffer_size: Some(64),
                    usable: true,
                    issue: None,
                }),
                down: Some(RttChannelDirectionInfo {
                    buffer_size: Some(64),
                    usable: true,
                    issue: None,
                }),
                metadata_complete: true,
            }],
        );
        shared.set_stopped();

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.phase, RttPhase::Idle);
        assert_eq!(snapshot.automation_source_channel, None);
        assert_eq!(snapshot.send_channel, None);
        assert_eq!(snapshot.channels.len(), 1);
    }

    #[test]
    fn automation_subscription_keeps_its_startup_source_channel() {
        use super::super::model::{RttChannelDirectionInfo, RttChannelInfo};

        let shared = Arc::new(RttShared::new());
        shared.set_running(
            backend_descriptor(),
            vec![
                RttChannelInfo {
                    index: 1,
                    name: None,
                    up: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    down: None,
                    metadata_complete: true,
                },
                RttChannelInfo {
                    index: 2,
                    name: None,
                    up: Some(RttChannelDirectionInfo {
                        buffer_size: Some(64),
                        usable: true,
                        issue: None,
                    }),
                    down: None,
                    metadata_complete: true,
                },
            ],
        );
        shared.set_automation_source_channel(1).unwrap();
        let mut subscription = shared.subscribe_automation("test-script").unwrap();
        shared.set_automation_source_channel(2).unwrap();

        shared.record_rx(1, b"old-source".to_vec());
        shared.record_rx(2, b"new-view".to_vec());

        assert!(matches!(
            subscription.try_recv(),
            Ok(AutomationRxEvent::Data(data)) if data == b"old-source"
        ));
        assert!(matches!(
            subscription.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn canonical_rtt_observations_are_published_once_from_acquisition() {
        let shared = RttShared::new();
        let observations = shared.raw_source.subscribe_filtered(|_| true, |_| {});
        let chunk = shared.record_rx(3, b"trace".to_vec());

        let observed = observations.try_recv().unwrap();
        assert_eq!(observed.generation, chunk.generation);
        assert_eq!(observed.sequence, chunk.sequence);
        assert_eq!(observed.channel_index, 3);
        assert_eq!(observed.channel_offset, 0);
        assert_eq!(observed.data, b"trace");
    }

    #[test]
    fn runtime_generation_changes_for_each_instance() {
        let config = RttConfig::from_params(&serde_json::json!({
            "backend": "jlink_existing"
        }))
        .unwrap();
        let manager = Arc::new(EmbeddedDebugManager::new());
        let a = RttRuntime::new(config.clone(), Arc::clone(&manager));
        let b = RttRuntime::new(config, manager);
        assert_ne!(a.snapshot().generation, b.snapshot().generation);
    }
}
