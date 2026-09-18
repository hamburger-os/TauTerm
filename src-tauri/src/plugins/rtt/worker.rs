use super::backend::{open_backend, RttBackend};
use super::config::RttConfig;
use super::error::{RttError, RttErrorCode};
use super::model::{RttChannelInfo, RttChunkDto, RttPhase, RttReadChunk, StoredRttChunk};
use super::runtime::RttShared;
use crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry, LogEntry};
use crate::AppState;
use chrono::{Local, TimeZone};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const PRESENTATION_FLUSH_INTERVAL: Duration = Duration::from_millis(25);
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const PRESENTATION_QUEUE_MAX_BYTES: usize = 256 * 1024;
const MAX_COMMANDS_PER_TICK: usize = 8;
const WRITE_QUANTUM_BYTES: usize = 4 * 1024;

pub(super) enum WorkerCommand {
    Write {
        channel_index: u32,
        data: Vec<u8>,
        reply: mpsc::SyncSender<Result<usize, RttError>>,
    },
    RefreshChannels {
        reply: mpsc::SyncSender<Result<Vec<RttChannelInfo>, RttError>>,
    },
    Shutdown {
        reply: mpsc::SyncSender<()>,
    },
}

pub(super) struct WorkerContext {
    pub config: RttConfig,
    pub app: AppHandle,
    pub session_id: String,
    pub shared: Arc<RttShared>,
    pub shutting_down: Arc<AtomicBool>,
    pub worker_exited: Arc<AtomicBool>,
}

struct PendingWrite {
    channel_index: u32,
    data: Vec<u8>,
    offset: usize,
    deadline: Instant,
    reply: mpsc::SyncSender<Result<usize, RttError>>,
}

pub(super) fn run(
    context: WorkerContext,
    command_rx: mpsc::Receiver<WorkerCommand>,
    startup_tx: mpsc::SyncSender<Result<(), RttError>>,
) {
    let WorkerContext {
        config,
        app,
        session_id,
        shared,
        shutting_down,
        worker_exited,
    } = context;

    shared.set_phase(RttPhase::OpeningBackend);
    let mut backend = match open_backend(&config) {
        Ok(backend) => backend,
        Err(error) => {
            shared.set_error(error.clone());
            worker_exited.store(true, Ordering::Release);
            let _ = startup_tx.send(Err(error));
            return;
        }
    };
    shared.set_running(backend.descriptor(), backend.channels().to_vec());
    let _ = emit_snapshot(&app, &session_id, &shared);
    if startup_tx.send(Ok(())).is_err() {
        backend.shutdown();
        worker_exited.store(true, Ordering::Release);
        return;
    }

    let log_tx = app
        .state::<AppState>()
        .log_engine
        .lock()
        .ok()
        .map(|engine| engine.sender());
    let mut presentation = VecDeque::<StoredRttChunk>::new();
    let mut presentation_bytes = 0usize;
    let mut writes = VecDeque::<PendingWrite>::new();
    let mut last_flush = Instant::now();
    let mut last_snapshot = Instant::now();
    let mut fatal_error: Option<RttError> = None;
    let mut shutdown_reply: Option<mpsc::SyncSender<()>> = None;

    'worker: loop {
        for _ in 0..MAX_COMMANDS_PER_TICK {
            match command_rx.try_recv() {
                Ok(WorkerCommand::Write {
                    channel_index,
                    data,
                    reply,
                }) => writes.push_back(PendingWrite {
                    channel_index,
                    data,
                    offset: 0,
                    deadline: Instant::now() + config.write_timeout,
                    reply,
                }),
                Ok(WorkerCommand::RefreshChannels { reply }) => {
                    let result = backend.refresh_channels();
                    if let Ok(channels) = result.as_ref() {
                        shared.set_running(backend.descriptor(), channels.clone());
                        let _ = emit_snapshot(&app, &session_id, &shared);
                    }
                    let _ = reply.send(result);
                }
                Ok(WorkerCommand::Shutdown { reply }) => {
                    shutdown_reply = Some(reply);
                    break 'worker;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'worker,
            }
        }

        // shutdown_inner sets the atomic flag before enqueueing the explicit Shutdown command so
        // fatal-disconnect reporting can distinguish a requested close. Give the command queue one
        // scheduling turn first; if it was already unavailable/full, the atomic flag is still an
        // unconditional escape hatch and shutdown never depends on a successful control enqueue.
        if shutting_down.load(Ordering::Acquire) {
            break;
        }

        let mut reads = Vec::<RttReadChunk>::new();
        if let Err(error) = backend.poll(&mut reads) {
            fatal_error = Some(error);
            break;
        }
        for read in reads {
            let chunk = shared.record_rx(read.channel_index, read.data);
            if let Some(sender) = log_tx.as_ref() {
                log_rtt_data(
                    sender,
                    &session_id,
                    DataDirection::RX,
                    chunk.channel_index,
                    &chunk.data,
                    chunk.timestamp_ms,
                );
            }
            presentation_bytes = presentation_bytes.saturating_add(chunk.data.len());
            presentation.push_back(chunk);
            while presentation_bytes > PRESENTATION_QUEUE_MAX_BYTES {
                let Some(dropped) = presentation.pop_front() else {
                    break;
                };
                presentation_bytes = presentation_bytes.saturating_sub(dropped.data.len());
                shared.record_presentation_drop(dropped.data.len());
            }
        }

        service_one_write(
            backend.as_mut(),
            &shared,
            &mut writes,
            log_tx.as_ref(),
            &session_id,
        );

        if !presentation.is_empty() && last_flush.elapsed() >= PRESENTATION_FLUSH_INTERVAL {
            emit_presentation_batch(
                &app,
                &session_id,
                &mut presentation,
                &mut presentation_bytes,
            );
            last_flush = Instant::now();
        }
        if last_snapshot.elapsed() >= SNAPSHOT_INTERVAL {
            let _ = emit_snapshot(&app, &session_id, &shared);
            last_snapshot = Instant::now();
        }

        std::thread::sleep(config.poll_interval);
    }

    emit_presentation_batch(
        &app,
        &session_id,
        &mut presentation,
        &mut presentation_bytes,
    );
    cancel_pending_writes(&mut writes);
    backend.shutdown();
    worker_exited.store(true, Ordering::Release);
    if let Some(reply) = shutdown_reply {
        let _ = reply.send(());
    }

    if let Some(error) = fatal_error {
        shared.set_error(error.clone());
        let _ = emit_snapshot(&app, &session_id, &shared);
        if !shutting_down.load(Ordering::Acquire) {
            notify_unexpected_disconnect(app, session_id, error);
        }
    }
}

fn service_one_write(
    backend: &mut dyn RttBackend,
    shared: &Arc<RttShared>,
    writes: &mut VecDeque<PendingWrite>,
    log_tx: Option<&mpsc::SyncSender<LogEntry>>,
    session_id: &str,
) {
    let Some(mut pending) = writes.pop_front() else {
        return;
    };

    if Instant::now() >= pending.deadline {
        let _ = pending.reply.send(Err(write_timeout(&pending)));
        return;
    }

    let end = pending
        .offset
        .saturating_add(WRITE_QUANTUM_BYTES)
        .min(pending.data.len());
    match backend.write(pending.channel_index, &pending.data[pending.offset..end]) {
        Ok(count) => {
            let count = count.min(end.saturating_sub(pending.offset));
            if count > 0 {
                if let Some(sender) = log_tx {
                    log_rtt_data(
                        sender,
                        session_id,
                        DataDirection::TX,
                        pending.channel_index,
                        &pending.data[pending.offset..pending.offset + count],
                        now_ms(),
                    );
                }
            }
            pending.offset = pending.offset.saturating_add(count);
            shared.record_tx(count);
            if pending.offset >= pending.data.len() {
                let total = pending.data.len();
                let _ = pending.reply.send(Ok(total));
            } else if Instant::now() >= pending.deadline {
                let _ = pending.reply.send(Err(write_timeout(&pending)));
            } else {
                writes.push_back(pending);
            }
        }
        Err(error) => {
            let _ = pending.reply.send(Err(error));
        }
    }
}

fn write_timeout(pending: &PendingWrite) -> RttError {
    RttError::new(
        RttErrorCode::RttWriteTimeout,
        format!(
            "RTT Down Channel {} 写入超时（{}/{} bytes）",
            pending.channel_index,
            pending.offset,
            pending.data.len()
        ),
    )
}

fn cancel_pending_writes(writes: &mut VecDeque<PendingWrite>) {
    for pending in writes.drain(..) {
        let _ = pending.reply.send(Err(RttError::new(
            RttErrorCode::Cancelled,
            "RTT 会话正在关闭，未完成的写入已取消",
        )));
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn log_rtt_data(
    sender: &mpsc::SyncSender<LogEntry>,
    session_id: &str,
    direction: DataDirection,
    channel_index: u32,
    payload: &[u8],
    timestamp_ms: u64,
) {
    let timestamp = Local
        .timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Local::now);
    try_send_session_log(
        sender,
        DataLogEntry {
            session_id: session_id.to_string(),
            direction,
            // LogWriter treats this prefix as an optional stream label. Ordinary sessions keep
            // using text/hex/dual here, so this remains backward-free and protocol-neutral.
            data_mode: format!("stream:RTT:{channel_index}"),
            encoding: "utf-8".to_string(),
            payload: payload.to_vec(),
            timestamp,
        },
    );
}

fn emit_presentation_batch(
    app: &AppHandle,
    session_id: &str,
    presentation: &mut VecDeque<StoredRttChunk>,
    presentation_bytes: &mut usize,
) {
    if presentation.is_empty() {
        return;
    }
    let chunks = presentation
        .drain(..)
        .map(|chunk| RttChunkDto::from_stored(&chunk))
        .collect::<Vec<_>>();
    *presentation_bytes = 0;
    let generation = chunks.first().map_or(0, |chunk| chunk.generation);
    let _ = app.emit(
        "rtt-event",
        json!({
            "kind": "batch",
            "session_id": session_id,
            "generation": generation,
            "chunks": chunks,
        }),
    );
}

fn emit_snapshot(app: &AppHandle, session_id: &str, shared: &Arc<RttShared>) -> Result<(), ()> {
    app.emit(
        "rtt-event",
        json!({
            "kind": "snapshot",
            "session_id": session_id,
            "generation": shared.generation(),
            "snapshot": shared.snapshot(),
        }),
    )
    .map_err(|_| ())
}

fn notify_unexpected_disconnect(app: AppHandle, session_id: String, error: RttError) {
    std::thread::spawn(move || {
        let state: State<'_, AppState> = app.state();
        if let Ok(mut store) = state.session_store.lock() {
            store.mark_disconnected(&session_id);
        }
        let _ = app.emit(
            "session-disconnected",
            json!({
                "session_id": session_id,
                "reason": error.message,
                "disconnect_info": {
                    "kind": "io_error",
                    "reason": error.message,
                    "retain_terminal": true,
                    "plugin_error_code": error.code.as_str(),
                }
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::rtt::model::{
        RttBackendCapabilities, RttBackendDescriptor, RttChannelInfo, RttSnapshot,
    };

    struct PartialWriteBackend {
        channels: Vec<RttChannelInfo>,
        max_write: usize,
        written: Vec<u8>,
    }

    impl RttBackend for PartialWriteBackend {
        fn descriptor(&self) -> RttBackendDescriptor {
            RttBackendDescriptor {
                kind: "mock".into(),
                display_name: "Mock".into(),
                target: None,
                probe: None,
                control_block_address: None,
                capabilities: RttBackendCapabilities::default(),
            }
        }

        fn channels(&self) -> &[RttChannelInfo] {
            &self.channels
        }

        fn poll(&mut self, _output: &mut Vec<RttReadChunk>) -> Result<(), RttError> {
            Ok(())
        }

        fn write(&mut self, _channel_index: u32, data: &[u8]) -> Result<usize, RttError> {
            let count = data.len().min(self.max_write);
            self.written.extend_from_slice(&data[..count]);
            Ok(count)
        }

        fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
            Ok(self.channels.clone())
        }
    }

    #[test]
    fn write_scheduler_handles_partial_backend_writes_without_busy_loop() {
        let mut backend = PartialWriteBackend {
            channels: Vec::new(),
            max_write: 2,
            written: Vec::new(),
        };
        let shared = Arc::new(RttShared::new(1));
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let payload = b"partial RTT write".to_vec();
        let mut writes = VecDeque::from([PendingWrite {
            channel_index: 0,
            data: payload.clone(),
            offset: 0,
            deadline: Instant::now() + Duration::from_secs(1),
            reply: reply_tx,
        }]);

        while !writes.is_empty() {
            service_one_write(&mut backend, &shared, &mut writes, None, "test");
        }

        assert_eq!(reply_rx.recv().unwrap().unwrap(), payload.len());
        assert_eq!(backend.written, payload);
        let snapshot: RttSnapshot = shared.snapshot();
        assert_eq!(snapshot.tx_bytes, payload.len() as u64);
    }

    #[test]
    fn presentation_drop_is_not_counted_as_history_loss() {
        let shared = RttShared::new(1);
        shared.record_presentation_drop(12);
        let snapshot = shared.snapshot();
        assert_eq!(snapshot.dropped_presentation_chunks, 1);
        assert_eq!(snapshot.dropped_presentation_bytes, 12);
        assert_eq!(snapshot.dropped_history_chunks, 0);
    }
}
