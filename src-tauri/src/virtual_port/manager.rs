//! VirtualPortManager — Windows com0com 虚拟串口端口对管理。
//!
//! 资源模型只有两层真相：
//! - `active_endpoints`：当前进程仍有 Session 持有的端口对；
//! - `com0com_state.json`：TauTerm 创建且仍应负责回收的端口对（包含 active + orphan）。
//!
//! 因此 orphan 的定义严格为 `owned - active`。驱动中的其他 com0com bus 只用于
//! 端口/bus 冲突检测，绝不能被当作 TauTerm 残留资源删除。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::backend::{
    register_internal_endpoint_path, unregister_internal_endpoint_path, VirtualEndpoint,
    VirtualPortBackend, VirtualPortConfig, VirtualPortError,
};

use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, HANDLE};
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
const CANDIDATE_MULTIPLIER: u32 = 2;
const DESTROY_STAGE2_RETRY_COUNT: u32 = 3;
const DESTROY_STAGE2_RETRY_DELAY_MS: u64 = 200;
const DESTROY_UNBIND_WAIT_MS: u64 = 300;

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

#[derive(Debug, Default)]
struct DriverState {
    occupied_ports: HashSet<u32>,
    buses: HashSet<u32>,
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
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "failed to create com0com mutation mutex (Win32 {})",
                unsafe { GetLastError() }
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

    fn load_owned_records(&self) -> Vec<OwnedEndpointRecord> {
        let path = self.state_path();
        if !path.exists() {
            return Vec::new();
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                log::warn!("Failed to read virtual-port ownership state: {error}");
                return Vec::new();
            }
        };
        match serde_json::from_str::<PersistedState>(&content) {
            Ok(mut state) if state.schema_version == OWNERSHIP_SCHEMA_VERSION => {
                state
                    .owned_endpoints
                    .sort_by_key(|record| record.endpoint.resource_id);
                state
                    .owned_endpoints
                    .dedup_by_key(|record| record.endpoint.resource_id);
                state.owned_endpoints
            }
            Ok(state) => {
                self.reset_obsolete_state(
                    &path,
                    &format!(
                        "unsupported schema version {} (expected {})",
                        state.schema_version, OWNERSHIP_SCHEMA_VERSION
                    ),
                );
                Vec::new()
            }
            Err(error) => {
                self.reset_obsolete_state(&path, &error.to_string());
                Vec::new()
            }
        }
    }

    fn reset_obsolete_state(&self, path: &Path, reason: &str) {
        if self.mode == ManagementMode::DirectUac {
            log::warn!(
                "virtual-port ownership state requires privileged repair ({reason}); GUI leaves the protected ledger untouched"
            );
            return;
        }
        let backup = path.with_extension(format!(
            "json.{}.bak",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        let _ = std::fs::copy(path, &backup);
        log::warn!(
            "virtual-port ownership state has obsolete/corrupt schema ({reason}); backed up to {:?} and reset",
            backup
        );
        self.persist_owned_records(&[]);
    }

    fn load_owned_endpoints(&self) -> Vec<VirtualEndpoint> {
        self.load_owned_records()
            .into_iter()
            .map(|record| record.endpoint)
            .collect()
    }

    fn persist_owned_records(&self, records: &[OwnedEndpointRecord]) {
        if self.mode == ManagementMode::DirectUac {
            log::error!("refusing to write protected virtual-port ownership from direct GUI mode");
            return;
        }
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create virtual-port state directory: {error}");
                return;
            }
        }

        let mut owned = records.to_vec();
        owned.sort_by_key(|record| record.endpoint.resource_id);
        owned.dedup_by_key(|record| record.endpoint.resource_id);
        let state = PersistedState {
            schema_version: OWNERSHIP_SCHEMA_VERSION,
            owned_endpoints: owned,
        };
        let json = match serde_json::to_string(&state) {
            Ok(json) => json,
            Err(error) => {
                log::error!("Failed to serialize virtual-port ownership state: {error}");
                return;
            }
        };

        let temporary = path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(&temporary, &json) {
            log::warn!("Failed to write virtual-port state temp file: {error}");
            return;
        }
        if let Err(error) = std::fs::rename(&temporary, &path) {
            log::warn!("Failed to atomically replace virtual-port state: {error}");
            let _ = std::fs::write(&path, json);
            let _ = std::fs::remove_file(&temporary);
        }
    }

    fn remember_owned_endpoints_with_owner(
        &mut self,
        endpoints: &[VirtualEndpoint],
        owner_pid: Option<u32>,
    ) {
        if endpoints.is_empty() {
            return;
        }
        let mut owned = self.load_owned_records();
        for endpoint in endpoints {
            register_internal_endpoint_path(&endpoint.bridge_path);
            owned.retain(|existing| existing.endpoint.resource_id != endpoint.resource_id);
            owned.push(OwnedEndpointRecord {
                endpoint: endpoint.clone(),
                owner_pid,
            });
        }
        self.persist_owned_records(&owned);
    }

    fn remember_owned_endpoints(&mut self, endpoints: &[VirtualEndpoint]) {
        self.remember_owned_endpoints_with_owner(endpoints, self.owner_pid);
    }

    fn track_active_endpoint_with_owner(
        &mut self,
        endpoint: VirtualEndpoint,
        owner_pid: Option<u32>,
    ) {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        self.active_endpoints.insert(endpoint.clone());
        register_internal_endpoint_path(&endpoint.bridge_path);
        if self.mode == ManagementMode::Privileged {
            self.remember_owned_endpoints_with_owner(std::slice::from_ref(&endpoint), owner_pid);
        }
    }

    fn track_active_endpoint(&mut self, endpoint: VirtualEndpoint) {
        self.track_active_endpoint_with_owner(endpoint, self.owner_pid);
    }

    fn adopt_active_endpoints(&mut self, endpoints: &[VirtualEndpoint]) {
        for endpoint in endpoints {
            self.track_active_endpoint(endpoint.clone());
        }
    }

    fn forget_owned_endpoint(&mut self, endpoint: &VirtualEndpoint) {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);

        if self.mode == ManagementMode::DirectUac {
            // The direct GUI has read-only access to machine ownership state. The elevated helper
            // already committed/removed the protected record; the GUI only updates local hiding.
            unregister_internal_endpoint_path(&endpoint.bridge_path);
            return;
        }

        let mut owned = self.load_owned_records();
        let removed_paths = owned
            .iter()
            .filter(|existing| existing.endpoint.resource_id == endpoint.resource_id)
            .map(|existing| existing.endpoint.bridge_path.clone())
            .collect::<Vec<_>>();
        owned.retain(|existing| existing.endpoint.resource_id != endpoint.resource_id);
        self.persist_owned_records(&owned);

        if removed_paths.is_empty() {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        } else {
            for path in removed_paths {
                unregister_internal_endpoint_path(&path);
            }
        }
    }

    fn defer_cleanup(&mut self, endpoint: &VirtualEndpoint) {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        if self.mode == ManagementMode::Privileged {
            self.remember_owned_endpoints(std::slice::from_ref(endpoint));
        }
    }

    fn record_is_reclaimable(&self, record: &OwnedEndpointRecord) -> bool {
        if self
            .active_endpoints
            .iter()
            .any(|active| active.resource_id == record.endpoint.resource_id)
        {
            return false;
        }
        match record.owner_pid {
            Some(pid) if Some(pid) != self.owner_pid && process_is_running(pid) => false,
            _ => true,
        }
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

    /// 不启动 setupc 的本地 ownership 投影，仅用于冲突避让与恢复判断。
    fn local_driver_state(&self) -> DriverState {
        let mut state = DriverState::default();
        for endpoint in self.load_owned_endpoints() {
            let bus = endpoint.resource_id;
            state.buses.insert(bus);
            state.max_bus = Some(state.max_bus.map_or(bus, |current| current.max(bus)));
            for path in [&endpoint.bridge_path, &endpoint.external_path] {
                if let Some(number) = path
                    .strip_prefix("COM")
                    .and_then(|number| number.parse::<u32>().ok())
                {
                    state.occupied_ports.insert(number);
                }
            }
        }
        state
    }

    /// 特权上下文中的完整驱动状态查询。只由 TauTermService/管理员直接路径调用。
    fn query_driver_state(&self) -> DriverState {
        let mut state = self.local_driver_state();
        match run_setupc(&self.resource_dir, &["list"]) {
            Ok(output) if output.status.success() => {
                state.queried = true;
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    for prefix in ["CNCA", "CNCB"] {
                        if let Some(rest) = trimmed.strip_prefix(prefix) {
                            if let Some(token) = rest.split_whitespace().next() {
                                if let Ok(bus) = token.parse::<u32>() {
                                    if !is_reserved_bus(bus) {
                                        state.buses.insert(bus);
                                        state.max_bus = Some(
                                            state.max_bus.map_or(bus, |current| current.max(bus)),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if let Some(port_part) = line.split("PortName=").nth(1) {
                        let name = port_part.split(',').next().unwrap_or("").trim();
                        if let Some(number) = name
                            .strip_prefix("COM")
                            .and_then(|number| number.parse::<u32>().ok())
                        {
                            state.occupied_ports.insert(number);
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

    fn resolve_installed_bus(&self, bridge_path: &str, external_path: &str) -> Option<u32> {
        let output = run_setupc(&self.resource_dir, &["list"]).ok()?;
        if !output.status.success() {
            return None;
        }
        let mut bridge_bus = None;
        let mut external_bus = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let trimmed = line.trim();
            let port_name = trimmed
                .split("PortName=")
                .nth(1)
                .and_then(|value| value.split(',').next())
                .map(str::trim);
            let parse_bus = |prefix: &str| {
                trimmed
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|value| value.parse::<u32>().ok())
            };
            if port_name == Some(bridge_path) {
                bridge_bus = parse_bus("CNCA");
            }
            if port_name == Some(external_path) {
                external_bus = parse_bus("CNCB");
            }
        }
        match (bridge_bus, external_bus) {
            (Some(a), Some(b)) if a == b && !is_reserved_bus(a) => Some(a),
            _ => None,
        }
    }

    fn reconcile_orphan_state(&mut self) {
        let driver = self.query_driver_state();
        if !driver.queried {
            return;
        }
        for endpoint in self.orphan_endpoints() {
            if !driver.buses.contains(&endpoint.resource_id) {
                log::info!(
                    "Forgetting already-removed virtual endpoint {} ↔ {} (bus {})",
                    endpoint.bridge_path,
                    endpoint.external_path,
                    endpoint.resource_id
                );
                self.forget_owned_endpoint(&endpoint);
            }
        }
    }

    fn next_free_bus(&self, driver: &DriverState) -> u32 {
        let owned_ids = self
            .load_owned_endpoints()
            .into_iter()
            .map(|endpoint| endpoint.resource_id)
            .collect::<HashSet<_>>();
        let mut bus = driver
            .max_bus
            .into_iter()
            .chain(owned_ids.iter().copied())
            .chain(
                self.active_endpoints
                    .iter()
                    .map(|endpoint| endpoint.resource_id),
            )
            .max()
            .map_or(0, |max| max.saturating_add(1));
        while is_reserved_bus(bus) || driver.buses.contains(&bus) || owned_ids.contains(&bus) {
            bus = bus.saturating_add(1);
        }
        bus
    }

    fn next_bus_after(&self, current: u32, driver: &DriverState) -> u32 {
        let owned_ids = self
            .load_owned_endpoints()
            .into_iter()
            .map(|endpoint| endpoint.resource_id)
            .collect::<HashSet<_>>();
        let mut bus = current.saturating_add(1);
        while is_reserved_bus(bus) || driver.buses.contains(&bus) || owned_ids.contains(&bus) {
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
                    self.forget_owned_endpoint(endpoint);
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
        let driver = self.query_driver_state();
        let candidates = Self::find_available_port_pairs(
            count.saturating_mul(CANDIDATE_MULTIPLIER),
            &driver.occupied_ports,
        );
        if candidates.is_empty() {
            return Err("No available COM port pairs — all port numbers are in use".into());
        }

        let mut bus = self.next_free_bus(&driver);
        let mut pairs = Vec::new();
        let mut skipped = Vec::new();

        for (bridge_number, external_number) in candidates {
            if pairs.len() >= count as usize {
                break;
            }
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
            match run_setupc(
                &self.resource_dir,
                &["install", &bus_arg, &bridge_arg, &external_arg],
            ) {
                Ok(output) if output.status.success() => {
                    let actual_bus = self
                        .resolve_installed_bus(&endpoint.bridge_path, &endpoint.external_path)
                        .ok_or_else(|| {
                            format!(
                                "setupc reported success but the requested mapping {} ↔ {} was not present",
                                endpoint.bridge_path, endpoint.external_path
                            )
                        })?;
                    let mut endpoint = endpoint;
                    endpoint.resource_id = actual_bus;
                    log::info!(
                        "Virtual port pair created: {} ↔ {} (bus {})",
                        endpoint.bridge_path,
                        endpoint.external_path,
                        endpoint.resource_id
                    );
                    self.track_active_endpoint_with_owner(endpoint.clone(), owner_pid);
                    pairs.push(endpoint);
                    bus = self.next_bus_after(actual_bus, &driver);
                }
                Ok(output) => {
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
                        bus = self.next_bus_after(bus, &driver);
                        continue;
                    }
                    for created in pairs.clone() {
                        let _ = self.destroy_endpoint(&created);
                    }
                    return Err(format!(
                        "Failed to create port pair {}↔{} (exit {:?}): {}",
                        endpoint.bridge_path,
                        endpoint.external_path,
                        output.status.code(),
                        detail
                    ));
                }
                Err(error) => {
                    for created in pairs.clone() {
                        let _ = self.destroy_endpoint(&created);
                    }
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
            log::warn!(
                "Requested {} virtual port pairs, created only {}",
                count,
                pairs.len()
            );
        }
        Ok(pairs)
    }

    pub fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        if self.mode == ManagementMode::DirectUac {
            // Disconnect must stay non-interactive. Keep ownership and let the next explicit
            // create/cleanup action reclaim the endpoint through the one-shot helper.
            self.defer_cleanup(endpoint);
            return Ok(());
        }
        self.destroy_endpoint_privileged(endpoint)
    }

    fn destroy_endpoint_privileged(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        let _mutation = DriverMutationGuard::acquire()?;

        let bus = endpoint.resource_id.to_string();

        match run_setupc(&self.resource_dir, &["remove", &bus]) {
            Ok(output) if output.status.success() => {
                self.forget_owned_endpoint(endpoint);
                log::info!(
                    "Virtual port pair destroyed: {} ↔ {}",
                    endpoint.bridge_path,
                    endpoint.external_path
                );
                return Ok(());
            }
            Err(error) => {
                self.defer_cleanup(endpoint);
                return Err(error);
            }
            Ok(_) => {
                let driver = self.query_driver_state();
                if driver.queried && !driver.buses.contains(&endpoint.resource_id) {
                    self.forget_owned_endpoint(endpoint);
                    return Ok(());
                }
            }
        }

        let cnc_a = format!("CNCA{}", endpoint.resource_id);
        let cnc_b = format!("CNCB{}", endpoint.resource_id);
        let _ = run_setupc(&self.resource_dir, &["change", &cnc_a, "PortName=-"]);
        let _ = run_setupc(&self.resource_dir, &["change", &cnc_b, "PortName=-"]);
        std::thread::sleep(std::time::Duration::from_millis(DESTROY_UNBIND_WAIT_MS));

        for attempt in 0..DESTROY_STAGE2_RETRY_COUNT {
            match run_setupc(&self.resource_dir, &["remove", &bus]) {
                Ok(output) if output.status.success() => {
                    self.forget_owned_endpoint(endpoint);
                    log::info!(
                        "Virtual port pair destroyed after unbind: {} ↔ {}",
                        endpoint.bridge_path,
                        endpoint.external_path
                    );
                    return Ok(());
                }
                Ok(_) | Err(_) if attempt + 1 < DESTROY_STAGE2_RETRY_COUNT => {
                    std::thread::sleep(std::time::Duration::from_millis(
                        DESTROY_STAGE2_RETRY_DELAY_MS,
                    ));
                }
                Ok(_) | Err(_) => {}
            }
        }

        let driver = self.query_driver_state();
        if driver.queried && !driver.buses.contains(&endpoint.resource_id) {
            self.forget_owned_endpoint(endpoint);
            Ok(())
        } else {
            self.defer_cleanup(endpoint);
            let error = format!(
                "Virtual port pair {} ↔ {} (bus {}) requires deferred cleanup",
                endpoint.bridge_path, endpoint.external_path, endpoint.resource_id
            );
            log::warn!("{error}");
            Err(error)
        }
    }

    pub fn cleanup_all(&mut self) {
        let active = self.active_endpoints.iter().cloned().collect::<Vec<_>>();
        for endpoint in active {
            let result = if self.mode == ManagementMode::DirectUac {
                self.defer_cleanup(&endpoint);
                Ok(())
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
        let orphans = self.orphan_endpoints();
        if orphans.is_empty() {
            return Ok(0);
        }

        if self.mode == ManagementMode::DirectUac {
            let cleaned = super::elevated::cleanup_endpoints(&self.resource_dir, orphans)?;
            for endpoint in &cleaned {
                self.forget_owned_endpoint(endpoint);
            }
            return Ok(cleaned.len() as u32);
        }

        self.reconcile_orphan_state();
        let orphans = self.orphan_endpoints();
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
        manager.track_active_endpoint(endpoint.clone());
        assert_eq!(manager.pending_orphan_count(), 0);
        manager.defer_cleanup(&endpoint);
        assert_eq!(manager.pending_orphan_count(), 1);
        manager.forget_owned_endpoint(&endpoint);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn current_schema_records_owner_pid() {
        let (mut manager, root) = test_manager();
        let endpoint = sample_endpoint(2);
        manager.track_active_endpoint(endpoint.clone());
        let raw = std::fs::read_to_string(manager.state_path()).unwrap();
        let state: PersistedState = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.schema_version, OWNERSHIP_SCHEMA_VERSION);
        assert_eq!(state.owned_endpoints.len(), 1);
        assert_eq!(state.owned_endpoints[0].owner_pid, Some(std::process::id()));
        manager.forget_owned_endpoint(&endpoint);
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
        writer.remember_owned_endpoints(std::slice::from_ref(&endpoint));
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
        manager.track_active_endpoint(first.clone());
        manager.track_active_endpoint(second.clone());
        manager.defer_cleanup(&first);
        manager.defer_cleanup(&second);
        manager.forget_owned_endpoint(&first);

        let owned = manager.load_owned_endpoints();
        assert_eq!(owned, vec![second.clone()]);
        manager.forget_owned_endpoint(&second);
        let _ = std::fs::remove_dir_all(root);
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
