use super::probe_runtime::{
    resolve_probe_config, DebugProbeConfig, DebugProbeOpenError, DebugProbeRuntime,
    ResolvedDebugProbeConfig,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const COMMAND_QUEUE_CAPACITY: usize = 64;
const SERVICE_QUEUE_CAPACITY: usize = 16;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub(crate) struct DebugTargetDescriptor {
    pub probe_label: String,
    pub target: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DebugTargetRuntimeError {
    #[error(transparent)]
    Open(#[from] DebugProbeOpenError),
    #[error("启动嵌入式调试目标 worker 失败: {0}")]
    WorkerStart(String),
    #[error("嵌入式调试目标调度队列繁忙")]
    QueueFull,
    #[error("嵌入式调试目标 worker 已停止")]
    WorkerStopped,
    #[error("嵌入式调试目标操作在调度前超时")]
    Timeout,
    #[error("嵌入式调试目标操作执行中超时，结果状态未知")]
    InFlightTimeout,
    #[error("调试目标服务 {service} 已被占用")]
    ServiceBusy { service: String },
    #[error("调试探针 {probe} 已连接到不同目标配置")]
    TargetConfigConflict { probe: String },
}

type TargetOperation = Box<dyn FnOnce(&mut DebugProbeRuntime) + Send + 'static>;

struct ScheduledTargetOperation {
    service: String,
    operation: TargetOperation,
}

#[derive(Default)]
struct TargetSchedulerState {
    queues: HashMap<String, VecDeque<TargetOperation>>,
    service_order: VecDeque<String>,
    total_pending: usize,
    closed: bool,
    shutdown_reply: Option<mpsc::SyncSender<()>>,
}

struct TargetScheduler {
    state: Mutex<TargetSchedulerState>,
    ready: Condvar,
}

enum ScheduledWork {
    Operation(ScheduledTargetOperation),
    Shutdown(Option<mpsc::SyncSender<()>>),
}

impl TargetScheduler {
    fn new() -> Self {
        Self {
            state: Mutex::new(TargetSchedulerState::default()),
            ready: Condvar::new(),
        }
    }

    fn try_enqueue(
        &self,
        service: &str,
        operation: TargetOperation,
    ) -> Result<(), DebugTargetRuntimeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DebugTargetRuntimeError::WorkerStopped)?;
        if state.closed {
            return Err(DebugTargetRuntimeError::WorkerStopped);
        }
        if state.total_pending >= COMMAND_QUEUE_CAPACITY {
            return Err(DebugTargetRuntimeError::QueueFull);
        }

        let queue = state.queues.entry(service.to_string()).or_default();
        if queue.len() >= SERVICE_QUEUE_CAPACITY {
            return Err(DebugTargetRuntimeError::QueueFull);
        }
        let was_empty = queue.is_empty();
        queue.push_back(operation);
        state.total_pending += 1;
        if was_empty {
            state.service_order.push_back(service.to_string());
        }
        self.ready.notify_one();
        Ok(())
    }

    fn request_shutdown(&self, reply: mpsc::SyncSender<()>) {
        if let Ok(mut state) = self.state.lock() {
            if state.closed {
                let _ = reply.send(());
                return;
            }
            state.closed = true;
            state.queues.clear();
            state.service_order.clear();
            state.total_pending = 0;
            state.shutdown_reply = Some(reply);
            self.ready.notify_all();
        } else {
            let _ = reply.send(());
        }
    }

    fn next(&self) -> ScheduledWork {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if state.closed {
                return ScheduledWork::Shutdown(state.shutdown_reply.take());
            }

            if let Some(service) = state.service_order.pop_front() {
                let (operation, remove_queue) = {
                    let queue = state
                        .queues
                        .get_mut(&service)
                        .expect("scheduled service queue must exist");
                    let operation = queue
                        .pop_front()
                        .expect("scheduled service queue must not be empty");
                    (operation, queue.is_empty())
                };
                state.total_pending = state.total_pending.saturating_sub(1);
                if remove_queue {
                    state.queues.remove(&service);
                } else {
                    state.service_order.push_back(service.clone());
                }
                return ScheduledWork::Operation(ScheduledTargetOperation { service, operation });
            }

            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}

/// Single-owner debug-target worker shared by RTT and future observation services.
///
/// The probe-rs Session never leaves this worker thread. Protocol/observation services submit
/// short operations and keep their own protocol state outside the target worker.
pub(crate) struct DebugTargetRuntime {
    scheduler: Arc<TargetScheduler>,
    worker: Mutex<Option<JoinHandle<()>>>,
    connection_config: DebugProbeConfig,
    descriptor: DebugTargetDescriptor,
    active_services: Mutex<HashSet<String>>,
}

impl DebugTargetRuntime {
    fn open(config: ResolvedDebugProbeConfig) -> Result<Arc<Self>, DebugTargetRuntimeError> {
        let connection_config = config.connection_config();
        let scheduler = Arc::new(TargetScheduler::new());
        let worker_scheduler = Arc::clone(&scheduler);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name("embedded-debug-target".to_string())
            .spawn(move || {
                let mut probe = match DebugProbeRuntime::open_resolved(&config) {
                    Ok(probe) => probe,
                    Err(error) => {
                        let _ = startup_tx.send(Err(error));
                        return;
                    }
                };
                let descriptor = DebugTargetDescriptor {
                    probe_label: probe.probe_label().to_string(),
                    target: probe.target().to_string(),
                };
                if startup_tx.send(Ok(descriptor)).is_err() {
                    return;
                }

                loop {
                    match worker_scheduler.next() {
                        ScheduledWork::Operation(operation) => {
                            debug_assert!(!operation.service.is_empty());
                            (operation.operation)(&mut probe);
                        }
                        ScheduledWork::Shutdown(reply) => {
                            if let Some(reply) = reply {
                                let _ = reply.send(());
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|error| DebugTargetRuntimeError::WorkerStart(error.to_string()))?;

        let descriptor = match startup_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(descriptor)) => descriptor,
            Ok(Err(error)) => {
                let _ = handle.join();
                return Err(DebugTargetRuntimeError::Open(error));
            }
            Err(_) => {
                // The worker still owns config/probe open. A late startup observes the dropped
                // startup receiver and exits without accepting target work.
                return Err(DebugTargetRuntimeError::Timeout);
            }
        };

        Ok(Arc::new(Self {
            scheduler,
            worker: Mutex::new(Some(handle)),
            connection_config,
            descriptor,
            active_services: Mutex::new(HashSet::new()),
        }))
    }

    /// Execute one short operation on the single owner of the probe-rs Session.
    ///
    /// A command that waited in the shared queue until its caller deadline is discarded before
    /// touching the target. This prevents a timed-out write/read request from producing a late
    /// target-side effect after another service temporarily occupied the worker.
    fn execute<T, E, F>(
        &self,
        service: &str,
        timeout: Duration,
        operation: F,
    ) -> Result<Result<T, E>, DebugTargetRuntimeError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&mut DebugProbeRuntime) -> Result<T, E> + Send + 'static,
    {
        let deadline = Instant::now() + timeout;
        let started = Arc::new(AtomicBool::new(false));
        let operation_started = Arc::clone(&started);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.scheduler.try_enqueue(
            service,
            Box::new(move |probe| {
                if Instant::now() >= deadline {
                    let _ = reply_tx.send(Err(DebugTargetRuntimeError::Timeout));
                    return;
                }
                operation_started.store(true, Ordering::Release);
                let _ = reply_tx.send(Ok(operation(probe)));
            }),
        )?;

        reply_rx
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout if started.load(Ordering::Acquire) => {
                    DebugTargetRuntimeError::InFlightTimeout
                }
                mpsc::RecvTimeoutError::Timeout => DebugTargetRuntimeError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => DebugTargetRuntimeError::WorkerStopped,
            })?
    }

    pub(crate) fn acquire_service(
        self: &Arc<Self>,
        service: impl Into<String>,
    ) -> Result<DebugServiceLease, DebugTargetRuntimeError> {
        let service = service.into();
        let mut active = self
            .active_services
            .lock()
            .map_err(|_| DebugTargetRuntimeError::WorkerStopped)?;
        if !active.insert(service.clone()) {
            return Err(DebugTargetRuntimeError::ServiceBusy { service });
        }
        Ok(DebugServiceLease {
            runtime: Arc::clone(self),
            service,
        })
    }
}

impl Drop for DebugTargetRuntime {
    fn drop(&mut self) {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.scheduler.request_shutdown(reply_tx);
        let join_safe = reply_rx.recv_timeout(SHUTDOWN_TIMEOUT).is_ok();

        if let Ok(worker) = self.worker.get_mut() {
            if let Some(handle) = worker.take() {
                let same_thread = handle.thread().id() == std::thread::current().id();
                if !same_thread && (join_safe || handle.is_finished()) {
                    let _ = handle.join();
                }
                // A stuck probe operation cannot safely be cancelled. If the bounded shutdown
                // handshake did not complete, dropping JoinHandle deliberately detaches the
                // worker; the scheduler is already closed, so no new operation can reach it.
            }
        }
    }
}

pub(crate) struct DebugServiceLease {
    runtime: Arc<DebugTargetRuntime>,
    service: String,
}

impl DebugServiceLease {
    pub(crate) fn descriptor(&self) -> &DebugTargetDescriptor {
        &self.runtime.descriptor
    }

    pub(crate) fn execute<T, E, F>(
        &self,
        timeout: Duration,
        operation: F,
    ) -> Result<Result<T, E>, DebugTargetRuntimeError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&mut DebugProbeRuntime) -> Result<T, E> + Send + 'static,
    {
        self.runtime.execute(&self.service, timeout, operation)
    }
}

impl Drop for DebugServiceLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.runtime.active_services.lock() {
            active.remove(&self.service);
        };
    }
}

type TargetSlot = Arc<Mutex<Weak<DebugTargetRuntime>>>;

/// Process-local registry for physical debug probes.
///
/// Auto and explicit selectors are canonicalized before lookup. Each canonical physical probe has
/// one slot and therefore at most one active probe-rs Session. Services may share that target
/// runtime only when target/wire/speed settings match; each service kind (for example RTT
/// acquisition) is additionally protected by its own service lease. Slots hold weak runtimes, so
/// the registry never keeps a probe open after the last service disconnects.
pub(crate) struct EmbeddedDebugManager {
    targets: Mutex<HashMap<String, TargetSlot>>,
}

impl EmbeddedDebugManager {
    pub(crate) fn new() -> Self {
        Self {
            targets: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn acquire_target(
        &self,
        config: &DebugProbeConfig,
    ) -> Result<Arc<DebugTargetRuntime>, DebugTargetRuntimeError> {
        let resolved = resolve_probe_config(config)?;
        let selector = resolved.selector.clone();

        let slot = {
            let mut targets = self
                .targets
                .lock()
                .map_err(|_| DebugTargetRuntimeError::WorkerStopped)?;
            targets
                .entry(selector)
                .or_insert_with(|| Arc::new(Mutex::new(Weak::new())))
                .clone()
        };

        let mut runtime_slot = slot
            .lock()
            .map_err(|_| DebugTargetRuntimeError::WorkerStopped)?;
        if let Some(runtime) = runtime_slot.upgrade() {
            if runtime.connection_config == resolved.connection_config() {
                return Ok(runtime);
            }
            return Err(DebugTargetRuntimeError::TargetConfigConflict {
                probe: runtime.descriptor.probe_label.clone(),
            });
        }

        let runtime = DebugTargetRuntime::open(resolved)?;
        *runtime_slot = Arc::downgrade(&runtime);
        Ok(runtime)
    }
}

impl Default for EmbeddedDebugManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_round_robins_services_and_bounds_each_service() {
        let scheduler = TargetScheduler::new();
        for _ in 0..SERVICE_QUEUE_CAPACITY {
            scheduler.try_enqueue("rtt", Box::new(|_| {})).unwrap();
        }
        assert!(matches!(
            scheduler.try_enqueue("rtt", Box::new(|_| {})),
            Err(DebugTargetRuntimeError::QueueFull)
        ));
        scheduler
            .try_enqueue("superwatch", Box::new(|_| {}))
            .unwrap();

        match scheduler.next() {
            ScheduledWork::Operation(operation) => assert_eq!(operation.service, "rtt"),
            ScheduledWork::Shutdown(_) => panic!("unexpected shutdown"),
        }
        match scheduler.next() {
            ScheduledWork::Operation(operation) => assert_eq!(operation.service, "superwatch"),
            ScheduledWork::Shutdown(_) => panic!("unexpected shutdown"),
        }
        match scheduler.next() {
            ScheduledWork::Operation(operation) => assert_eq!(operation.service, "rtt"),
            ScheduledWork::Shutdown(_) => panic!("unexpected shutdown"),
        }
    }

    #[test]
    fn target_connection_config_distinguishes_session_settings_not_service_kind() {
        let config = DebugProbeConfig {
            selector: Some("probe".to_string()),
            target: "chip".to_string(),
            wire_protocol: super::super::probe_runtime::DebugWireProtocol::Swd,
            speed_khz: Some(4_000),
        };
        let same = config.clone();
        let mut different = config.clone();
        different.speed_khz = Some(2_000);

        assert_eq!(config, same);
        assert_ne!(config, different);
    }
}
