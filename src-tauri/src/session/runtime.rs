use std::sync::atomic::{AtomicBool, Ordering};

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
    shutdown_requested: AtomicBool,
}

impl SessionDataPlane {
    pub fn attach(
        runtime: DataPlaneRuntime,
        session_id: String,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
    ) -> Result<Self, TransportError> {
        let handle = runtime.handle.clone();
        let event_handle = handle.clone();
        let subscription = handle.subscribe()?;
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
                        // A normal requested shutdown marks the DataPlane disconnected before its
                        // subscriber senders disappear. If the subscription disappears while the
                        // handle still reports connected, the transport actor terminated
                        // unexpectedly (for example because a driver panicked). Surface that as a
                        // real disconnect instead of leaving the UI in a stale connected state.
                        if event_handle.is_connected() {
                            on_disconnect(
                                session_id.clone(),
                                DisconnectInfo::io_error(
                                    "transport runtime stopped unexpectedly",
                                ),
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
            shutdown_requested: AtomicBool::new(false),
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
