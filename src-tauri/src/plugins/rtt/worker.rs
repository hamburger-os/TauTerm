use super::backend::{open_backend, RttBackend};
use super::config::RttConfig;
use super::error::{RttError, RttErrorCode};
use super::model::{RttChannelInfo, RttPhase, RttReadChunk};
use super::runtime::RttShared;
use crate::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const PRESENTATION_FLUSH_INTERVAL: Duration = Duration::from_millis(25);
const PRESENTATION_FLUSH_BYTES: usize = 8 * 1024;

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

pub(super) fn run(
    config: RttConfig,
    app: AppHandle,
    session_id: String,
    shared: Arc<RttShared>,
    shutting_down: Arc<AtomicBool>,
    worker_exited: Arc<AtomicBool>,
    command_rx: mpsc::Receiver<WorkerCommand>,
    startup_tx: mpsc::SyncSender<Result<(), RttError>>,
) {
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

    let mut pending: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut pending_bytes = 0usize;
    let mut last_flush = Instant::now();
    let mut fatal_error: Option<RttError> = None;

    'worker: loop {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                WorkerCommand::Write {
                    channel_index,
                    data,
                    reply,
                } => {
                    let result =
                        write_all(backend.as_mut(), channel_index, &data, config.write_timeout);
                    if let Ok(bytes) = result.as_ref() {
                        shared.record_tx(*bytes);
                    }
                    let _ = reply.send(result);
                }
                WorkerCommand::RefreshChannels { reply } => {
                    let result = backend.refresh_channels();
                    if let Ok(channels) = result.as_ref() {
                        shared.update_channels(channels.clone());
                        let _ = emit_snapshot(&app, &session_id, &shared);
                    }
                    let _ = reply.send(result);
                }
                WorkerCommand::Shutdown { reply } => {
                    let _ =
                        flush_pending(&app, &session_id, &shared, &mut pending, &mut pending_bytes);
                    backend.shutdown();
                    let _ = reply.send(());
                    break 'worker;
                }
            }
        }

        let mut reads = Vec::<RttReadChunk>::new();
        if let Err(error) = backend.poll(&mut reads) {
            fatal_error = Some(error);
            break;
        }
        for chunk in reads {
            pending_bytes = pending_bytes.saturating_add(chunk.data.len());
            pending
                .entry(chunk.channel_index)
                .or_default()
                .extend_from_slice(&chunk.data);
        }
        if pending_bytes >= PRESENTATION_FLUSH_BYTES
            || (!pending.is_empty() && last_flush.elapsed() >= PRESENTATION_FLUSH_INTERVAL)
        {
            let _ = flush_pending(&app, &session_id, &shared, &mut pending, &mut pending_bytes);
            last_flush = Instant::now();
        }
        std::thread::sleep(config.poll_interval);
    }

    let _ = flush_pending(&app, &session_id, &shared, &mut pending, &mut pending_bytes);
    backend.shutdown();
    worker_exited.store(true, Ordering::Release);

    if let Some(error) = fatal_error {
        shared.set_error(error.clone());
        let _ = emit_snapshot(&app, &session_id, &shared);
        if !shutting_down.load(Ordering::Acquire) {
            notify_unexpected_disconnect(app, session_id, error);
        }
    }
}

fn write_all(
    backend: &mut dyn RttBackend,
    channel_index: u32,
    data: &[u8],
    timeout: Duration,
) -> Result<usize, RttError> {
    let deadline = Instant::now() + timeout;
    let mut written = 0usize;
    while written < data.len() {
        let count = backend.write(channel_index, &data[written..])?;
        written = written.saturating_add(count);
        if written == data.len() {
            return Ok(written);
        }
        if Instant::now() >= deadline {
            return Err(RttError::new(
                RttErrorCode::RttWriteTimeout,
                format!(
                    "RTT Down Channel {channel_index} 写入超时（{written}/{} bytes）",
                    data.len()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(written)
}

fn flush_pending(
    app: &AppHandle,
    session_id: &str,
    shared: &Arc<RttShared>,
    pending: &mut BTreeMap<u32, Vec<u8>>,
    pending_bytes: &mut usize,
) -> Result<(), ()> {
    for (channel_index, data) in std::mem::take(pending) {
        if data.is_empty() {
            continue;
        }
        let chunk = shared.record_rx(channel_index, data);
        let _ = app.emit(
            "rtt-event",
            json!({
                "kind": "chunk",
                "session_id": session_id,
                "channel_index": chunk.channel_index,
                "sequence": chunk.sequence,
                "timestamp_ms": chunk.timestamp_ms,
                "data_b64": BASE64.encode(&chunk.data),
            }),
        );
    }
    *pending_bytes = 0;
    Ok(())
}

fn emit_snapshot(app: &AppHandle, session_id: &str, shared: &Arc<RttShared>) -> Result<(), ()> {
    app.emit(
        "rtt-event",
        json!({
            "kind": "snapshot",
            "session_id": session_id,
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
        RttBackendCapabilities, RttBackendDescriptor, RttChannelInfo,
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
    fn write_all_handles_partial_backend_writes() {
        let mut backend = PartialWriteBackend {
            channels: Vec::new(),
            max_write: 2,
            written: Vec::new(),
        };
        let payload = b"partial RTT write";
        let written = write_all(&mut backend, 0, payload, Duration::from_secs(1)).unwrap();
        assert_eq!(written, payload.len());
        assert_eq!(backend.written, payload);
    }
}
