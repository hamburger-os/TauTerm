use super::probe_runtime::{DebugProbeConfig, DebugProbeOpenError, DebugProbeRuntime, DebugWireProtocol};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

const COMMAND_QUEUE_CAPACITY: usize = 64;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DebugTargetKey {
    selector: Option<String>,
    target: String,
    wire_protocol: DebugWireProtocol,
    speed_khz: Option<u32>,
}

impl From<&DebugProbeConfig> for DebugTargetKey {
    fn from(config: &DebugProbeConfig) -> Self {
        Self {
            selector: config.selector.clone(),
            target: config.target.clone(),
            wire_protocol: config.wire_protocol,
            speed_khz: config.speed_khz,
        }
    }
}

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
    #[error("等待嵌入式调试目标操作完成超时")]
    Timeout,
    #[error("调试目标服务 {service} 已被占用")]
    ServiceBusy { service: String },
}

type TargetOperation = Box<dyn FnOnce(&mut DebugProbeRuntime) + Send + 'static>;

enum TargetCommand {
    Execute(TargetOperation),
    Shutdown(mpsc::SyncSender<()>),
}

/// Single-owner debug-target worker shared by RTT and future observation services.
///
/// The probe-rs Session never leaves this worker thread. Protocol/observation services submit
/// short operations and keep their own protocol state outside the target worker.
pub(crate) struct DebugTargetRuntime {
    command_tx: mpsc::SyncSender<TargetCommand>,
    worker: Mutex<Option<JoinHandle<()>>>,
    descriptor: DebugTargetDescriptor,
    active_services: Mutex<HashSet<String>>,
}

impl DebugTargetRuntime {
    fn open(config: DebugProbeConfig) -> Result<Arc<Self>, DebugTargetRuntimeError> {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name("embedded-debug-target".to_string())
            .spawn(move || {
                let mut probe = match DebugProbeRuntime::open(&config) {
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

                while let Ok(command) = command_rx.recv() {
                    match command {
                        TargetCommand::Execute(operation) => operation(&mut probe),
                        TargetCommand::Shutdown(reply) => {
                            let _ = reply.send(());
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
                // Dropping command_tx lets a late-starting worker exit as soon as probe open returns.
                drop(command_tx);
                return Err(DebugTargetRuntimeError::Timeout);
            }
        };

        Ok(Arc::new(Self {
            command_tx,
            worker: Mutex::new(Some(handle)),
            descriptor,
            active_services: Mutex::new(HashSet::new()),
        }))
    }

    pub(crate) fn descriptor(&self) -> &DebugTargetDescriptor {
        &self.descriptor
    }

    /// Execute one short operation on the single owner of the probe-rs Session.
    ///
    /// Scheduling failures are returned separately from the service's own error type so protocol
    /// modules keep ownership of their error taxonomy.
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
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.command_tx
            .try_send(TargetCommand::Execute(Box::new(move |probe| {
                let _ = reply_tx.send(operation(probe));
            })))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => DebugTargetRuntimeError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => DebugTargetRuntimeError::WorkerStopped,
            })?;

        reply_rx
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => DebugTargetRuntimeError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => DebugTargetRuntimeError::WorkerStopped,
            })
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
            runtime: Arc::downgrade(self),
            service,
        })
    }
}

impl Drop for DebugTargetRuntime {
    fn drop(&mut self) {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if self
            .command_tx
            .send(TargetCommand::Shutdown(reply_tx))
            .is_ok()
        {
            let _ = reply_rx.recv_timeout(SHUTDOWN_TIMEOUT);
        }

        if let Ok(worker) = self.worker.get_mut() {
            if let Some(handle) = worker.take() {
                if handle.thread().id() != std::thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
    }
}

pub(crate) struct DebugServiceLease {
    runtime: Weak<DebugTargetRuntime>,
    service: String,
}

impl Drop for DebugServiceLease {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.upgrade() else {
            return;
        };
        if let Ok(mut active) = runtime.active_services.lock() {
            active.remove(&self.service);
        }
    }
}

/// Process-local registry for physical debug targets.
///
/// Entries are weak: the registry does not keep probes open after the last service disconnects.
/// A target is keyed by the connection properties that determine probe/target ownership. Different
/// observation services can share the same target worker while an individual service kind (for
/// example RTT acquisition) is protected by a service lease.
pub(crate) struct EmbeddedDebugManager {
    targets: Mutex<HashMap<DebugTargetKey, Weak<DebugTargetRuntime>>>,
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
        let key = DebugTargetKey::from(config);
        let mut targets = self
            .targets
            .lock()
            .map_err(|_| DebugTargetRuntimeError::WorkerStopped)?;

        if let Some(runtime) = targets.get(&key).and_then(Weak::upgrade) {
            return Ok(runtime);
        }

        targets.retain(|_, runtime| runtime.strong_count() > 0);
        let runtime = DebugTargetRuntime::open(config.clone())?;
        targets.insert(key, Arc::downgrade(&runtime));
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
    fn target_key_keeps_transport_identity_fields() {
        let config = DebugProbeConfig {
            selector: Some("probe".to_string()),
            target: "chip".to_string(),
            wire_protocol: DebugWireProtocol::Swd,
            speed_khz: Some(4_000),
        };
        let key = DebugTargetKey::from(&config);
        assert_eq!(key.selector.as_deref(), Some("probe"));
        assert_eq!(key.target, "chip");
        assert_eq!(key.wire_protocol, DebugWireProtocol::Swd);
        assert_eq!(key.speed_khz, Some(4_000));
    }
}
