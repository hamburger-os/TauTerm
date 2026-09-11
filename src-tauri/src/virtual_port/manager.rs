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
    contains_elevation_indicator, register_internal_endpoint_path,
    unregister_internal_endpoint_path, VirtualEndpoint, VirtualPortBackend, VirtualPortConfig,
};

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const ERROR_CANCELLED: u32 = 1223;
#[cfg(target_os = "windows")]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(target_os = "windows")]
const WAIT_TIMEOUT: u32 = 258;
#[cfg(target_os = "windows")]
const ELEVATED_TIMEOUT_MS: u32 = 120_000;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedState {
    owned_endpoints: Vec<VirtualEndpoint>,
}

#[derive(Debug, Default)]
struct DriverState {
    occupied_ports: HashSet<u32>,
    buses: HashSet<u32>,
    max_bus: Option<u32>,
    queried: bool,
}

pub struct VirtualPortManager {
    driver_installed: bool,
    active_endpoints: HashSet<VirtualEndpoint>,
    resource_dir: PathBuf,
    state_dir: Option<PathBuf>,
}

fn normalize_windows_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn is_elevation_error(error: &str) -> bool {
    contains_elevation_indicator(error)
}

fn is_elevation_output(output: &std::process::Output) -> bool {
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    contains_elevation_indicator(&combined)
}

fn output_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr.trim().to_string()
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
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

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
            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
            Err("setupc.exe execution timed out".into())
        }
        Err(_) => Err("setupc.exe process exited abnormally".into()),
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 用一次 UAC 执行受控批处理。批处理内容只由本模块生成，不接受 UI 传入命令。
#[cfg(target_os = "windows")]
fn run_elevated(batch: &str) -> Result<(), String> {
    let batch_path = std::env::temp_dir().join(format!(
        "tauterm-elev-{}.cmd",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&batch_path, batch).map_err(|error| format!("写入临时批处理失败: {error}"))?;

    let verb = wide("runas");
    let file = wide("cmd.exe");
    let params = wide(&format!("/c \"{}\"", batch_path.display()));
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
    sei.nShow = 0;

    if unsafe { ShellExecuteExW(&mut sei) } == 0 {
        let error = unsafe { GetLastError() };
        let _ = std::fs::remove_file(&batch_path);
        if error == ERROR_CANCELLED {
            return Err("User cancelled the UAC elevation prompt".into());
        }
        return Err(format!("提权启动失败 (win32 error {error})"));
    }

    let wait = if sei.hProcess.is_null() {
        WAIT_OBJECT_0
    } else {
        unsafe { WaitForSingleObject(sei.hProcess, ELEVATED_TIMEOUT_MS) }
    };
    if wait == WAIT_TIMEOUT && !sei.hProcess.is_null() {
        unsafe { TerminateProcess(sei.hProcess, 1) };
    }

    let mut exit_code = 0u32;
    if !sei.hProcess.is_null() {
        unsafe { GetExitCodeProcess(sei.hProcess, &mut exit_code) };
        unsafe { CloseHandle(sei.hProcess) };
    }
    let _ = std::fs::remove_file(&batch_path);

    if wait == WAIT_TIMEOUT {
        return Err("提权操作超时".into());
    }
    if exit_code != 0 {
        return Err(format!("提权操作失败 (exit code {exit_code})"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn append_remove_batch(batch: &mut String, setupc: &str, bus: u32) {
    batch.push_str(&format!(
        "\"{setupc}\" remove {bus} >nul 2>&1\r\n\
if errorlevel 1 (\r\n\
  \"{setupc}\" change CNCA{bus} PortName=- >nul 2>&1\r\n\
  \"{setupc}\" change CNCB{bus} PortName=- >nul 2>&1\r\n\
  ping -n 2 127.0.0.1 >nul\r\n\
  \"{setupc}\" remove {bus} >nul 2>&1\r\n\
  if errorlevel 1 exit /b 1\r\n\
)\r\n"
    ));
}

#[cfg(target_os = "windows")]
fn append_best_effort_remove_batch(batch: &mut String, setupc: &str, bus: u32) {
    batch.push_str(&format!(
        "\"{setupc}\" remove {bus} >nul 2>&1\r\n\
if errorlevel 1 (\r\n\
  \"{setupc}\" change CNCA{bus} PortName=- >nul 2>&1\r\n\
  \"{setupc}\" change CNCB{bus} PortName=- >nul 2>&1\r\n\
  ping -n 2 127.0.0.1 >nul\r\n\
  \"{setupc}\" remove {bus} >nul 2>&1\r\n\
)\r\n"
    ));
}

#[cfg(target_os = "windows")]
fn build_elevated_create_batch(
    resource: &str,
    setupc: &str,
    orphans: &[VirtualEndpoint],
    pairs: &[VirtualEndpoint],
) -> String {
    let mut batch = format!("@echo off\r\nchcp 65001 >nul\r\ncd /d \"{resource}\"\r\n");
    for orphan in orphans {
        append_remove_batch(&mut batch, setupc, orphan.resource_id);
    }
    for endpoint in pairs {
        batch.push_str(&format!(
            "\"{setupc}\" install {bus} PortName={bridge} PortName={external},PlugInMode=yes\r\n\
if errorlevel 1 goto rollback\r\n",
            bus = endpoint.resource_id,
            bridge = endpoint.bridge_path,
            external = endpoint.external_path
        ));
    }
    batch.push_str("goto success\r\n:rollback\r\n");
    for endpoint in pairs {
        append_best_effort_remove_batch(&mut batch, setupc, endpoint.resource_id);
    }
    batch.push_str("exit /b 1\r\n:success\r\nexit /b 0\r\n");
    batch
}

impl VirtualPortManager {
    pub fn new(resource_dir: PathBuf, state_dir: PathBuf) -> Self {
        let manager = Self {
            driver_installed: false,
            active_endpoints: HashSet::new(),
            resource_dir: normalize_windows_path(&resource_dir),
            state_dir: Some(normalize_windows_path(&state_dir)),
        };
        // 上次进程记录的 owned endpoint 在当前进程尚无 active owner，因此属于
        // 待恢复/清理资源；同时隐藏其内部 bridge COM，避免出现在普通串口列表中。
        for endpoint in manager.load_owned_endpoints() {
            register_internal_endpoint_path(&endpoint.bridge_path);
        }
        manager
    }

    /// 特权服务模式：生命周期由服务端 client_id 内存记账控制，不把未知驱动 bus
    /// 视为自己的资源，也不在启动时扫描并删除第三方 com0com 端口对。
    pub fn new_stateless(resource_dir: PathBuf) -> Self {
        Self {
            driver_installed: false,
            active_endpoints: HashSet::new(),
            resource_dir: normalize_windows_path(&resource_dir),
            state_dir: None,
        }
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

    pub fn detect_driver(&self) -> bool {
        let mut service_query = Command::new("sc");
        service_query.args(["query", "com0com"]);
        #[cfg(target_os = "windows")]
        service_query.creation_flags(CREATE_NO_WINDOW);
        if service_query
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return true;
        }

        if !self.setupc_path().exists() {
            return false;
        }
        run_setupc(&self.resource_dir, &["list"])
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn state_path(&self) -> Option<PathBuf> {
        self.state_dir
            .as_ref()
            .map(|directory| directory.join("com0com_state.json"))
    }

    fn load_owned_endpoints(&self) -> Vec<VirtualEndpoint> {
        let Some(path) = self.state_path() else {
            return Vec::new();
        };
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
            Ok(mut state) => {
                state
                    .owned_endpoints
                    .sort_by_key(|endpoint| endpoint.resource_id);
                state
                    .owned_endpoints
                    .dedup_by_key(|endpoint| endpoint.resource_id);
                state.owned_endpoints
            }
            Err(error) => {
                // 预稳定阶段不迁移旧 schema；保留备份便于诊断，随后使用唯一的新模型。
                let backup = path.with_extension("json.bak");
                let _ = std::fs::copy(&path, &backup);
                log::warn!(
                    "virtual-port ownership state has obsolete/corrupt schema ({error}); backed up to {:?}",
                    backup
                );
                // 当前版本只接受唯一 ownership schema；不迁移旧 bus-only 状态。
                self.persist_owned_endpoints(&[]);
                Vec::new()
            }
        }
    }

    fn persist_owned_endpoints(&self, endpoints: &[VirtualEndpoint]) {
        let Some(path) = self.state_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create virtual-port state directory: {error}");
                return;
            }
        }

        let mut owned = endpoints.to_vec();
        owned.sort_by_key(|endpoint| endpoint.resource_id);
        owned.dedup_by_key(|endpoint| endpoint.resource_id);
        let state = PersistedState {
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

    fn remember_owned_endpoints(&mut self, endpoints: &[VirtualEndpoint]) {
        if endpoints.is_empty() {
            return;
        }
        let mut owned = self.load_owned_endpoints();
        for endpoint in endpoints {
            register_internal_endpoint_path(&endpoint.bridge_path);
            owned.retain(|existing| existing.resource_id != endpoint.resource_id);
            owned.push(endpoint.clone());
        }
        self.persist_owned_endpoints(&owned);
    }

    fn track_active_endpoint(&mut self, endpoint: VirtualEndpoint) {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        self.active_endpoints.insert(endpoint.clone());
        self.remember_owned_endpoints(std::slice::from_ref(&endpoint));
    }

    fn forget_owned_endpoint(&mut self, endpoint: &VirtualEndpoint) {
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        let mut owned = self.load_owned_endpoints();
        let removed_paths = owned
            .iter()
            .filter(|existing| existing.resource_id == endpoint.resource_id)
            .map(|existing| existing.bridge_path.clone())
            .collect::<Vec<_>>();
        owned.retain(|existing| existing.resource_id != endpoint.resource_id);
        self.persist_owned_endpoints(&owned);

        if removed_paths.is_empty() {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        } else {
            for path in removed_paths {
                unregister_internal_endpoint_path(&path);
            }
        }
    }

    fn defer_cleanup(&mut self, endpoint: &VirtualEndpoint) {
        // owner 消失，但驱动资源可能仍存在：从 active 移除、保留 ownership。
        self.active_endpoints
            .retain(|existing| existing.resource_id != endpoint.resource_id);
        self.remember_owned_endpoints(std::slice::from_ref(endpoint));
    }

    fn orphan_endpoints(&self) -> Vec<VirtualEndpoint> {
        let active_ids = self
            .active_endpoints
            .iter()
            .map(|endpoint| endpoint.resource_id)
            .collect::<HashSet<_>>();
        self.load_owned_endpoints()
            .into_iter()
            .filter(|endpoint| !active_ids.contains(&endpoint.resource_id))
            .collect()
    }

    fn query_driver_state(&self) -> DriverState {
        let mut state = DriverState::default();
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

        // 查询失败时只使用 TauTerm 自己的 ownership 记录避免 bus 碰撞；绝不把这些
        // fallback bus 当成“驱动里所有残留”去删除。
        if !state.queried {
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
        }
        state
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

    pub fn install_driver(&mut self) -> Result<(), String> {
        if self.detect_driver() {
            self.driver_installed = true;
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
        self.driver_installed = true;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub fn install_driver_elevated(&mut self) -> Result<(), String> {
        if self.detect_driver() {
            self.driver_installed = true;
            return Ok(());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }
        let driver = self.query_driver_state();
        let bus = self.next_free_bus(&driver);
        let setupc = self.setupc_path().display().to_string();
        let resource = self.resource_dir.display().to_string();
        let batch = format!(
            "@echo off\r\nchcp 65001 >nul\r\ncd /d \"{resource}\"\r\n\
\"{setupc}\" install {bus} - - >nul 2>&1\r\n\
if errorlevel 1 exit /b 1\r\n\
\"{setupc}\" remove {bus} >nul 2>&1\r\n\
exit /b 0\r\n"
        );
        run_elevated(&batch)?;
        self.driver_installed = true;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn install_driver_elevated(&mut self) -> Result<(), String> {
        Err("UAC elevation is only supported on Windows".into())
    }

    /// 扫描空闲连续 COM 号。extra_occupied 来自 com0com 驱动自身，因为
    /// PlugInMode 端口可能不会出现在 serialport::available_ports() 中。
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

    pub fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

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
            let bridge_arg = format!("PortName={}", endpoint.bridge_path);
            let external_arg = format!("PortName={},PlugInMode=yes", endpoint.external_path);
            let bus_arg = bus.to_string();
            match run_setupc(
                &self.resource_dir,
                &["install", &bus_arg, &bridge_arg, &external_arg],
            ) {
                Ok(output) if output.status.success() => {
                    log::info!(
                        "Virtual port pair created: {} ↔ {} (bus {})",
                        endpoint.bridge_path,
                        endpoint.external_path,
                        endpoint.resource_id
                    );
                    self.track_active_endpoint(endpoint.clone());
                    pairs.push(endpoint);
                    bus = self.next_bus_after(bus, &driver);
                }
                Ok(output) if is_elevation_output(&output) => {
                    for created in pairs.clone() {
                        let _ = self.destroy_endpoint(&created);
                    }
                    return Err(output_detail(&output));
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
        self.driver_installed = true;
        Ok(pairs)
    }

    #[cfg(target_os = "windows")]
    pub fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        self.reconcile_orphan_state();
        let orphans = self.orphan_endpoints();
        let count = config.count.clamp(1, 4);
        let driver = self.query_driver_state();
        let candidates = Self::find_available_port_pairs(count, &driver.occupied_ports);
        if candidates.len() < count as usize {
            return Err("No available COM port pairs".into());
        }

        let mut next_bus = self.next_free_bus(&driver);
        let mut pairs = Vec::new();
        for (bridge_number, external_number) in candidates.into_iter().take(count as usize) {
            pairs.push(VirtualEndpoint {
                bridge_path: format!("COM{bridge_number}"),
                external_path: format!("COM{external_number}"),
                resource_id: next_bus,
            });
            next_bus = self.next_bus_after(next_bus, &driver);
        }

        // 在启动提权子进程前先登记 ownership。这样即使进程超时/被终止，
        // 任何已经安装但来不及执行 rollback 的端口对也不会成为不可追踪资源。
        self.remember_owned_endpoints(&pairs);

        let setupc = self.setupc_path().display().to_string();
        let resource = self.resource_dir.display().to_string();
        let batch = build_elevated_create_batch(&resource, &setupc, &orphans, &pairs);

        if let Err(error) = run_elevated(&batch) {
            if error.to_lowercase().contains("cancel") {
                // ShellExecuteEx 在 UAC 取消时没有创建子进程，因此这些预登记资源
                // 一定不存在，可以立即撤销 ownership。
                for endpoint in &pairs {
                    self.forget_owned_endpoint(endpoint);
                }
                return Err("User cancelled the UAC elevation prompt".to_string());
            }

            // 正常失败路径会在同一批处理中 rollback；超时/异常终止则可能留下
            // 部分资源。重新读取驱动状态，只撤销已确认不存在的 ownership。
            self.reconcile_orphan_state();
            return Err(format!("Elevated virtual-port creation failed: {error}"));
        }

        // 同一提权事务已成功清理 orphan；保留当前 active ownership，仅移除这些 orphan。
        for orphan in &orphans {
            self.forget_owned_endpoint(orphan);
        }
        for endpoint in &pairs {
            self.track_active_endpoint(endpoint.clone());
        }
        self.driver_installed = true;
        Ok(pairs)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn create_endpoints_elevated(
        &mut self,
        _config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        Err("UAC elevation is only supported on Windows".into())
    }

    #[cfg(target_os = "windows")]
    pub fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }
        self.reconcile_orphan_state();
        let orphans = self.orphan_endpoints();
        if orphans.is_empty() {
            return Ok(0);
        }

        let setupc = self.setupc_path().display().to_string();
        let resource = self.resource_dir.display().to_string();
        let mut batch = format!("@echo off\r\nchcp 65001 >nul\r\ncd /d \"{resource}\"\r\n");
        for orphan in &orphans {
            append_remove_batch(&mut batch, &setupc, orphan.resource_id);
        }
        batch.push_str("exit /b 0\r\n");

        run_elevated(&batch).map_err(|error| {
            if error.to_lowercase().contains("cancel") {
                "User cancelled the UAC elevation prompt".to_string()
            } else {
                format!("Elevated virtual-port cleanup failed: {error}")
            }
        })?;

        for orphan in &orphans {
            self.forget_owned_endpoint(orphan);
        }
        Ok(orphans.len() as u32)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        Err("UAC elevation is only supported on Windows".into())
    }

    pub fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
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
            Err(error) if is_elevation_error(&error) => {
                self.defer_cleanup(endpoint);
                return Ok(());
            }
            Ok(output) if is_elevation_output(&output) => {
                self.defer_cleanup(endpoint);
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
                Err(error) if is_elevation_error(&error) => {
                    self.defer_cleanup(endpoint);
                    return Ok(());
                }
                Ok(output) if is_elevation_output(&output) => {
                    self.defer_cleanup(endpoint);
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
        } else {
            self.defer_cleanup(endpoint);
            log::warn!(
                "Virtual port pair {} ↔ {} (bus {}) requires deferred cleanup",
                endpoint.bridge_path,
                endpoint.external_path,
                endpoint.resource_id
            );
        }
        Ok(())
    }

    pub fn cleanup_all(&mut self) {
        for endpoint in self.active_endpoints.iter().cloned().collect::<Vec<_>>() {
            if let Err(error) = self.destroy_endpoint(&endpoint) {
                log::warn!(
                    "Failed to destroy virtual endpoint {} ↔ {}: {}",
                    endpoint.bridge_path,
                    endpoint.external_path,
                    error
                );
            }
        }

        let pending = self.pending_orphan_count();
        if pending > 0 {
            // 退出/断开路径绝不主动弹 UAC。已持久化 orphan 由下次显式创建或
            // “清理残留端口”操作处理，避免在关闭应用时出现意外权限提示。
            log::warn!(
                "{} virtual-port pair(s) remain pending for explicit cleanup",
                pending
            );
        }
    }

    /// 清理上一个进程遗留的、且能证明属于 TauTerm 的端口对。
    ///
    /// 无状态服务后端没有跨进程 ownership 证据，因此绝不扫描删除驱动中的任意 bus。
    pub fn cleanup_orphans(&mut self) -> u32 {
        if self.state_dir.is_none() {
            return 0;
        }
        self.reconcile_orphan_state();
        let orphans = self.orphan_endpoints();
        let total = orphans.len();
        let mut cleaned = 0u32;
        for endpoint in orphans {
            let resource_id = endpoint.resource_id;
            if self.destroy_endpoint(&endpoint).is_ok()
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
        cleaned
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

    fn install_driver_elevated(&mut self) -> Result<(), String> {
        VirtualPortManager::install_driver_elevated(self)
    }

    fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        VirtualPortManager::create_endpoints(self, config)
    }

    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        VirtualPortManager::create_endpoints_elevated(self, config)
    }

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        VirtualPortManager::destroy_endpoint(self, endpoint)
    }

    fn cleanup_all(&mut self) {
        VirtualPortManager::cleanup_all(self)
    }

    fn cleanup_orphans(&mut self) -> u32 {
        VirtualPortManager::cleanup_orphans(self)
    }

    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        VirtualPortManager::cleanup_endpoints_elevated(self)
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
        (VirtualPortManager::new(root.clone(), root.clone()), root)
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
    fn remembered_endpoint_is_orphan_until_activated() {
        let (mut manager, root) = test_manager();
        let endpoint = sample_endpoint(5);
        manager.remember_owned_endpoints(std::slice::from_ref(&endpoint));
        assert_eq!(manager.pending_orphan_count(), 1);
        manager.track_active_endpoint(endpoint.clone());
        assert_eq!(manager.pending_orphan_count(), 0);
        manager.forget_owned_endpoint(&endpoint);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_owned_endpoint_becomes_orphan_after_restart() {
        let (mut manager, root) = test_manager();
        let endpoint = sample_endpoint(2);
        manager.track_active_endpoint(endpoint.clone());
        assert_eq!(manager.pending_orphan_count(), 0);
        drop(manager);

        let mut restarted = VirtualPortManager::new(root.clone(), root.clone());
        assert_eq!(restarted.pending_orphan_count(), 1);
        restarted.forget_owned_endpoint(&endpoint);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forgetting_one_endpoint_preserves_other_ownership() {
        let (mut manager, root) = test_manager();
        let first = sample_endpoint(3);
        let second = sample_endpoint(4);
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
    fn stateless_manager_never_claims_driver_wide_orphans() {
        let root = std::env::temp_dir().join("tauterm-vport-stateless-test");
        let manager = VirtualPortManager::new_stateless(root);
        assert_eq!(manager.pending_orphan_count(), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn elevated_create_batch_rolls_back_every_new_pair() {
        let orphans = vec![sample_endpoint(6)];
        let pairs = vec![sample_endpoint(10), sample_endpoint(11)];
        let batch = build_elevated_create_batch("C:\\TauTerm", "setupc.exe", &orphans, &pairs);

        assert!(batch.contains("if errorlevel 1 goto rollback"));
        assert!(batch.contains("goto success\r\n:rollback\r\n"));
        assert!(batch.contains(":success\r\nexit /b 0"));
        for endpoint in &pairs {
            let remove = format!("\"setupc.exe\" remove {}", endpoint.resource_id);
            assert_eq!(batch.matches(&remove).count(), 2);
        }
    }
}
