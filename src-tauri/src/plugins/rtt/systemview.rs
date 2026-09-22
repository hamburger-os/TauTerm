use super::model::StoredRttChunk;
use crate::embedded_debug::observation::ObservationSubscription;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_EVENT_HISTORY: usize = 4096;
const MAX_PENDING_PRESENTATION_EVENTS: usize = 1024;
const PRESENTATION_FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
const MAX_DECODER_BUFFER: usize = 64 * 1024;
const METADATA_RETRY_DELAYS_MS: [u64; 10] = [
    350, 1_000, 2_500, 5_000, 10_000, 20_000, 30_000, 45_000, 60_000, 90_000,
];

const COMMAND_START: u8 = 1;
const COMMAND_STOP: u8 = 2;
const COMMAND_GET_SYSTIME: u8 = 3;
const COMMAND_GET_TASKLIST: u8 = 4;
const COMMAND_GET_SYSDESC: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemViewPhase {
    Idle,
    Recording,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemViewTaskSnapshot {
    pub id: u32,
    pub name: Option<String>,
    pub priority: Option<u32>,
    pub runtime_cycles: u64,
    pub switches: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemViewEvent {
    pub sequence: u64,
    pub event_id: u32,
    pub kind: String,
    pub target_cycles: u64,
    pub delta_cycles: u32,
    pub context_id: Option<u32>,
    pub value: Option<u64>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemViewSnapshot {
    pub generation: u64,
    pub channel_index: u32,
    pub phase: SystemViewPhase,
    pub control_available: bool,
    pub event_count: u64,
    pub task_count: usize,
    pub metadata_sync_attempts: u32,
    pub metadata_sync_max_attempts: u32,
    pub target_overflow_packets: u64,
    pub target_dropped_events: u64,
    pub decoder_dropped_chunks: u64,
    pub decoder_errors: u64,
    pub presentation_dropped_events: u64,
    pub cleared_through_sequence: u64,
    pub sys_freq_hz: Option<u32>,
    pub cpu_freq_hz: Option<u32>,
    pub ram_base: Option<u32>,
    pub id_shift: Option<u32>,
    pub system_description: Vec<String>,
    pub window_start_cycles: u64,
    pub last_target_cycles: u64,
    pub tasks: Vec<SystemViewTaskSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemViewHistoryResponse {
    pub generation: u64,
    pub channel_index: u32,
    pub events: Vec<SystemViewEvent>,
}

#[derive(Debug, Clone)]
pub enum SystemViewPresentation {
    Batch {
        events: Vec<SystemViewEvent>,
        snapshot: SystemViewSnapshot,
    },
    Snapshot {
        snapshot: SystemViewSnapshot,
    },
}

pub type SystemViewPresenter = Arc<dyn Fn(SystemViewPresentation) + Send + Sync + 'static>;
pub type SystemViewMetadataRefresh = Arc<dyn Fn() -> bool + Send + Sync + 'static>;

#[cfg(test)]
pub fn discard_presenter() -> SystemViewPresenter {
    Arc::new(|presentation| match presentation {
        SystemViewPresentation::Batch { events, snapshot } => {
            let _ = (events.len(), snapshot.channel_index);
        }
        SystemViewPresentation::Snapshot { snapshot } => {
            let _ = snapshot.channel_index;
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemViewControl {
    Start,
    Stop,
    RefreshMetadata,
    RefreshTasks,
}

impl SystemViewControl {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "refresh" => Ok(Self::RefreshMetadata),
            "refresh_tasks" => Ok(Self::RefreshTasks),
            other => Err(format!("未知 SystemView 控制命令: {other}")),
        }
    }

    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Start => &[COMMAND_START],
            Self::Stop => &[COMMAND_STOP],
            Self::RefreshMetadata => &[
                COMMAND_GET_SYSDESC,
                COMMAND_GET_TASKLIST,
                COMMAND_GET_SYSTIME,
            ],
            Self::RefreshTasks => &[COMMAND_GET_TASKLIST],
        }
    }
}

#[derive(Debug, Default)]
struct TaskState {
    name: Option<String>,
    priority: Option<u32>,
    runtime_cycles: u64,
    switches: u64,
}

struct TraceState {
    generation: u64,
    channel_index: u32,
    control_available: bool,
    phase: SystemViewPhase,
    event_count: u64,
    metadata_sync_attempts: u32,
    target_overflow_packets: u64,
    target_dropped_events: u64,
    decoder_errors: u64,
    presentation_dropped_events: u64,
    cleared_through_sequence: u64,
    sys_freq_hz: Option<u32>,
    cpu_freq_hz: Option<u32>,
    ram_base: Option<u32>,
    id_shift: Option<u32>,
    system_description: Vec<String>,
    window_start_cycles: u64,
    last_target_cycles: u64,
    active_task: Option<(u32, u64)>,
    interrupted_task: Option<u32>,
    isr_depth: u32,
    tasks: BTreeMap<u32, TaskState>,
    events: VecDeque<SystemViewEvent>,
    next_event_sequence: u64,
}

impl TraceState {
    fn new(generation: u64, channel_index: u32, control_available: bool) -> Self {
        Self {
            generation,
            channel_index,
            control_available,
            phase: SystemViewPhase::Idle,
            event_count: 0,
            metadata_sync_attempts: 0,
            target_overflow_packets: 0,
            target_dropped_events: 0,
            decoder_errors: 0,
            presentation_dropped_events: 0,
            cleared_through_sequence: 0,
            sys_freq_hz: None,
            cpu_freq_hz: None,
            ram_base: None,
            id_shift: None,
            system_description: Vec::new(),
            window_start_cycles: 0,
            last_target_cycles: 0,
            active_task: None,
            interrupted_task: None,
            isr_depth: 0,
            tasks: BTreeMap::new(),
            events: VecDeque::new(),
            next_event_sequence: 1,
        }
    }

    fn close_active_task(&mut self, at_cycles: u64) -> Option<u32> {
        let (task_id, started_at) = self.active_task.take()?;
        let elapsed = at_cycles.saturating_sub(started_at);
        let task = self.tasks.entry(task_id).or_default();
        task.runtime_cycles = task.runtime_cycles.saturating_add(elapsed);
        Some(task_id)
    }

    fn apply(&mut self, packet: ParsedPacket) -> SystemViewEvent {
        let previous_target_cycles = self.last_target_cycles;
        if packet.sync_boundary {
            // SEGGER resets LastTxTimeStamp before emitting the sync/start sequence. The first
            // event after the 10-byte sync marker therefore carries a delta of zero, not an
            // absolute target timestamp. Treat the marker as a trace-epoch boundary: close any
            // open execution interval, keep the presentation timeline monotonic, and resume by
            // accumulating the new epoch's deltas. Absolute SYSTIME values remain available as
            // event payloads (IDs 12/13) instead of being conflated with packet deltas.
            self.close_active_task(previous_target_cycles);
            self.interrupted_task = None;
            self.isr_depth = 0;
        }
        if packet.event_id == 1 {
            // Overflow means events before this packet are missing. The packet delta spans an
            // interval whose scheduler state is unknowable, so never charge that interval to the
            // previously active task.
            self.close_active_task(previous_target_cycles);
            self.interrupted_task = None;
            self.isr_depth = 0;
        }
        self.last_target_cycles = previous_target_cycles.saturating_add(packet.delta_cycles as u64);
        self.event_count = self.event_count.saturating_add(1);

        let mut context_id = None;
        let mut value = None;
        let mut text = packet.text.clone();

        match packet.event_id {
            1 => {
                self.target_overflow_packets = self.target_overflow_packets.saturating_add(1);
                let dropped = packet.fields.first().copied().unwrap_or_default() as u64;
                self.target_dropped_events = self.target_dropped_events.saturating_add(dropped);
                value = Some(dropped);
            }
            2 => {
                context_id = packet.fields.first().copied();
                if self.isr_depth == 0 {
                    self.interrupted_task = self.close_active_task(self.last_target_cycles);
                }
                self.isr_depth = self.isr_depth.saturating_add(1);
            }
            3 => {
                self.isr_depth = self.isr_depth.saturating_sub(1);
                if self.isr_depth == 0 {
                    if let Some(task_id) = self.interrupted_task.take() {
                        self.active_task = Some((task_id, self.last_target_cycles));
                    }
                }
            }
            4 => {
                if let Some(task_id) = packet.fields.first().copied() {
                    self.close_active_task(self.last_target_cycles);
                    self.interrupted_task = None;
                    self.isr_depth = 0;
                    let task = self.tasks.entry(task_id).or_default();
                    task.switches = task.switches.saturating_add(1);
                    self.active_task = Some((task_id, self.last_target_cycles));
                    context_id = Some(task_id);
                }
            }
            5 | 17 => {
                self.close_active_task(self.last_target_cycles);
                self.interrupted_task = None;
            }
            6 | 8 | 15 | 16 | 19 | 29 => {
                context_id = packet.fields.first().copied();
            }
            7 => {
                context_id = packet.fields.first().copied();
                value = packet.fields.get(1).copied().map(u64::from);
            }
            9 => {
                if let Some(task_id) = packet.fields.first().copied() {
                    let task = self.tasks.entry(task_id).or_default();
                    task.priority = packet.fields.get(1).copied();
                    if let Some(name) = packet.text.as_ref() {
                        task.name = Some(name.clone());
                    }
                    context_id = Some(task_id);
                }
            }
            10 => {
                self.phase = SystemViewPhase::Recording;
            }
            11 => {
                self.close_active_task(self.last_target_cycles);
                self.interrupted_task = None;
                self.isr_depth = 0;
                self.phase = SystemViewPhase::Stopped;
            }
            12 => {
                value = packet.fields.first().copied().map(u64::from);
            }
            13 => {
                let low = packet.fields.first().copied().unwrap_or_default() as u64;
                let high = packet.fields.get(1).copied().unwrap_or_default() as u64;
                value = Some(low | (high << 32));
            }
            14 => {
                if let Some(description) = packet.text.as_ref() {
                    if !description.is_empty()
                        && !self
                            .system_description
                            .iter()
                            .any(|item| item == description)
                    {
                        self.system_description.push(description.clone());
                    }
                }
            }
            18 => {
                self.isr_depth = 0;
                self.interrupted_task = None;
            }
            21 => {
                context_id = packet.fields.first().copied();
                value = packet.fields.get(3).copied().map(u64::from);
            }
            23 => {
                context_id = packet.fields.first().copied();
                value = packet.fields.get(1).copied().map(u64::from);
            }
            24 => {
                self.sys_freq_hz = packet.fields.first().copied();
                self.cpu_freq_hz = packet.fields.get(1).copied();
                self.ram_base = packet.fields.get(2).copied();
                self.id_shift = packet.fields.get(3).copied();
            }
            25 => {
                context_id = packet.fields.first().copied();
            }
            27 => {
                value = packet.fields.first().copied().map(u64::from);
            }
            31 => {
                value = packet.fields.first().copied().map(u64::from);
            }
            _ => {
                if let Some(first) = packet.fields.first().copied() {
                    value = Some(first as u64);
                }
                if text.as_deref() == Some("") {
                    text = None;
                }
            }
        }

        let event = SystemViewEvent {
            sequence: self.next_event_sequence,
            event_id: packet.event_id,
            kind: event_kind(packet.event_id).to_string(),
            target_cycles: self.last_target_cycles,
            delta_cycles: packet.delta_cycles,
            context_id,
            value,
            text,
        };
        self.next_event_sequence = self.next_event_sequence.saturating_add(1);
        self.events.push_back(event.clone());
        while self.events.len() > MAX_EVENT_HISTORY {
            self.events.pop_front();
        }
        event
    }

    fn snapshot(&self, decoder_dropped_chunks: u64) -> SystemViewSnapshot {
        let mut tasks = self
            .tasks
            .iter()
            .map(|(id, task)| {
                let mut runtime_cycles = task.runtime_cycles;
                if self
                    .active_task
                    .is_some_and(|(active_id, _)| active_id == *id)
                {
                    if let Some((_, started_at)) = self.active_task {
                        runtime_cycles = runtime_cycles
                            .saturating_add(self.last_target_cycles.saturating_sub(started_at));
                    }
                }
                SystemViewTaskSnapshot {
                    id: *id,
                    name: task.name.clone(),
                    priority: task.priority,
                    runtime_cycles,
                    switches: task.switches,
                }
            })
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            right
                .runtime_cycles
                .cmp(&left.runtime_cycles)
                .then_with(|| left.id.cmp(&right.id))
        });

        SystemViewSnapshot {
            generation: self.generation,
            channel_index: self.channel_index,
            phase: self.phase,
            control_available: self.control_available,
            event_count: self.event_count,
            task_count: tasks.len(),
            metadata_sync_attempts: self.metadata_sync_attempts,
            metadata_sync_max_attempts: METADATA_RETRY_DELAYS_MS.len() as u32,
            target_overflow_packets: self.target_overflow_packets,
            target_dropped_events: self.target_dropped_events,
            decoder_dropped_chunks,
            decoder_errors: self.decoder_errors,
            presentation_dropped_events: self.presentation_dropped_events,
            cleared_through_sequence: self.cleared_through_sequence,
            sys_freq_hz: self.sys_freq_hz,
            cpu_freq_hz: self.cpu_freq_hz,
            ram_base: self.ram_base,
            id_shift: self.id_shift,
            system_description: self.system_description.clone(),
            window_start_cycles: self.window_start_cycles,
            last_target_cycles: self.last_target_cycles,
            tasks,
        }
    }

    fn history(&self, limit: usize) -> SystemViewHistoryResponse {
        let limit = limit.clamp(1, MAX_EVENT_HISTORY);
        let skip = self.events.len().saturating_sub(limit);
        SystemViewHistoryResponse {
            generation: self.generation,
            channel_index: self.channel_index,
            events: self.events.iter().skip(skip).cloned().collect(),
        }
    }

    fn mark_input_gap(&mut self) {
        self.close_active_task(self.last_target_cycles);
        self.interrupted_task = None;
        self.isr_depth = 0;
    }

    fn clear(&mut self) {
        let phase = self.phase;
        let last_target_cycles = self.last_target_cycles;
        let active_task_id = self.active_task.map(|(task_id, _)| task_id);
        let interrupted_task = self.interrupted_task;
        let isr_depth = self.isr_depth;
        let task_metadata = self
            .tasks
            .iter()
            .map(|(id, task)| {
                (
                    *id,
                    TaskState {
                        name: task.name.clone(),
                        priority: task.priority,
                        runtime_cycles: 0,
                        switches: 0,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        self.event_count = 0;
        self.target_overflow_packets = 0;
        self.target_dropped_events = 0;
        self.decoder_errors = 0;
        self.presentation_dropped_events = 0;
        self.events.clear();
        self.cleared_through_sequence = self.next_event_sequence.saturating_sub(1);
        self.tasks = task_metadata;
        self.active_task = active_task_id.map(|task_id| (task_id, last_target_cycles));
        self.interrupted_task = interrupted_task;
        self.isr_depth = isr_depth;
        self.phase = phase;
        self.window_start_cycles = last_target_cycles;
        self.last_target_cycles = last_target_cycles;
    }
}

struct SystemViewShared {
    state: Mutex<TraceState>,
    decoder: Mutex<SystemViewDecoder>,
    decoder_dropped_chunks: Arc<AtomicU64>,
}

impl SystemViewShared {
    fn new(
        generation: u64,
        channel_index: u32,
        control_available: bool,
        decoder_dropped_chunks: Arc<AtomicU64>,
    ) -> Self {
        Self {
            state: Mutex::new(TraceState::new(
                generation,
                channel_index,
                control_available,
            )),
            decoder: Mutex::new(SystemViewDecoder::default()),
            decoder_dropped_chunks,
        }
    }

    fn ingest(&self, chunk: StoredRttChunk) -> Vec<SystemViewEvent> {
        let (packets, errors) = self
            .decoder
            .lock()
            .map(|mut decoder| decoder.push(&chunk.data))
            .unwrap_or_default();
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        state.decoder_errors = state.decoder_errors.saturating_add(errors);
        packets
            .into_iter()
            .map(|packet| state.apply(packet))
            .collect()
    }

    fn mark_input_gap(&self) {
        if let Ok(mut decoder) = self.decoder.lock() {
            decoder.reset_after_gap();
        }
        if let Ok(mut state) = self.state.lock() {
            state.mark_input_gap();
        }
    }

    fn snapshot(&self) -> SystemViewSnapshot {
        let drops = self.decoder_dropped_chunks.load(Ordering::Relaxed);
        self.state
            .lock()
            .map(|state| state.snapshot(drops))
            .unwrap_or_else(|_| SystemViewSnapshot {
                generation: 0,
                channel_index: 0,
                phase: SystemViewPhase::Idle,
                control_available: false,
                event_count: 0,
                task_count: 0,
                metadata_sync_attempts: 0,
                metadata_sync_max_attempts: METADATA_RETRY_DELAYS_MS.len() as u32,
                target_overflow_packets: 0,
                target_dropped_events: 0,
                decoder_dropped_chunks: drops,
                decoder_errors: 0,
                presentation_dropped_events: 0,
                cleared_through_sequence: 0,
                sys_freq_hz: None,
                cpu_freq_hz: None,
                ram_base: None,
                id_shift: None,
                system_description: Vec::new(),
                window_start_cycles: 0,
                last_target_cycles: 0,
                tasks: Vec::new(),
            })
    }

    fn history(&self, limit: usize) -> SystemViewHistoryResponse {
        self.state
            .lock()
            .map(|state| state.history(limit))
            .unwrap_or(SystemViewHistoryResponse {
                generation: 0,
                channel_index: 0,
                events: Vec::new(),
            })
    }

    fn set_metadata_sync_attempts(&self, attempts: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.metadata_sync_attempts = attempts.min(u32::MAX as usize) as u32;
        }
    }

    fn clear(&self) {
        // "Clear" is a presentation/statistics operation, not a transport reset. Keep any
        // partially received packet in the streaming decoder so clearing a live recording cannot
        // desynchronize the SystemView byte stream.
        if let Ok(mut state) = self.state.lock() {
            state.clear();
        }
    }

    fn record_presentation_drop(&self, count: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.presentation_dropped_events = state
                .presentation_dropped_events
                .saturating_add(count as u64);
        }
    }
}

pub struct SystemViewRuntime {
    channel_index: u32,
    shared: Arc<SystemViewShared>,
    stopping: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

pub struct SystemViewSpawn {
    pub presenter: SystemViewPresenter,
    pub metadata_refresh: SystemViewMetadataRefresh,
    pub generation: u64,
    pub channel_index: u32,
    pub control_available: bool,
    pub bootstrap: Vec<StoredRttChunk>,
    pub subscription: ObservationSubscription<StoredRttChunk>,
    pub decoder_dropped_chunks: Arc<AtomicU64>,
}

impl SystemViewRuntime {
    pub fn spawn(config: SystemViewSpawn) -> Result<Self, String> {
        let SystemViewSpawn {
            presenter,
            metadata_refresh,
            generation,
            channel_index,
            control_available,
            bootstrap,
            subscription,
            decoder_dropped_chunks,
        } = config;
        let shared = Arc::new(SystemViewShared::new(
            generation,
            channel_index,
            control_available,
            decoder_dropped_chunks,
        ));
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_shared = Arc::clone(&shared);
        let worker_stopping = Arc::clone(&stopping);
        let worker = std::thread::Builder::new()
            .name(format!("sysview-{channel_index}"))
            .spawn(move || {
                run_decoder(
                    presenter,
                    metadata_refresh,
                    bootstrap,
                    subscription,
                    worker_shared,
                    worker_stopping,
                )
            })
            .map_err(|error| format!("启动 SystemView decoder 失败: {error}"))?;

        Ok(Self {
            channel_index,
            shared,
            stopping,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn channel_index(&self) -> u32 {
        self.channel_index
    }

    pub fn snapshot(&self) -> SystemViewSnapshot {
        self.shared.snapshot()
    }

    pub fn set_control_available(&self, available: bool) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.control_available = available;
        }
    }

    pub fn history(&self, limit: usize) -> SystemViewHistoryResponse {
        self.shared.history(limit)
    }

    pub fn clear(&self) {
        self.shared.clear();
    }

    pub fn shutdown(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                if handle.thread().id() != std::thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
    }
}

impl Drop for SystemViewRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug, Default)]
struct MetadataRetryCoordinator {
    unknown_tasks: Vec<u32>,
    attempts: usize,
    next_retry: Option<Instant>,
}

impl MetadataRetryCoordinator {
    fn update(&mut self, snapshot: &SystemViewSnapshot, now: Instant) -> bool {
        let mut unknown_tasks = snapshot
            .tasks
            .iter()
            .filter(|task| task.name.is_none() || task.priority.is_none())
            .map(|task| task.id)
            .collect::<Vec<_>>();
        unknown_tasks.sort_unstable();

        if unknown_tasks != self.unknown_tasks {
            self.unknown_tasks = unknown_tasks;
            self.attempts = 0;
            self.next_retry = None;
        }

        if self.unknown_tasks.is_empty() || !snapshot.control_available {
            self.next_retry = None;
            return false;
        }

        if self.attempts >= METADATA_RETRY_DELAYS_MS.len() {
            self.next_retry = None;
            return false;
        }

        let next_retry = self.next_retry.get_or_insert_with(|| {
            now + Duration::from_millis(METADATA_RETRY_DELAYS_MS[self.attempts])
        });
        now >= *next_retry
    }

    fn mark_dispatched(&mut self, now: Instant) {
        self.attempts = self.attempts.saturating_add(1);
        self.next_retry = (self.attempts < METADATA_RETRY_DELAYS_MS.len()).then(|| {
            now + Duration::from_millis(METADATA_RETRY_DELAYS_MS[self.attempts])
        });
    }

    fn defer(&mut self, now: Instant) {
        self.next_retry = Some(now + Duration::from_millis(250));
    }
}

fn run_decoder(
    presenter: SystemViewPresenter,
    metadata_refresh: SystemViewMetadataRefresh,
    bootstrap: Vec<StoredRttChunk>,
    subscription: ObservationSubscription<StoredRttChunk>,
    shared: Arc<SystemViewShared>,
    stopping: Arc<AtomicBool>,
) {
    let replay_through_sequence = bootstrap.last().map_or(0, |chunk| chunk.sequence);
    let mut pending = VecDeque::<SystemViewEvent>::new();
    let mut expected_channel_offset = None;
    for chunk in bootstrap {
        if expected_channel_offset.is_some_and(|expected| chunk.channel_offset != expected) {
            shared.mark_input_gap();
        }
        expected_channel_offset =
            Some(chunk.channel_offset.saturating_add(chunk.data.len() as u64));
        pending.extend(shared.ingest(chunk));
        while pending.len() > MAX_PENDING_PRESENTATION_EVENTS {
            pending.pop_front();
            shared.record_presentation_drop(1);
        }
    }

    let mut last_flush = Instant::now();
    let mut last_snapshot = Instant::now();
    let mut metadata_retry = MetadataRetryCoordinator::default();
    let mut changed = true;

    while !stopping.load(Ordering::Acquire) {
        match subscription.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => {
                if chunk.sequence <= replay_through_sequence {
                    continue;
                }
                if expected_channel_offset.is_some_and(|expected| chunk.channel_offset != expected)
                {
                    // Channel offsets are canonical and contiguous for one RTT Up stream. A
                    // discontinuity is therefore an exact acquisition/subscriber gap signal,
                    // unlike sampling the asynchronous drop counter which could reset the decoder
                    // before an older queued chunk is consumed.
                    shared.mark_input_gap();
                }
                expected_channel_offset =
                    Some(chunk.channel_offset.saturating_add(chunk.data.len() as u64));
                for event in shared.ingest(chunk) {
                    pending.push_back(event);
                }
                while pending.len() > MAX_PENDING_PRESENTATION_EVENTS {
                    pending.pop_front();
                    shared.record_presentation_drop(1);
                }
                changed = true;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let now = Instant::now();
        let metadata_snapshot = shared.snapshot();
        let attempts_before = metadata_retry.attempts;
        if metadata_retry.update(&metadata_snapshot, now) {
            if metadata_refresh() {
                metadata_retry.mark_dispatched(now);
            } else {
                metadata_retry.defer(now);
            }
        }
        if metadata_retry.attempts != attempts_before {
            shared.set_metadata_sync_attempts(metadata_retry.attempts);
            changed = true;
        }

        if last_flush.elapsed() >= PRESENTATION_FLUSH_INTERVAL && !pending.is_empty() {
            let events = pending.drain(..).collect::<Vec<_>>();
            let snapshot = shared.snapshot();
            presenter(SystemViewPresentation::Batch { events, snapshot });
            last_flush = Instant::now();
            last_snapshot = Instant::now();
            changed = false;
        } else if changed && last_snapshot.elapsed() >= SNAPSHOT_INTERVAL {
            let snapshot = shared.snapshot();
            presenter(SystemViewPresentation::Snapshot { snapshot });
            last_snapshot = Instant::now();
            changed = false;
        }
    }

    if !pending.is_empty() {
        let snapshot = shared.snapshot();
        presenter(SystemViewPresentation::Batch {
            events: pending.drain(..).collect::<Vec<_>>(),
            snapshot,
        });
    }
}

#[derive(Debug, Clone)]
struct ParsedPacket {
    event_id: u32,
    fields: Vec<u32>,
    text: Option<String>,
    delta_cycles: u32,
    sync_boundary: bool,
}

#[derive(Default)]
struct SystemViewDecoder {
    buffer: Vec<u8>,
    synchronized: bool,
    mark_next_sync_boundary: bool,
}

impl SystemViewDecoder {
    fn reset_after_gap(&mut self) {
        self.buffer.clear();
        self.synchronized = false;
        self.mark_next_sync_boundary = false;
    }

    fn push(&mut self, bytes: &[u8]) -> (Vec<ParsedPacket>, u64) {
        self.buffer.extend_from_slice(bytes);
        let mut packets = Vec::new();
        let mut errors = 0u64;

        loop {
            if self.buffer.is_empty() {
                break;
            }

            if !self.synchronized {
                if let Some(sync_offset) = find_sync_marker(&self.buffer) {
                    self.buffer.drain(..sync_offset + 10);
                    self.synchronized = true;
                    self.mark_next_sync_boundary = true;
                    continue;
                }

                // A semantic observer may attach in the middle of an already running RTT stream.
                // Standard SystemView events below ID 24 do not carry a payload length, so
                // guessing packet boundaries before SEGGER's 10-byte sync marker can create
                // convincing but false events. Discard unsynchronized bytes while retaining only
                // a possible split sync suffix.
                let trailing_zeros = self
                    .buffer
                    .iter()
                    .rev()
                    .take_while(|byte| **byte == 0)
                    .count()
                    .min(9);
                let discard = self.buffer.len().saturating_sub(trailing_zeros);
                self.buffer.drain(..discard);
                break;
            }

            if self.buffer.len() >= 10 && self.buffer[..10].iter().all(|byte| *byte == 0) {
                self.buffer.drain(..10);
                self.mark_next_sync_boundary = true;
                continue;
            }

            match decode_packet(&self.buffer) {
                Ok(Some((mut packet, consumed))) => {
                    self.buffer.drain(..consumed);
                    packet.sync_boundary = self.mark_next_sync_boundary;
                    self.mark_next_sync_boundary = false;
                    packets.push(packet);
                }
                Ok(None) => break,
                Err(()) => {
                    self.buffer.remove(0);
                    errors = errors.saturating_add(1);
                }
            }
        }

        if self.buffer.len() > MAX_DECODER_BUFFER {
            self.buffer.clear();
            errors = errors.saturating_add(1);
        }
        (packets, errors)
    }
}

fn find_sync_marker(data: &[u8]) -> Option<usize> {
    data.windows(10)
        .position(|window| window.iter().all(|byte| *byte == 0))
}

fn decode_packet(data: &[u8]) -> Result<Option<(ParsedPacket, usize)>, ()> {
    let Some((event_id, event_id_len)) = decode_varint(data)? else {
        return Ok(None);
    };
    if event_id < 24 {
        return decode_standard_packet(event_id, event_id_len, data);
    }

    let Some((payload_len, len_len)) = decode_varint(&data[event_id_len..])? else {
        return Ok(None);
    };
    let payload_len = usize::try_from(payload_len).map_err(|_| ())?;
    if payload_len > MAX_DECODER_BUFFER {
        return Err(());
    }
    let payload_start = event_id_len + len_len;
    let payload_end = payload_start.checked_add(payload_len).ok_or(())?;
    if data.len() < payload_end {
        return Ok(None);
    }
    let Some((delta_cycles, timestamp_len)) = decode_varint(&data[payload_end..])? else {
        return Ok(None);
    };
    let (fields, text) =
        decode_length_delimited_payload(event_id, &data[payload_start..payload_end]);
    Ok(Some((
        ParsedPacket {
            event_id,
            fields,
            text,
            delta_cycles,
            sync_boundary: false,
        },
        payload_end + timestamp_len,
    )))
}

fn decode_standard_packet(
    event_id: u32,
    event_id_len: usize,
    data: &[u8],
) -> Result<Option<(ParsedPacket, usize)>, ()> {
    let mut cursor = event_id_len;
    let mut fields = Vec::new();
    let mut text = None;

    let field_count = match event_id {
        0 | 3 | 5 | 10 | 11 | 17 | 18 | 20 => 0,
        1 | 2 | 4 | 6 | 8 | 12 | 15 | 16 | 19 => 1,
        7 | 13 | 23 => 2,
        21 => 4,
        9 | 14 | 22 => 0,
        _ => return Err(()),
    };

    for _ in 0..field_count {
        let Some((value, consumed)) = decode_varint(&data[cursor..])? else {
            return Ok(None);
        };
        fields.push(value);
        cursor += consumed;
    }

    match event_id {
        9 => {
            for _ in 0..2 {
                let Some((value, consumed)) = decode_varint(&data[cursor..])? else {
                    return Ok(None);
                };
                fields.push(value);
                cursor += consumed;
            }
            let Some((value, consumed)) = decode_string(&data[cursor..])? else {
                return Ok(None);
            };
            text = Some(value);
            cursor += consumed;
        }
        14 => {
            let Some((value, consumed)) = decode_string(&data[cursor..])? else {
                return Ok(None);
            };
            text = Some(value);
            cursor += consumed;
        }
        22 => {
            for _ in 0..2 {
                let Some((value, consumed)) = decode_varint(&data[cursor..])? else {
                    return Ok(None);
                };
                fields.push(value);
                cursor += consumed;
            }
            let Some((value, consumed)) = decode_string(&data[cursor..])? else {
                return Ok(None);
            };
            text = Some(value);
            cursor += consumed;
        }
        _ => {}
    }

    let Some((delta_cycles, timestamp_len)) = decode_varint(&data[cursor..])? else {
        return Ok(None);
    };
    cursor += timestamp_len;

    Ok(Some((
        ParsedPacket {
            event_id,
            fields,
            text,
            delta_cycles,
            sync_boundary: false,
        },
        cursor,
    )))
}

fn decode_length_delimited_payload(event_id: u32, payload: &[u8]) -> (Vec<u32>, Option<String>) {
    let mut cursor = 0usize;
    let mut fields = Vec::new();
    let mut text = None;

    match event_id {
        24 => {
            for _ in 0..4 {
                let Some(value) = take_payload_varint(payload, &mut cursor) else {
                    break;
                };
                fields.push(value);
            }
        }
        25 => {
            if let Some(value) = take_payload_varint(payload, &mut cursor) {
                fields.push(value);
            }
            if let Ok(Some((value, _))) = decode_string(payload.get(cursor..).unwrap_or_default()) {
                text = Some(value);
            }
        }
        27 | 29 => {
            if let Some(value) = take_payload_varint(payload, &mut cursor) {
                fields.push(value);
            }
        }
        31 => {
            if let Some(value) = take_payload_varint(payload, &mut cursor) {
                fields.push(value);
            }
            if fields.first().copied() == Some(1) {
                if let Some(marker_id) = take_payload_varint(payload, &mut cursor) {
                    fields.push(marker_id);
                }
                if let Ok(Some((value, _))) =
                    decode_string(payload.get(cursor..).unwrap_or_default())
                {
                    text = Some(value);
                }
            }
        }
        _ => {
            while cursor < payload.len() && fields.len() < 4 {
                let Some(value) = take_payload_varint(payload, &mut cursor) else {
                    break;
                };
                fields.push(value);
            }
        }
    }

    (fields, text)
}

fn take_payload_varint(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let (value, consumed) = decode_varint(payload.get(*cursor..)?).ok().flatten()?;
    *cursor += consumed;
    Some(value)
}

fn decode_varint(data: &[u8]) -> Result<Option<(u32, usize)>, ()> {
    let mut value = 0u32;
    for (index, byte) in data.iter().copied().take(5).enumerate() {
        let shift = index * 7;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(Some((value, index + 1)));
        }
    }
    if data.len() < 5 {
        Ok(None)
    } else {
        Err(())
    }
}

fn decode_string(data: &[u8]) -> Result<Option<(String, usize)>, ()> {
    let Some(first) = data.first().copied() else {
        return Ok(None);
    };
    let (length, prefix) = if first == 255 {
        if data.len() < 3 {
            return Ok(None);
        }
        ((((data[1] as usize) << 8) | data[2] as usize), 3usize)
    } else {
        (first as usize, 1usize)
    };
    let end = prefix.checked_add(length).ok_or(())?;
    if data.len() < end {
        return Ok(None);
    }
    Ok(Some((
        String::from_utf8_lossy(&data[prefix..end]).into_owned(),
        end,
    )))
}

fn event_kind(event_id: u32) -> &'static str {
    match event_id {
        0 => "nop",
        1 => "overflow",
        2 => "isr_enter",
        3 => "isr_exit",
        4 => "task_start_exec",
        5 => "task_stop_exec",
        6 => "task_start_ready",
        7 => "task_stop_ready",
        8 => "task_create",
        9 => "task_info",
        10 => "trace_start",
        11 => "trace_stop",
        12 => "systime_cycles",
        13 => "systime_us",
        14 => "system_description",
        15 => "marker_start",
        16 => "marker_stop",
        17 => "idle",
        18 => "isr_to_scheduler",
        19 => "timer_enter",
        20 => "timer_exit",
        21 => "stack_info",
        22 => "module_description",
        23 => "data_sample",
        24 => "init",
        25 => "resource_name",
        26 => "formatted_print",
        27 => "module_count",
        28 => "end_call",
        29 => "task_terminate",
        31 => "extended",
        _ => "user_event",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_varint(mut value: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }

    fn packet(event_id: u8, fields: &[u32], delta: u32) -> Vec<u8> {
        let mut bytes = vec![event_id];
        for field in fields {
            bytes.extend(encode_varint(*field));
        }
        bytes.extend(encode_varint(delta));
        bytes
    }

    #[test]
    fn decodes_split_task_switch_packets_after_sync() {
        let mut decoder = SystemViewDecoder::default();
        let mut start = vec![0u8; 10];
        let bytes = packet(4, &[0x123], 8);
        start.extend_from_slice(&bytes[..2]);
        let (first, errors) = decoder.push(&start);
        assert!(first.is_empty());
        assert_eq!(errors, 0);

        let (second, errors) = decoder.push(&bytes[2..]);
        assert_eq!(errors, 0);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].event_id, 4);
        assert_eq!(second[0].fields, vec![0x123]);
        assert_eq!(second[0].delta_cycles, 8);
        assert!(second[0].sync_boundary);
    }

    #[test]
    fn skips_live_sync_prefix() {
        let mut decoder = SystemViewDecoder::default();
        let mut bytes = vec![0u8; 10];
        bytes.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&bytes);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, 10);
    }

    #[test]
    fn decodes_length_delimited_init_packet() {
        let mut payload = Vec::new();
        for field in [1_000_000u32, 72_000_000, 0x2000_0000, 0] {
            payload.extend(encode_varint(field));
        }
        let mut bytes = encode_varint(24);
        bytes.extend(encode_varint(payload.len() as u32));
        bytes.extend(payload);
        bytes.extend(encode_varint(3));

        let (decoded, consumed) = decode_packet(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(decoded.event_id, 24);
        assert_eq!(decoded.fields[0], 1_000_000);
        assert_eq!(decoded.fields[1], 72_000_000);
        assert_eq!(decoded.fields[2], 0x2000_0000);
        assert_eq!(decoded.delta_cycles, 3);
    }

    #[test]
    fn target_overflow_does_not_charge_unknown_gap_to_active_task() {
        let mut state = TraceState::new(7, 1, true);
        state.apply(ParsedPacket {
            event_id: 4,
            fields: vec![2],
            text: None,
            delta_cycles: 10,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 1,
            fields: vec![12],
            text: None,
            delta_cycles: 100,
            sync_boundary: false,
        });

        let snapshot = state.snapshot(0);
        assert_eq!(snapshot.tasks[0].runtime_cycles, 0);
        assert_eq!(snapshot.target_dropped_events, 12);
        assert_eq!(snapshot.last_target_cycles, 110);
    }

    #[test]
    fn decoder_gap_requires_a_new_sync_marker() {
        let mut decoder = SystemViewDecoder::default();
        let mut synced = vec![0; 10];
        synced.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&synced);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);

        decoder.reset_after_gap();
        let (packets, errors) = decoder.push(&packet(4, &[7], 1));
        assert_eq!(errors, 0);
        assert!(packets.is_empty());

        let mut resynced = vec![0; 10];
        resynced.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&resynced);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].sync_boundary);
    }

    #[test]
    fn trace_state_tracks_runtime_and_target_overflow_separately() {
        let mut state = TraceState::new(7, 1, true);
        state.apply(ParsedPacket {
            event_id: 4,
            fields: vec![2],
            text: None,
            delta_cycles: 10,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 5,
            fields: Vec::new(),
            text: None,
            delta_cycles: 20,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 1,
            fields: vec![4],
            text: None,
            delta_cycles: 1,
            sync_boundary: false,
        });
        let snapshot = state.snapshot(3);
        assert_eq!(snapshot.tasks[0].runtime_cycles, 20);
        assert_eq!(snapshot.target_overflow_packets, 1);
        assert_eq!(snapshot.target_dropped_events, 4);
        assert_eq!(snapshot.decoder_dropped_chunks, 3);
    }

    #[test]
    fn split_sync_marker_waits_and_then_resynchronizes() {
        let mut decoder = SystemViewDecoder::default();
        let (packets, errors) = decoder.push(&[0, 0, 0, 0, 0]);
        assert!(packets.is_empty());
        assert_eq!(errors, 0);

        let mut rest = vec![0; 5];
        rest.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&rest);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, 10);
    }

    #[test]
    fn nop_with_zero_timestamp_does_not_look_like_partial_sync_forever() {
        let mut decoder = SystemViewDecoder::default();
        let mut bytes = vec![0; 10];
        bytes.extend([0, 0]);
        bytes.extend(packet(4, &[7], 1));
        let (packets, errors) = decoder.push(&bytes);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].event_id, 0);
        assert_eq!(packets[1].event_id, 4);
    }

    #[test]
    fn ignores_midstream_bytes_until_a_sync_marker_arrives() {
        let mut decoder = SystemViewDecoder::default();
        let (packets, errors) = decoder.push(&packet(4, &[7], 1));
        assert!(packets.is_empty());
        assert_eq!(errors, 0);

        let mut synced = vec![0; 10];
        synced.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&synced);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].event_id, 10);
        assert!(packets[0].sync_boundary);
    }

    #[test]
    fn clear_preserves_stream_time_phase_and_task_metadata() {
        let mut state = TraceState::new(7, 1, true);
        state.apply(ParsedPacket {
            event_id: 9,
            fields: vec![2, 5],
            text: Some("worker".to_string()),
            delta_cycles: 1,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 10,
            fields: Vec::new(),
            text: None,
            delta_cycles: 1,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 4,
            fields: vec![2],
            text: None,
            delta_cycles: 8,
            sync_boundary: false,
        });
        let before = state.last_target_cycles;
        state.clear();

        let snapshot = state.snapshot(0);
        assert_eq!(snapshot.phase, SystemViewPhase::Recording);
        assert_eq!(snapshot.last_target_cycles, before);
        assert_eq!(snapshot.event_count, 0);
        assert_eq!(snapshot.cleared_through_sequence, 3);
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.tasks[0].name.as_deref(), Some("worker"));
        assert_eq!(snapshot.tasks[0].runtime_cycles, 0);
    }

    #[test]
    fn task_runtime_excludes_nested_isr_time() {
        let mut state = TraceState::new(7, 1, true);
        state.apply(ParsedPacket {
            event_id: 4,
            fields: vec![2],
            text: None,
            delta_cycles: 10,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 2,
            fields: vec![15],
            text: None,
            delta_cycles: 20,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 2,
            fields: vec![16],
            text: None,
            delta_cycles: 5,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 3,
            fields: Vec::new(),
            text: None,
            delta_cycles: 7,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 3,
            fields: Vec::new(),
            text: None,
            delta_cycles: 8,
            sync_boundary: false,
        });
        state.apply(ParsedPacket {
            event_id: 5,
            fields: Vec::new(),
            text: None,
            delta_cycles: 30,
            sync_boundary: false,
        });

        let snapshot = state.snapshot(0);
        assert_eq!(snapshot.tasks[0].runtime_cycles, 50);
        assert_eq!(snapshot.last_target_cycles, 80);
    }

    #[test]
    fn sync_starts_a_new_delta_epoch_without_inventing_absolute_time() {
        let mut decoder = SystemViewDecoder::default();
        let mut state = TraceState::new(7, 1, true);

        let mut first = vec![0; 10];
        first.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&first);
        assert_eq!(errors, 0);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].sync_boundary);
        state.apply(packets.into_iter().next().unwrap());

        let (packets, errors) = decoder.push(&packet(4, &[2], 20));
        assert_eq!(errors, 0);
        state.apply(packets.into_iter().next().unwrap());
        assert_eq!(state.last_target_cycles, 20);

        let mut restart = vec![0; 10];
        restart.extend(packet(10, &[], 0));
        let (packets, errors) = decoder.push(&restart);
        assert_eq!(errors, 0);
        assert!(packets[0].sync_boundary);
        state.apply(packets.into_iter().next().unwrap());

        let (packets, errors) = decoder.push(&packet(4, &[2], 5));
        assert_eq!(errors, 0);
        state.apply(packets.into_iter().next().unwrap());
        assert_eq!(state.last_target_cycles, 25);
    }
}
