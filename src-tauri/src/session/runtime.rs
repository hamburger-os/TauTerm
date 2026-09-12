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
        let event_thread = std::thread::Builder::new()
            .name(format!("session-data-{session_id}"))
            .spawn(move || loop {
                match subscription.recv() {
                    Ok(DataPlaneEvent::Data(data)) => on_data(session_id.clone(), data),
                    Ok(DataPlaneEvent::Closed(info)) => {
                        on_disconnect(session_id.clone(), info.into());
                        break;
                    }
                    Err(_) => break,
                }
            })
            .map_err(|error| TransportError::io("session_data_pump", error))?;
        Ok(Self {
            runtime: Some(runtime),
            handle,
            event_thread: Some(event_thread),
        })
    }

    pub fn handle(&self) -> &DataPlaneHandle {
        &self.handle
    }

    /// Request resource shutdown without joining callback threads. SessionStore uses this while its
    /// mutex is held so a disconnect callback can never deadlock waiting for the same store lock.
    pub fn request_shutdown(&self) {
        let _ = self.handle.shutdown();
    }

    /// Deterministic cleanup for callers that are outside the SessionStore lock.
    pub fn shutdown(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.join();
        } else {
            let _ = self.handle.shutdown();
        }
        if let Some(thread) = self.event_thread.take() {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for SessionDataPlane {
    fn drop(&mut self) {
        // Drop must never block: ActiveSessionHandle is commonly dropped while SessionStore is
        // locked, and the event pump may be inside a callback that needs that same lock.
        self.request_shutdown();
    }
}
