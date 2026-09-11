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
}

impl SessionDataPlane {
    pub fn attach(
        runtime: DataPlaneRuntime,
        session_id: String,
        on_data: Arc<dyn Fn(String, Vec<u8>) + Send + Sync>,
        on_disconnect: Arc<dyn Fn(String, DisconnectInfo) + Send + Sync>,
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

    pub fn shutdown(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.join();
        } else {
            let _ = self.handle.shutdown();
        }
        if let Some(thread) = self.event_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for SessionDataPlane {
    fn drop(&mut self) {
        self.shutdown();
    }
}
