//! VirtualPortManager — Windows com0com 虚拟串口端口对管理。
//!
//! 资源模型只有两层真相：
//! - `active_endpoints`：当前进程仍有 Session 持有的端口对；
//! - `com0com_state.json`：TauTerm 创建且仍应负责回收的端口对（包含 active + orphan）。
//!
//! 因此可回收 orphan 的定义是“owned、当前 backend 非 active、且没有其它仍存活
//! TauTerm owner”。驱动中的其他 com0com bus 只用于冲突/身份核验，绝不能仅凭
//! bus 编号被当作 TauTerm 残留资源删除。

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::backend::{
    register_internal_endpoint_path, unregister_internal_endpoint_path, VirtualEndpoint,
    VirtualPortBackend, VirtualPortConfig, VirtualPortError,
};

use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_INVALID_PARAMETER, HANDLE,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, ReleaseMutex, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x0000_0080;
const WAIT_TIMEOUT: u32 = 258;
const MUTATION_LOCK_TIMEOUT_MS: u32 = 120_000;

const SETUPC_TIMEOUT_SECS: u64 = 30;
const COM_PORT_SCAN_START: u32 = 20;
const MAX_COM_PORT: u32 = 256;
const CANDIDATE_MULTIPLIER: u32 = 8;
const DESTROY_RETRY_COUNT: u32 = 3;
const DESTROY_RETRY_DELAY_MS: u64 = 200;

// scripts/test-serial-session.py 专用预留区。产品分配端口/bus 必须避开。
pub(crate) const RESERVED_PORT_BASE: u32 = 200;
pub(crate) const RESERVED_PORT_END: u32 = 255;
pub(crate) const RESERVED_BUS_BASE: u32 = 200;
pub(crate) const RESERVED_BUS_END: u32 = 255;

fn is_reserved_bus(bus: u32) -> bool {
    (RESERVED_BUS_BASE..=RESERVED_BUS_END).contains(&bus)
}

fn is_reserved_port(port: u32) -> bool {
    (RESERVED_PORT_BASE..=RESERVED_PORT_END).contains(&port)
}

const OWNERSHIP_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnedEndpointRecord {
    endpoint: VirtualEndpoint,
    owner_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    schema_version: u32,
    owned_endpoints: Vec<OwnedEndpointRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagementMode {
    Privileged,
    DirectUac,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DriverEndpointIdentity {
    bridge_path: Option<String>,
    external_path: Option<String>,
}

impl DriverEndpointIdentity {
    fn matches(&self, endpoint: &VirtualEndpoint) -> bool {
        self.bridge_path.as_deref() == Some(endpoint.bridge_path.as_str())
            && self.external_path.as_deref() == Some(endpoint.external_path.as_str())
    }
}

#[derive(Debug, Default)]
struct DriverState {
    occupied_ports: HashSet<u32>,
    buses: HashSet<u32>,
    actual_buses: HashSet<u32>,
    endpoints_by_bus: HashMap<u32, DriverEndpointIdentity>,
    max_bus: Option<u32>,
    queried: bool,
}

pub struct VirtualPortManager {
    active_endpoints: HashSet<VirtualEndpoint>,
    resource_dir: PathBuf,
    state_dir: PathBuf,
    mode: ManagementMode,
    owner_pid: Option<u32>,
}

fn normalize_windows_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn run_setupc(resource_dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let setupc = resource_dir.join("setupc.exe");
    if !setupc.exists() {
        return Err(format!("setupc.exe not found: {:?}", setupc));
    }

    let mut command = Command::new(&setupc);
    command
        .current_dir(resource_dir)
        .arg("--silent")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    let child = command
        .spawn()
        .map_err(|error| format!("Failed to spawn setupc.exe: {error}"))?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(std::time::Duration::from_secs(SETUPC_TIMEOUT_SECS)) {
        Ok(result) => result.map_err(|error| format!("setupc.exe execution failed: {error}")),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            log::warn!(
                "setupc.exe (PID {}) timed out after {}s; terminating it",
                pid,
                SETUPC_TIMEOUT_SECS
            );
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            Err("setupc.exe execution timed out".into())
        }
        Err(_) => Err("setupc.exe process exited abnormally".into()),
    }
}

fn output_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

struct DriverMutationGuard {
    handle: HANDLE,
}

impl DriverMutationGuard {
    fn acquire() -> Result<Self, String> {
        let name = wide(r"Global\TauTermCom0comMutation");
        let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)");
        let mut descriptor: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut descriptor_size = 0u32;
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                &mut descriptor_size,
            )
        };
        if converted == 0 || descriptor.is_null() {
            return Err(format!(
                "failed to build com0com mutation mutex DACL (Win32 {})",
                unsafe { GetLastError() }
            ));
        }

        let security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = unsafe { CreateMutexW(&security, 0, name.as_ptr()) };
        let create_error = handle.is_null().then(|| unsafe { GetLastError() });
        unsafe {
            let _ = LocalFree(descriptor);
        }
        if let Some(error) = create_error {
            return Err(format!(
                "failed to create/open com0com mutation mutex (Win32 {error})"
            ));
        }
        let wait = unsafe { WaitForSingleObject(handle, MUTATION_LOCK_TIMEOUT_MS) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe { CloseHandle(handle) };
            if wait == WAIT_TIMEOUT {
                return Err("timed out waiting for com0com mutation lock".into());
            }
            return Err(format!("failed waiting for com0com mutation lock ({wait})"));
        }
        Ok(Self { handle })
    }
}

impl Drop for DriverMutationGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}

fn process_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if !handle.is_null() {
        unsafe { CloseHandle(handle) };
        return true;
    }
    // Access denied/other lookup failures are treated as "possibly alive" so cleanup fails safe.
    (unsafe { GetLastError() }) != ERROR_INVALID_PARAMETER
}

impl VirtualPortManager {
    fn build(
        resource_dir: PathBuf,
        state_dir: PathBuf,
        mode: ManagementMode,
        owner_pid: Option<u32>,
    ) -> Self {
        let manager = Self {
            active_endpoints: HashSet::new(),
            resource_dir: normalize_windows_path(&resource_dir),
            state_dir: normalize_windows_path(&state_dir),
            mode,
            owner_pid,
        };
        for endpoint in manager.load_owned_endpoints() {
            register_internal_endpoint_path(&endpoint.bridge_path);
        }
        manager
    }

    pub fn new_privileged(resource_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self::build(resource_dir, state_dir, ManagementMode::Privileged, None)
    }

    pub fn new_direct_uac(resource_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self::build(
            resource_dir,
            state_dir,
            ManagementMode::DirectUac,
            Some(std::process::id()),
        )
    }

    pub fn new_privileged_for_owner(
        resource_dir: PathBuf,
        state_dir: PathBuf,
        owner_pid: u32,
    ) -> Self {
        Self::build(
            resource_dir,
            state_dir,
            ManagementMode::Privileged,
            Some(owner_pid),
        )
    }

    pub fn resource_dir(&self) -> &PathBuf {
        &self.resource_dir
    }

    pub fn setupc_path(&self) -> PathBuf {
        self.resource_dir.join("setupc.exe")
    }

    /// 仅统计“TauTerm 拥有但当前进程没有活跃 owner”的端口对。
    pub fn pending_orphan_count(&self) -> u32 {
        self.orphan_endpoints().len() as u32
    }

    pub fn are_files_present(&self) -> bool {
        self.setupc_path().exists()
            && self.resource_dir.join("setup.dll").exists()
            && self.resource_dir.join("com0com.sys").exists()
            && self.resource_dir.join("com0com.inf").exists()
            && self.resource_dir.join("com0com.cat").exists()
            && self.resource_dir.join("cncport.inf").exists()
            && self.resource_dir.join("comport.inf").exists()
    }

    /// 驱动安装状态是 SCM 事实；普通 GUI 不为探测启动 setupc.exe。
    pub fn detect_driver(&self) -> bool {
        super::windows_driver::is_com0com_driver_installed()
    }

    fn state_path(&self) -> PathBuf {
        self.state_dir.join("com0com_state.json")
    }

    fn try_load_owned_records(&self) -> Result<Vec<OwnedEndpointRecord>, String> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read virtual-port ownership state: {error}"))?;
        match serde_json::from_str::<PersistedState>(&content) {
            Ok(mut state) if state.schema_version == OWNERSHIP_SCHEMA_VERSION => {
                state
                    .owned_endpoints
                    .sort_by_key(|record| record.endpoint.resource_id);
                state
                    .owned_endpoints
                    .dedup_by_key(|record| record.endpoint.resource_id);
                Ok(state.owned_endpoints)
            }
            Ok(state) => {
                let reason = format!(
                    "unsupported schema version {} (expected {})",
                    state.schema_version, OWNERSHIP_SCHEMA_VERSION
                );
                self.reset_obsolete_state(&path, &reason)?;
                Ok(Vec::new())
            }
            Err(error) => {
                self.reset_obsolete_state(&path, &error.to_string())?;
                Ok(Vec::new())
            }
        }
    }

    fn load_owned_records(&self) -> Vec<OwnedEndpointRecord> {
        match self.try_load_owned_records() {
            Ok(records) => records,
            Err(error) => {
                log::warn!("virtual-port ownership state unavailable: {error}");
                Vec::new()
            }
        }
    }

    fn reset_obsolete_state(&self, path: &Path, reason: &str) -> Result<(), String> {
        if self.mode == ManagementMode::DirectUac {
            return Err(format!(
                "virtual-port ownership state requires privileged repair ({reason})"
            ));
        }
        let _mutation = DriverMutationGuard::acquire()?;
        let backup = path.with_extension(format!(
            "json.{}.bak",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        std::fs::copy(path, &backup).map_err(|error| {
            format!(
                "failed to back up obsolete virtual-port ownership state to {}: {error}",
                backup.display()
            )
        })?;
        log::warn!(
            "virtual-port ownership state has obsolete/corrupt schema ({reason}); backed up to {:?} and reset",
            backup
        );
        self.persist_owned_records(&[])
    }

    fn load_owned_endpoints(&self) -> Vec<VirtualEndpoint> {
        self.load_owned_records()
            .into_iter()
            .map(|record| record.endpoint)
            .collect()
    }

    fn persist_owned_records(&self, records: &[OwnedEndpointRecord]) -> Result<(), String> {
        if self.mode == ManagementMode::DirectUac {
            return Err(
                "refusing to write protected virtual-port ownership from direct GUI mode".into(),
            );
        }
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("failed to create virtual-port state directory: {error}")
            })?;
        }

        let mut owned = records.to_vec();
        owned.sort_by_key(|record| record.endpoint.resource_id);
        owned.dedup_by_key(|record| record.endpoint.resource_id);
        let state = PersistedState {
            schema_version: OWNERSHIP_SCHEMA_VERSION,
            owned_endpoints: owned,
        };
        let json = serde_json::to_string(&state)
            .map_err(|error| format!("failed to serialize virtual-port ownership state: {error}"))?;

        let mut file = atomic_write_file::AtomicWriteFile::open(&path)
            .map_err(|error| {
                format!("failed to open virtual-port ownership state for atomic write: {error}")
            })?;
        file.write_all(json.as_bytes())
            .map_err(|error| format!("failed to write virtual-port ownership state: {error}"))?;
        file.commit()
            .map_err(|error| format!("failed to atomically commit virtual-port ownership state: {error}"))
    }

    fn remember_owned_endpoints_with_owner(
        &mut self,
        endpoints: &[VirtualEndpoint],
        owner_pid: Option<u32>,
    ) -> Result<(), String> {
        if endpoints.is_empty() {
            return Ok(());
        }
        let mut owned = self.try_load_owned_records()?;
        for endpoint in endpoints {
            owned.retain(|existing| existing.endpoint.resource_id != endpoint.resource_id);
            owned.push(OwnedEndpointRecord {
                endpoint: endpoint.clone(),
                owner_pid,
            });
        }
        self.persist_owned_records(&owned)?;
        for endpoint in endpoints {
            register_internal_endpoint_path(&endpoint.bridge_path);
        }
        Ok(())
    }

    fn remember_owned_endpoints(
        &mut self,
        endpoints: &[VirtualEndpoint],
    ) -> Result<(), String> {
        self.remember_owned_endpoints_with_owner(endpoints, self.owner_pid)
    }

    fn track_active_endpoint_with_owner(
        &mut self,
        endpoint: VirtualEndpoint,
        owner_pid: Option<u32>,
    ) -> Result<(), String> {
        if self.mode == ManagementMode::Privileged {
            self.remember_owned_endpoints_with_owner(
                std::slice::from_ref(&endpoint),
                owner_pid,
            )?;
        } else {
            register_internal_endpoint_path(&endpoint.bridge_path);
        }

        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        self.active_endpoints.insert(endpoint);
        Ok(())
    }

    fn track_active_endpoint(&mut self, endpoint: VirtualEndpoint) -> Result<(), String> {
        self.track_active_endpoint_with_owner(endpoint, self.owner_pid)
    }

    fn adopt_active_endpoints(&mut self, endpoints: &[VirtualEndpoint]) {
        for endpoint in endpoints {
            self.active_endpoints
                .retain(|existing| existing.resource_id != endpoint.resource_id);
            self.active_endpoints.insert(endpoint.clone());
            register_internal_endpoint_path(&endpoint.bridge_path);
        }
    }

    fn forget_owned_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);

        if self.mode == ManagementMode::DirectUac {
            // The direct GUI has read-only access to machine ownership state. The elevated helper
            // already committed/removed the protected record; the GUI only updates local hiding.
            unregister_internal_endpoint_path(&endpoint.bridge_path);
            return Ok(());
        }

        let mut owned = self.try_load_owned_records()?;
        let removed_paths = owned
            .iter()
            .filter(|existing| existing.endpoint.resource_id == endpoint.resource_id)
            .map(|existing| existing.endpoint.bridge_path.clone())
            .collect::<Vec<_>>();
        owned.retain(|existing| existing.endpoint.resource_id != endpoint.resource_id);
        self.persist_owned_records(&owned)?;

        if removed_paths.is_empty() {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        } else {
            for path in removed_paths {
                unregister_internal_endpoint_path(&path);
            }
        }
        Ok(())
    }

    fn defer_cleanup(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        if self.mode == ManagementMode::Privileged {
            self.remember_owned_endpoints(std::slice::from_ref(endpoint))?;
        }
        Ok(())
    }

    fn record_is_reclaimable(&self, record: &OwnedEndpointRecord) -> bool {
        if self
            .active_endpoints
            .iter()
            .any(|active| active.resource_id == record.endpoint.resource_id)
        {
            return false;
        }
        !matches!(
            record.owner_pid,
            Some(pid) if Some(pid) != self.owner_pid && process_is_running(pid)
        )
    }

    fn orphan_endpoints(&self) -> Vec<VirtualEndpoint> {
        self.load_owned_records()
            .into_iter()
            .filter(|record| self.record_is_reclaimable(record))
            .map(|record| record.endpoint)
            .collect()
    }

    pub(crate) fn is_reclaimable_owned_endpoint(&self, endpoint: &VirtualEndpoint) -> bool {
        self.load_owned_records()
            .into_iter()
            .any(|record| record.endpoint == *endpoint && self.record_is_reclaimable(&record))
    }

    /// 特权上下文中的完整驱动状态查询。只由 TauTermService/管理员直接路径调用。
    fn query_driver_state(&self) -> DriverState {
        let mut state = DriverState::default();
        match run_setupc(&self.resource_dir, &["list"]) {
            Ok(output) if output.status.success() => {
                state.queried = true;
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    let parsed = ["CNCA", "CNCB"].into_iter().find_map(|prefix| {
                        let rest = trimmed.strip_prefix(prefix)?;
                        let bus = rest.split_whitespace().next()?.parse::<u32>().ok()?;
                        Some((prefix, bus))
                    });

                    if let Some((prefix, bus)) = parsed {
                        if !is_reserved_bus(bus) {
                            state.buses.insert(bus);
                            state.actual_buses.insert(bus);
                            state.max_bus =
                                Some(state.max_bus.map_or(bus, |current| current.max(bus)));

                            let port_name = trimmed
                                .split("PortName=")
                                .nth(1)
                                .and_then(|value| value.split(',').next())
                                .map(str::trim)
                                .filter(|value| value.starts_with("COM"))
                                .map(str::to_owned);

                            let identity = state.endpoints_by_bus.entry(bus).or_default();
                            if prefix == "CNCA" {
                                identity.bridge_path = port_name.clone();
                            } else {
                                identity.external_path = port_name.clone();
                            }

                            if let Some(name) = port_name {
                                if let Some(number) = name
                                    .strip_prefix("COM")
                                    .and_then(|number| number.parse::<u32>().ok())
                                {
                                    state.occupied_ports.insert(number);
                                }
                            }
                        }
                    }
                }
            }
            Ok(output) => {
                log::warn!(
                    "setupc list returned {:?}: {}",
                    output.status.code(),
                    output_detail(&output)
                );
            }
            Err(error) => log::warn!("setupc list failed: {error}"),
        }
        state
    }

    fn augment_busy_com_names(&self, state: &mut DriverState) {
        match run_setupc(&self.resource_dir, &["busynames", "COM?*"]) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for token in stdout.split(|ch: char| !ch.is_ascii_alphanumeric()) {
                    let upper = token.to_ascii_uppercase();
                    if let Some(number) = upper
                        .strip_prefix("COM")
                        .and_then(|number| number.parse::<u32>().ok())
                    {
                        state.occupied_ports.insert(number);
                    }
                }
            }
            Ok(output) => {
                log::warn!(
                    "setupc busynames returned {:?}: {}",
                    output.status.code(),
                    output_detail(&output)
                );
            }
            Err(error) => log::warn!("setupc busynames failed: {error}"),
        }
    }

    fn resolve_installed_bus(&self, bridge_path: &str, external_path: &str) -> Option<u32> {
        let driver = self.query_driver_state();
        if !driver.queried {
            return None;
        }
        driver.endpoints_by_bus.iter().find_map(|(bus, identity)| {
            (identity.bridge_path.as_deref() == Some(bridge_path)
                && identity.external_path.as_deref() == Some(external_path))
            .then_some(*bus)
        })
    }

    fn reconcile_owned_state(&mut self) -> Result<(), String> {
        let driver = self.query_driver_state();
        if !driver.queried {
            return Err("cannot enumerate com0com state while reconciling ownership".into());
        }

        let owned = self
            .try_load_owned_records()?
            .into_iter()
            .map(|record| record.endpoint)
            .collect::<Vec<_>>();
        for endpoint in owned {
            let exact = driver
                .endpoints_by_bus
                .get(&endpoint.resource_id)
                .is_some_and(|identity| identity.matches(&endpoint));
            if !exact {
                log::warn!(
                    "Forgetting stale virtual-port ownership {} ↔ {} (bus {}): driver identity is missing or changed",
                    endpoint.bridge_path,
                    endpoint.external_path,
                    endpoint.resource_id
                );
                self.forget_owned_endpoint(&endpoint)?;
            }
        }
        Ok(())
    }

    fn rollback_verified_endpoint(&mut self, endpoint: &VirtualEndpoint) {
        if let Err(error) = self.destroy_endpoint_privileged(endpoint) {
            log::warn!(
                "Failed to roll back verified virtual endpoint {} ↔ {} (bus {}): {}",
                endpoint.bridge_path,
                endpoint.external_path,
                endpoint.resource_id,
                error
            );
        }
    }

    fn rollback_batch(&mut self, endpoints: &[VirtualEndpoint]) {
        for endpoint in endpoints.iter().rev() {
            self.rollback_verified_endpoint(endpoint);
        }
    }

    fn next_free_bus(&self, driver: &DriverState) -> u32 {
        let mut bus = driver.max_bus.map_or(0, |max| max.saturating_add(1));
        while is_reserved_bus(bus) || driver.actual_buses.contains(&bus) {
            bus = bus.saturating_add(1);
        }
        bus
    }

    fn install_driver_privileged(&mut self) -> Result<(), String> {
        let _mutation = DriverMutationGuard::acquire()?;
        if self.detect_driver() {
            return Ok(());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        let driver = self.query_driver_state();
        let bus = self.next_free_bus(&driver);
        let output = run_setupc(&self.resource_dir, &["install", &bus.to_string(), "-", "-"])?;
        if !output.status.success() {
            return Err(format!(
                "com0com driver install failed (exit {:?}): {}",
                output.status.code(),
                output_detail(&output)
            ));
        }
        let _ = run_setupc(&self.resource_dir, &["remove", &bus.to_string()]);
        Ok(())
    }

    pub fn install_driver(&mut self) -> Result<(), String> {
        match self.mode {
            ManagementMode::Privileged => self.install_driver_privileged(),
            ManagementMode::DirectUac => super::elevated::ensure_driver(&self.resource_dir),
        }
    }

    pub fn ensure_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, VirtualPortError> {
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        if !self.are_files_present() {
            return Err(VirtualPortError::FilesMissing);
        }

        match self.mode {
            ManagementMode::DirectUac => {
                let cleanup = self.orphan_endpoints();
                let result = super::elevated::ensure_endpoints(
                    &self.resource_dir,
                    config.count.clamp(1, 4),
                    cleanup,
                )
                .map_err(VirtualPortError::from_backend)?;
                for endpoint in &result.cleaned_endpoints {
                    self.forget_owned_endpoint(endpoint)
                        .map_err(VirtualPortError::from_backend)?;
                }
                self.adopt_active_endpoints(&result.endpoints);
                Ok(result.endpoints)
            }
            ManagementMode::Privileged => {
                if !self.detect_driver() {
                    self.install_driver_privileged()
                        .map_err(VirtualPortError::from_backend)?;
                    if !self.detect_driver() {
                        return Err(VirtualPortError::DriverMissing);
                    }
                }
                self.create_endpoints_privileged(config, self.owner_pid)
                    .map_err(VirtualPortError::from_backend)
            }
        }
    }

    /// Re-adopt protected endpoints for a verified live GUI after TauTermService restarts.
    ///
    /// This is intentionally read-only with respect to the ownership ledger: the records already
    /// carry the authenticated GUI PID. Re-adoption only rebuilds the service's in-memory active
    /// set and client map so a later remove/cleanup request targets the same proven resources.
    pub fn adopt_owned_endpoints_for_owner(
        &mut self,
        owner_pid: u32,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        if self.mode != ManagementMode::Privileged {
            return Err("endpoint re-adoption requires a privileged backend".into());
        }
        self.reconcile_owned_state()?;
        let endpoints = self
            .try_load_owned_records()?
            .into_iter()
            .filter(|record| record.owner_pid == Some(owner_pid))
            .map(|record| record.endpoint)
            .collect::<Vec<_>>();
        for endpoint in &endpoints {
            self.active_endpoints
                .retain(|active| active.resource_id != endpoint.resource_id);
            self.active_endpoints.insert(endpoint.clone());
            register_internal_endpoint_path(&endpoint.bridge_path);
        }
        Ok(endpoints)
    }

    /// Privileged service entry point that attributes newly created endpoints to the
    /// authenticated GUI process rather than to the service process itself. This lets a restarted
    /// service distinguish a live TauTerm client from a true orphan.
    pub fn ensure_endpoints_for_owner(
        &mut self,
        config: &VirtualPortConfig,
        owner_pid: u32,
    ) -> Result<Vec<VirtualEndpoint>, VirtualPortError> {
        if self.mode != ManagementMode::Privileged {
            return Err(VirtualPortError::Backend(
                "owner-aware endpoint creation requires a privileged backend".into(),
            ));
        }
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        if !self.are_files_present() {
            return Err(VirtualPortError::FilesMissing);
        }
        if !self.detect_driver() {
            self.install_driver_privileged()
                .map_err(VirtualPortError::from_backend)?;
            if !self.detect_driver() {
                return Err(VirtualPortError::DriverMissing);
            }
        }
        self.create_endpoints_privileged(config, Some(owner_pid))
            .map_err(VirtualPortError::from_backend)
    }

    /// 扫描空闲连续 COM 号。extra_occupied 来自 com0com 驱动自身或 TauTerm ownership。
    pub fn find_available_port_pairs(count: u32, extra_occupied: &HashSet<u32>) -> Vec<(u32, u32)> {
        let mut in_use = serialport::available_ports()
            .map(|ports| {
                ports
                    .iter()
                    .filter_map(|port| {
                        port.port_name
                            .strip_prefix("COM")
                            .and_then(|number| number.parse::<u32>().ok())
                    })
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_else(|error| {
                log::warn!("Failed to enumerate system COM ports: {error}");
                HashSet::new()
            });
        in_use.extend(extra_occupied.iter().copied());

        let max_in_use = in_use.iter().max().copied().unwrap_or(0);
        let mut candidate = (max_in_use.saturating_add(2)).max(COM_PORT_SCAN_START);
        let mut wrapped = false;
        let mut pairs = Vec::new();

        while pairs.len() < count as usize {
            if candidate.saturating_add(1) >= MAX_COM_PORT {
                if wrapped {
                    break;
                }
                wrapped = true;
                candidate = COM_PORT_SCAN_START;
                continue;
            }
            if is_reserved_port(candidate) || is_reserved_port(candidate + 1) {
                candidate = RESERVED_PORT_END.saturating_add(1);
                continue;
            }
            if !in_use.contains(&candidate) && !in_use.contains(&(candidate + 1)) {
                pairs.push((candidate, candidate + 1));
                in_use.insert(candidate);
                in_use.insert(candidate + 1);
            }
            candidate = candidate.saturating_add(2);
        }
        pairs
    }

    /// 已提权上下文（TauTermService）使用的直接创建路径。
    fn create_endpoints_privileged(
        &mut self,
        config: &VirtualPortConfig,
        owner_pid: Option<u32>,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }
        let _mutation = DriverMutationGuard::acquire()?;

        let count = config.count.clamp(1, 4);
        self.reconcile_owned_state()?;

        // Fail before touching the driver if the protected ledger cannot be durably replaced.
        let ownership_snapshot = self.try_load_owned_records()?;
        self.persist_owned_records(&ownership_snapshot)?;

        let mut initial_driver = self.query_driver_state();
        if !initial_driver.queried {
            return Err("cannot enumerate com0com driver state before endpoint allocation".into());
        }
        self.augment_busy_com_names(&mut initial_driver);
        let candidates = Self::find_available_port_pairs(
            count.saturating_mul(CANDIDATE_MULTIPLIER),
            &initial_driver.occupied_ports,
        );
        if candidates.is_empty() {
            return Err("No available COM port pairs — all port numbers are in use".into());
        }

        let mut pairs = Vec::new();
        let mut skipped = Vec::new();

        for (bridge_number, external_number) in candidates {
            if pairs.len() >= count as usize {
                break;
            }

            let before = self.query_driver_state();
            if !before.queried {
                self.rollback_batch(&pairs);
                return Err("cannot refresh com0com driver state during endpoint allocation".into());
            }
            let bus = self.next_free_bus(&before);
            let endpoint = VirtualEndpoint {
                bridge_path: format!("COM{bridge_number}"),
                external_path: format!("COM{external_number}"),
                resource_id: bus,
            };

            // DSR=ropen 是 bridge 的 peer-presence 信号。外部端未打开时 bridge 不向
            // com0com 写历史 backlog；外部端打开后才开始透明转发。
            let bridge_arg = format!("PortName={},dsr=ropen", endpoint.bridge_path);
            let external_arg = format!("PortName={},PlugInMode=yes", endpoint.external_path);
            let bus_arg = bus.to_string();
            let install = run_setupc(
                &self.resource_dir,
                &["install", &bus_arg, &bridge_arg, &external_arg],
            );

            match install {
                Ok(output) if output.status.success() => {
                    let Some(actual_bus) =
                        self.resolve_installed_bus(&endpoint.bridge_path, &endpoint.external_path)
                    else {
                        self.rollback_batch(&pairs);
                        return Err(format!(
                            "setupc reported success but no exact post-install mapping exists for {} ↔ {}; refusing to guess ownership",
                            endpoint.bridge_path, endpoint.external_path
                        ));
                    };
                    if before.actual_buses.contains(&actual_bus) {
                        self.rollback_batch(&pairs);
                        return Err(format!(
                            "post-install mapping {} ↔ {} resolved to pre-existing bus {}; refusing to claim it",
                            endpoint.bridge_path, endpoint.external_path, actual_bus
                        ));
                    }

                    let mut endpoint = endpoint;
                    endpoint.resource_id = actual_bus;
                    log::info!(
                        "Virtual port pair created: {} ↔ {} (bus {})",
                        endpoint.bridge_path,
                        endpoint.external_path,
                        endpoint.resource_id
                    );
                    if let Err(error) =
                        self.track_active_endpoint_with_owner(endpoint.clone(), owner_pid)
                    {
                        self.rollback_verified_endpoint(&endpoint);
                        self.rollback_batch(&pairs);
                        return Err(format!(
                            "virtual-port ownership commit failed after creating bus {}: {}",
                            endpoint.resource_id, error
                        ));
                    }
                    pairs.push(endpoint);
                }
                Ok(output) => {
                    // A non-zero setupc can still have partially created a pair. Only clean it if
                    // the authoritative post-state proves the exact requested COM mapping; never
                    // remove a merely "new" bus because a third-party setupc process could race us.
                    if let Some(actual_bus) =
                        self.resolve_installed_bus(&endpoint.bridge_path, &endpoint.external_path)
                    {
                        if !before.actual_buses.contains(&actual_bus) {
                            let partial = VirtualEndpoint {
                                resource_id: actual_bus,
                                ..endpoint.clone()
                            };
                            if let Err(error) =
                                self.track_active_endpoint_with_owner(partial.clone(), owner_pid)
                            {
                                self.rollback_verified_endpoint(&partial);
                                self.rollback_batch(&pairs);
                                return Err(format!(
                                    "failed to persist ownership for partially created bus {}: {}",
                                    partial.resource_id, error
                                ));
                            }
                            self.rollback_verified_endpoint(&partial);
                        }
                    }

                    let detail = output_detail(&output);
                    let lower = detail.to_lowercase();
                    if lower.contains("in use")
                        || lower.contains("already logged")
                        || lower.contains("already exists")
                    {
                        skipped.push(format!(
                            "{} / {}",
                            endpoint.bridge_path, endpoint.external_path
                        ));
                        continue;
                    }

                    self.rollback_batch(&pairs);
                    return Err(format!(
                        "Failed to create port pair {}↔{} (exit {:?}): {}",
                        endpoint.bridge_path,
                        endpoint.external_path,
                        output.status.code(),
                        detail
                    ));
                }
                Err(error) => {
                    self.rollback_batch(&pairs);
                    return Err(error);
                }
            }
        }

        if !skipped.is_empty() {
            log::warn!("Skipped occupied virtual port candidates: {skipped:?}");
        }
        if pairs.is_empty() {
            return Err("All candidate COM port pairs are occupied".into());
        }
        if pairs.len() < count as usize {
            self.rollback_batch(&pairs);
            return Err(format!(
                "Requested {count} virtual port pairs but only {} could be created transactionally",
                pairs.len()
            ));
        }
        Ok(pairs)
    }

    pub fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        if self.mode == ManagementMode::DirectUac {
            // Disconnect must stay non-interactive. Keep ownership and let the next explicit
            // create/cleanup action reclaim the endpoint through the one-shot helper.
            self.defer_cleanup(endpoint)?;
            return Ok(());
        }
        self.destroy_endpoint_privileged(endpoint)
    }

    fn destroy_endpoint_privileged(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        let _mutation = DriverMutationGuard::acquire()?;
        let bus = endpoint.resource_id.to_string();
        let mut last_error = None;

        for attempt in 0..DESTROY_RETRY_COUNT {
            let driver = self.query_driver_state();
            if !driver.queried {
                self.defer_cleanup(endpoint)?;
                return Err("cannot enumerate com0com state before endpoint removal".into());
            }

            match driver.endpoints_by_bus.get(&endpoint.resource_id) {
                None => {
                    self.forget_owned_endpoint(endpoint)?;
                    return Ok(());
                }
                Some(identity) if identity.matches(endpoint) => {}
                Some(identity) => {
                    log::warn!(
                        "Refusing to remove bus {} because its driver identity no longer matches ownership (expected {} ↔ {}, actual {:?} ↔ {:?})",
                        endpoint.resource_id,
                        endpoint.bridge_path,
                        endpoint.external_path,
                        identity.bridge_path,
                        identity.external_path
                    );
                    self.forget_owned_endpoint(endpoint)?;
                    return Ok(());
                }
            }

            match run_setupc(&self.resource_dir, &["remove", &bus]) {
                Ok(output) if output.status.success() => {
                    self.forget_owned_endpoint(endpoint)?;
                    log::info!(
                        "Virtual port pair destroyed: {} ↔ {}",
                        endpoint.bridge_path,
                        endpoint.external_path
                    );
                    return Ok(());
                }
                Ok(output) => {
                    last_error = Some(format!(
                        "setupc remove returned {:?}: {}",
                        output.status.code(),
                        output_detail(&output)
                    ));
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }

            if attempt + 1 < DESTROY_RETRY_COUNT {
                std::thread::sleep(std::time::Duration::from_millis(
                    DESTROY_RETRY_DELAY_MS,
                ));
            }
        }

        self.defer_cleanup(endpoint)?;
        let error = last_error.unwrap_or_else(|| {
            format!(
                "Virtual port pair {} ↔ {} (bus {}) requires deferred cleanup",
                endpoint.bridge_path, endpoint.external_path, endpoint.resource_id
            )
        });
        log::warn!(
            "Virtual port pair {} ↔ {} (bus {}) requires deferred cleanup: {}",
            endpoint.bridge_path,
            endpoint.external_path,
            endpoint.resource_id,
            error
        );
        Err(error)
    }

    pub fn cleanup_all(&mut self) {
        let active = self.active_endpoints.iter().cloned().collect::<Vec<_>>();
        for endpoint in active {
            let result = if self.mode == ManagementMode::DirectUac {
                self.defer_cleanup(&endpoint)
            } else {
                self.destroy_endpoint_privileged(&endpoint)
            };
            if let Err(error) = result {
                log::warn!(
                    "Failed to release virtual endpoint {} ↔ {}: {}",
                    endpoint.bridge_path,
                    endpoint.external_path,
                    error
                );
            }
        }

        let pending = self.pending_orphan_count();
        if pending > 0 {
            log::warn!(
                "{} virtual-port pair(s) remain pending for explicit cleanup",
                pending
            );
        }
    }

    pub fn cleanup_orphans(&mut self) -> Result<u32, String> {
        if self.mode == ManagementMode::Privileged {
            self.reconcile_owned_state()?;
        }
        let orphans = self.orphan_endpoints();
        if orphans.is_empty() {
            return Ok(0);
        }

        if self.mode == ManagementMode::DirectUac {
            let cleaned = super::elevated::cleanup_endpoints(&self.resource_dir, orphans)?;
            for endpoint in &cleaned {
                self.forget_owned_endpoint(endpoint)?;
            }
            return Ok(cleaned.len() as u32);
        }

        let total = orphans.len();
        let mut cleaned = 0u32;
        for endpoint in orphans {
            let resource_id = endpoint.resource_id;
            if self.destroy_endpoint_privileged(&endpoint).is_ok()
                && !self
                    .orphan_endpoints()
                    .iter()
                    .any(|candidate| candidate.resource_id == resource_id)
            {
                cleaned += 1;
            }
        }
        if cleaned > 0 {
            log::info!("Orphan virtual-port cleanup completed: {cleaned}/{total}");
        }
        Ok(cleaned)
    }
}

impl VirtualPortBackend for VirtualPortManager {
    fn are_files_present(&self) -> bool {
        VirtualPortManager::are_files_present(self)
    }

    fn detect_driver(&self) -> bool {
        VirtualPortManager::detect_driver(self)
    }

    fn install_driver(&mut self) -> Result<(), String> {
        VirtualPortManager::install_driver(self)
    }

    fn ensure_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, VirtualPortError> {
        VirtualPortManager::ensure_endpoints(self, config)
    }

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        VirtualPortManager::destroy_endpoint(self, endpoint)
    }

    fn cleanup_all(&mut self) {
        VirtualPortManager::cleanup_all(self)
    }

    fn cleanup_orphans(&mut self) -> Result<u32, String> {
        VirtualPortManager::cleanup_orphans(self)
    }

    fn pending_orphan_count(&self) -> u32 {
        VirtualPortManager::pending_orphan_count(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> (VirtualPortManager, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "tauterm-vport-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        (
            VirtualPortManager::new_privileged_for_owner(
                root.clone(),
                root.clone(),
                std::process::id(),
            ),
            root,
        )
    }

    fn sample_endpoint(bus: u32) -> VirtualEndpoint {
        VirtualEndpoint {
            bridge_path: format!("COM{}", 40 + bus * 2),
            external_path: format!("COM{}", 41 + bus * 2),
            resource_id: bus,
        }
    }

    #[test]
    fn active_owned_endpoint_is_not_orphan() {
        let (mut manager, root) = test_manager();
        let endpoint = sample_endpoint(1);
        manager.track_active_endpoint(endpoint.clone()).unwrap();
        assert_eq!(manager.pending_orphan_count(), 0);
        manager.defer_cleanup(&endpoint).unwrap();
        assert_eq!(manager.pending_orphan_count(), 1);
        manager.forget_owned_endpoint(&endpoint).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn current_schema_records_owner_pid() {
        let (mut manager, root) = test_manager();
        let endpoint = sample_endpoint(2);
        manager.track_active_endpoint(endpoint.clone()).unwrap();
        let raw = std::fs::read_to_string(manager.state_path()).unwrap();
        let state: PersistedState = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.schema_version, OWNERSHIP_SCHEMA_VERSION);
        assert_eq!(state.owned_endpoints.len(), 1);
        assert_eq!(state.owned_endpoints[0].owner_pid, Some(std::process::id()));
        manager.forget_owned_endpoint(&endpoint).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn obsolete_bus_only_state_is_reset_instead_of_trusted() {
        let (manager, root) = test_manager();
        std::fs::write(
            manager.state_path(),
            r#"{"owned_endpoints":[{"bridge_path":"COM20","external_path":"COM21","resource_id":0}]}"#,
        )
        .unwrap();
        assert!(manager.load_owned_records().is_empty());
        let raw = std::fs::read_to_string(manager.state_path()).unwrap();
        let state: PersistedState = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.schema_version, OWNERSHIP_SCHEMA_VERSION);
        assert!(state.owned_endpoints.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreign_live_owner_is_not_reclaimable() {
        let root = std::env::temp_dir().join(format!(
            "tauterm-vport-owner-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut writer = VirtualPortManager::new_privileged_for_owner(
            root.clone(),
            root.clone(),
            std::process::id(),
        );
        let endpoint = sample_endpoint(3);
        writer
            .remember_owned_endpoints(std::slice::from_ref(&endpoint))
            .unwrap();
        drop(writer);

        let reader = VirtualPortManager::build(
            root.clone(),
            root.clone(),
            ManagementMode::Privileged,
            Some(std::process::id().saturating_add(1)),
        );
        assert!(!reader.is_reclaimable_owned_endpoint(&endpoint));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forgetting_one_endpoint_preserves_other_ownership() {
        let (mut manager, root) = test_manager();
        let first = sample_endpoint(4);
        let second = sample_endpoint(5);
        manager.track_active_endpoint(first.clone()).unwrap();
        manager.track_active_endpoint(second.clone()).unwrap();
        manager.defer_cleanup(&first).unwrap();
        manager.defer_cleanup(&second).unwrap();
        manager.forget_owned_endpoint(&first).unwrap();

        let owned = manager.load_owned_endpoints();
        assert_eq!(owned, vec![second.clone()]);
        manager.forget_owned_endpoint(&second).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn driver_identity_requires_both_exact_com_paths() {
        let endpoint = sample_endpoint(6);
        let exact = DriverEndpointIdentity {
            bridge_path: Some(endpoint.bridge_path.clone()),
            external_path: Some(endpoint.external_path.clone()),
        };
        assert!(exact.matches(&endpoint));

        let reused_bus = DriverEndpointIdentity {
            bridge_path: Some(endpoint.bridge_path.clone()),
            external_path: Some("COM199".into()),
        };
        assert!(!reused_bus.matches(&endpoint));

        let incomplete = DriverEndpointIdentity {
            bridge_path: Some(endpoint.bridge_path.clone()),
            external_path: None,
        };
        assert!(!incomplete.matches(&endpoint));
    }

    #[test]
    fn reserved_region_is_never_allocated() {
        let occupied = (20..199).collect::<HashSet<_>>();
        let pairs = VirtualPortManager::find_available_port_pairs(2, &occupied);
        assert!(pairs
            .iter()
            .all(|(a, b)| { !is_reserved_port(*a) && !is_reserved_port(*b) }));
    }
}
