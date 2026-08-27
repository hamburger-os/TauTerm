//! Native PTY virtual serial backend for Linux/macOS.
//!
//! Each virtual endpoint is a single POSIX PTY pair created in-process with
//! `serialport::TTYPort::pair()`:
//!
//! ```text
//! physical serial <-> TauTerm <-> PTY master <-> PTY slave <-> external tool
//! ```
//!
//! TauTerm retains the PTY master and exposes only the slave path to external
//! applications. No helper process, PATH lookup, package-manager dependency,
//! world-writable symlink, or persistent kernel resource is required.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serialport::{SerialPort, TTYPort};

use super::backend::{VirtualEndpoint, VirtualPortBackend, VirtualPortConfig};

const MAX_ENDPOINT_COUNT: u32 = 4;
const PTY_READ_TIMEOUT_MS: u64 = 5;

static MASTER_REGISTRY: LazyLock<Mutex<HashMap<String, TTYPort>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Take ownership of the PTY master corresponding to an exposed slave path.
/// The bridge calls this exactly once when it starts.
pub(crate) fn take_master_for_slave(slave_path: &str) -> Option<Box<dyn SerialPort>> {
    MASTER_REGISTRY
        .lock()
        .ok()?
        .remove(slave_path)
        .map(|port| Box::new(port) as Box<dyn SerialPort>)
}

/// Native Unix PTY backend.
pub struct PtyBackend {
    active_paths: HashSet<String>,
    next_id: u32,
}

impl Default for PtyBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyBackend {
    pub fn new() -> Self {
        Self {
            active_paths: HashSet::new(),
            next_id: 0,
        }
    }

    fn create_endpoint(&mut self) -> Result<VirtualEndpoint, String> {
        let (mut master, mut slave) =
            TTYPort::pair().map_err(|e| format!("failed to create native PTY pair: {e}"))?;

        let _ = slave.set_exclusive(false);
        let _ = master.set_timeout(Duration::from_millis(PTY_READ_TIMEOUT_MS));

        let slave_path = slave
            .name()
            .ok_or_else(|| "native PTY slave has no device path".to_string())?;
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        MASTER_REGISTRY
            .lock()
            .map_err(|_| "native PTY registry lock poisoned".to_string())?
            .insert(slave_path.clone(), master);
        self.active_paths.insert(slave_path.clone());

        log::info!("native PTY endpoint created: {} (id={})", slave_path, id);

        Ok(VirtualEndpoint {
            bridge_path: slave_path.clone(),
            external_path: slave_path,
            resource_id: id,
        })
    }

    fn remove_endpoint(&mut self, path: &str) {
        if let Ok(mut registry) = MASTER_REGISTRY.lock() {
            registry.remove(path);
        }
        self.active_paths.remove(path);
    }
}

impl VirtualPortBackend for PtyBackend {
    fn are_files_present(&self) -> bool {
        true
    }

    fn detect_driver(&self) -> bool {
        true
    }

    fn install_driver(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn install_driver_elevated(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        let count = config.count.clamp(1, MAX_ENDPOINT_COUNT);
        let mut created = Vec::with_capacity(count as usize);

        for _ in 0..count {
            match self.create_endpoint() {
                Ok(endpoint) => created.push(endpoint),
                Err(error) => {
                    for endpoint in &created {
                        self.remove_endpoint(&endpoint.bridge_path);
                    }
                    return Err(error);
                }
            }
        }
        Ok(created)
    }

    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        self.create_endpoints(config)
    }

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        self.remove_endpoint(&endpoint.bridge_path);
        Ok(())
    }

    fn cleanup_all(&mut self) {
        let paths: Vec<String> = self.active_paths.iter().cloned().collect();
        for path in paths {
            self.remove_endpoint(&path);
        }
    }

    fn cleanup_orphans(&mut self) -> u32 {
        // PTYs are kernel objects tied to open file descriptors. Process exit closes
        // the master descriptors, so there is no persistent endpoint to clean up.
        0
    }

    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        self.cleanup_all();
        Ok(0)
    }

    fn pending_orphan_count(&self) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn native_pty_round_trip() {
        let mut backend = PtyBackend::new();
        let endpoints = backend
            .create_endpoints(&VirtualPortConfig {
                enabled: true,
                count: 1,
            })
            .expect("create PTY endpoint");
        let endpoint = &endpoints[0];

        let mut master = take_master_for_slave(&endpoint.bridge_path).expect("registered master");
        let mut slave = serialport::new(&endpoint.external_path, 0)
            .timeout(Duration::from_millis(100))
            .open()
            .expect("open slave");

        master.write_all(b"tau").expect("master write");
        let mut buf = [0u8; 3];
        slave.read_exact(&mut buf).expect("slave read");
        assert_eq!(&buf, b"tau");

        slave.write_all(b"pty").expect("slave write");
        master.read_exact(&mut buf).expect("master read");
        assert_eq!(&buf, b"pty");
    }
}
