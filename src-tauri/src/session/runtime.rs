use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::session::DisconnectInfo;
use crate::transport::{DataPlaneEvent, DataPlaneHandle, DataPlaneRuntime, TransportError};

/// Owns one live DataPlane actor plus the session-facing receive pump.
///
/// The store keeps this owner instead of separate write channels, cancellation senders and
/// sync/async task variants. All consumers clone `DataPlaneHandle` or create subscriptions.
pub struct SessionDataPlane {
    runtime: Option<DataPlaneRuntime>,
    handle: DataPlaneHandle,
    event_thread: Option<std::thread::JoinHandle<()>>,
    shutdown_requested: Arc<AtomicBool>,
}

impl SessionDataPlane {
    pub fn attach(
        runtime: DataPlaneRuntime,
        session_id: String,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
    ) -> Result<Self, TransportError> {
        let handle = runtime.handle.clone();
        let subscription = handle.subscribe()?;
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let event_shutdown_requested = shutdown_requested.clone();
        let event_thread = std::thread::Builder::new()
            .name(format!("session-data-{session_id}"))
            .spawn(move || loop {
                match subscription.recv() {
                    Ok(DataPlaneEvent::Data(data)) => on_data(session_id.clone(), data),
                    Ok(DataPlaneEvent::Closed(info)) => {
                        on_disconnect(session_id.clone(), info.into());
                        break;
                    }
                    Err(_) => {
                        // Subscription loss is expected after an explicit owner shutdown. Any
                        // other loss means the transport actor vanished without publishing its
                        // normal Closed event (for example because a driver panicked). Classify
                        // that from the owner's shutdown intent rather than from the transport's
                        // connected flag: the runtime now clears that flag on every exit path,
                        // including panics.
                        if !event_shutdown_requested.load(Ordering::Acquire) {
                            on_disconnect(
                                session_id.clone(),
                                DisconnectInfo::io_error("transport runtime stopped unexpectedly"),
                            );
                        }
                        break;
                    }
                }
            })
            .map_err(|error| TransportError::io("session_data_pump", error))?;
        Ok(Self {
            runtime: Some(runtime),
            handle,
            event_thread: Some(event_thread),
            shutdown_requested,
        })
    }

    pub fn handle(&self) -> &DataPlaneHandle {
        &self.handle
    }

    /// Request resource shutdown without joining the receive callback thread. The request is
    /// idempotent so cleanup paths cannot repeatedly enqueue shutdown while the actor is already
    /// exiting.
    pub fn request_shutdown(&self) {
        if self.shutdown_requested.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.handle.shutdown();
    }

    fn join_runtime_actor(&mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            if let Some(thread) = runtime.take_thread() {
                if thread.thread().id() != std::thread::current().id() {
                    let _ = thread.join();
                }
            }
        }
    }

    /// Deterministic cleanup for callers that are outside the SessionStore lock.
    pub fn shutdown(&mut self) {
        self.request_shutdown();
        self.join_runtime_actor();
        if let Some(thread) = self.event_thread.take() {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for SessionDataPlane {
    fn drop(&mut self) {
        // SessionDataPlane is commonly dropped while SessionStore is locked. Never wait for the
        // event/callback thread here because that callback may need the same lock. The transport
        // actor itself does not call SessionStore, so waiting for that owner thread is safe and
        // ensures the physical driver has actually left its runtime before resources are discarded.
        self.request_shutdown();
        self.join_runtime_actor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{BlockingByteStream, ReadStatus};
    use std::sync::mpsc;
    use std::time::Duration;

    struct PanicAfterGate {
        panic_now: std::sync::Arc<AtomicBool>,
    }

    impl BlockingByteStream for PanicAfterGate {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            if self.panic_now.load(Ordering::Acquire) {
                panic!("intentional transport actor panic");
            }
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
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

    #[test]
    fn unexpected_transport_actor_exit_surfaces_disconnect_and_clears_connected_state() {
        let panic_now = std::sync::Arc::new(AtomicBool::new(false));
        let runtime = DataPlaneRuntime::spawn(Box::new(PanicAfterGate {
            panic_now: panic_now.clone(),
        }));
        let (disconnect_tx, disconnect_rx) = mpsc::channel();
        let mut owner = SessionDataPlane::attach(
            runtime,
            "test-session".into(),
            Box::new(|_, _| {}),
            Box::new(move |_, info| {
                let _ = disconnect_tx.send(info);
            }),
        )
        .unwrap();

        panic_now.store(true, Ordering::Release);
        let info = disconnect_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("unexpected actor exit should become a disconnect");
        assert_eq!(info.reason, "transport runtime stopped unexpectedly");
        assert!(!owner.handle().is_connected());

        owner.shutdown();
    }
}
