//! VirtualPortManager — com0com 虚拟串口端口对管理
//!
//! ## setupc 命令参考 (v3.0.0.0)
//!
//! ```text
//! install <n> <prmsA> <prmsB>  - 安装端口对 CNCA<n>/CNCB<n>
//! install <prmsA> <prmsB>      - 自动分配总线号
//! install                      - 更新驱动（配合 --no-update）
//! remove <n>                   - 删除端口对 CNCA<n>/CNCB<n>
//! list                         - 列出所有端口
//! change <id> <params>         - 修改端口参数
//! disable all / enable all     - 禁用/启用所有端口
//! uninstall                    - 卸载驱动
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use super::backend::{contains_elevation_indicator, PortPair, VirtualPortConfig};

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
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const ERROR_CANCELLED: u32 = 1223;
#[cfg(target_os = "windows")]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(target_os = "windows")]
const WAIT_TIMEOUT: u32 = 258;
/// 批处理最长执行时间。批处理中每个失败 remove 会触发一次 `ping -n 2`
/// （约 1 秒延迟），大量残留端口对时总耗时可能较长，故放宽到 120s。
#[cfg(target_os = "windows")]
const ELEVATED_TIMEOUT_MS: u32 = 120_000;

// ── 可调参数常量 ──────────────────────────────────────

/// setupc.exe 单次命令执行超时（秒）
const SETUPC_TIMEOUT_SECS: u64 = 30;
/// COM 端口扫描上限
const MAX_COM_PORT: u32 = 256;
/// COM 端口扫描起始编号（避开常用低位端口）
const COM_PORT_SCAN_START: u32 = 20;
/// 候选端口对倍数（count * N 个候选对）
const CANDIDATE_MULTIPLIER: u32 = 2;
/// destroy_pair Stage 2 最大重试次数
const DESTROY_STAGE2_RETRY_COUNT: u32 = 3;
/// destroy_pair Stage 2 重试间隔（毫秒）
const DESTROY_STAGE2_RETRY_DELAY_MS: u64 = 200;
/// destroy_pair 解绑端口名后等待系统传播的间隔（毫秒）
const DESTROY_UNBIND_WAIT_MS: u64 = 300;

// ── 预留区（测试脚本 test-serial-session.py 专用，两边需保持一致） ──
// 约定：预留段仅供测试脚本使用，产品扫描端口/bus 时须避开，且服务启动的
// 孤儿清理不得触碰预留 bus 段。对应常量需与 scripts/test-serial-session.py
// 中的 RESERVED_* 完全一致（由 scripts/check-reserved-region.js 在构建时校验）。
/// 预留端口段下限（含）——测试脚本固定使用 COM200/COM201
pub(crate) const RESERVED_PORT_BASE: u32 = 200;
/// 预留端口段上限（含）
pub(crate) const RESERVED_PORT_END: u32 = 255;
/// 预留 bus 段下限（含）——测试脚本固定使用预留段内的某个 bus
pub(crate) const RESERVED_BUS_BASE: u32 = 200;
/// 预留 bus 段上限（含）
pub(crate) const RESERVED_BUS_END: u32 = 255;

/// 是否为预留 bus 段（测试脚本专用，产品不得分配、孤儿清理不得触碰）。
fn is_reserved_bus(bus: u32) -> bool {
    (RESERVED_BUS_BASE..=RESERVED_BUS_END).contains(&bus)
}

/// 是否为预留端口段（测试脚本专用，产品端口扫描须跳过）。
fn is_reserved_port(port: u32) -> bool {
    (RESERVED_PORT_BASE..=RESERVED_PORT_END).contains(&port)
}

pub struct VirtualPortManager {
    driver_installed: bool,
    active_pairs: HashSet<PortPair>,
    resource_dir: PathBuf,
    state_dir: Option<PathBuf>,
}

fn normalize_windows_path(path: &std::path::Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") { PathBuf::from(stripped) } else { path.to_path_buf() }
}

/// 判断错误字符串是否是 Windows 权限不足（保持向后兼容）。
fn is_elevation_error(err: &str) -> bool {
    contains_elevation_indicator(err)
}

/// 检查 setupc 进程 `Output` 的 stdout/stderr 是否包含权限不足错误。
///
/// 覆盖 setupc.exe 成功启动但内核驱动拒绝操作的场景：
/// - spawn 成功（`run_setupc` 返回 `Ok`）
/// - 进程因权限不足返回非零退出码
/// - stderr 包含 "Access is denied." 等
fn is_elevation_output(output: &std::process::Output) -> bool {
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    contains_elevation_indicator(&combined)
}

fn run_setupc(resource_dir: &PathBuf, args: &[&str]) -> Result<std::process::Output, String> {
    let setupc = resource_dir.join("setupc.exe");
    if !setupc.exists() { return Err(format!("setupc.exe not found: {:?}", setupc)); }
    let mut cmd = Command::new(&setupc);
    cmd.current_dir(resource_dir)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn setupc.exe: {}", e))?;
    let pid = child.id();

    // 在独立线程中等待子进程退出，主线程设置 30 秒超时
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(std::time::Duration::from_secs(SETUPC_TIMEOUT_SECS)) {
        Ok(result) => result.map_err(|e| format!("setupc.exe execution failed: {}", e)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            log::warn!("setupc.exe (PID {}) timed out after {}s, attempting to terminate...", pid, SETUPC_TIMEOUT_SECS);
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
            Err("setupc.exe execution timed out".into())
        }
        Err(_) => Err("setupc.exe process exited abnormally".into()),
    }
}

/// 通过 UAC 提权执行一段 `cmd.exe` 批处理命令。
///
/// 使用 `ShellExecuteExW` 的 `runas` 动词触发 UAC，拉起独立的 `cmd.exe`
/// 执行写入临时 `.cmd` 文件的 setupc 序列——不依赖 PowerShell。
/// 通过 `SEE_MASK_NOCLOSEPROCESS` + `WaitForSingleObject` + `GetExitCodeProcess`
/// 正确透传提权子进程的真实退出码。
#[cfg(target_os = "windows")]
fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn run_elevated(batch: &str) -> Result<(), String> {
    // 写入临时批处理文件（UTF-8；内容以 `chcp 65001` 声明编码，路径可含非 ASCII）
    let dir = std::env::temp_dir();
    let name = format!("tauterm-elev-{}.cmd", uuid::Uuid::new_v4().simple());
    let batch_path = dir.join(&name);
    std::fs::write(&batch_path, batch).map_err(|e| format!("写入临时批处理失败: {}", e))?;

    let verb = wide("runas");
    let file = wide("cmd.exe");
    let params = wide(&format!("/c \"{}\"", batch_path.display()));

    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
    sei.nShow = 0; // SW_HIDE

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        let _ = std::fs::remove_file(&batch_path);
        if err == ERROR_CANCELLED {
            return Err("User cancelled the UAC elevation prompt".into());
        }
        return Err(format!("提权启动失败 (win32 error {})", err));
    }

    let wait = if sei.hProcess.is_null() {
        WAIT_OBJECT_0
    } else {
        unsafe { WaitForSingleObject(sei.hProcess, ELEVATED_TIMEOUT_MS) }
    };

    if wait == WAIT_TIMEOUT && !sei.hProcess.is_null() {
        // 超时：强制终止被提权的 cmd.exe，避免其继续在后台执行批处理，
        // 造成「界面报错但端口对实际被创建/删除」的状态不一致。
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
        return Err(format!("提权操作失败 (exit code {})", exit_code));
    }
    Ok(())
}

impl VirtualPortManager {
    pub fn new(resource_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self {
            driver_installed: false,
            active_pairs: HashSet::new(),
            resource_dir: normalize_windows_path(&resource_dir),
            state_dir: Some(normalize_windows_path(&state_dir)),
        }
    }

    /// 无状态模式：不创建、不读取 `com0com_state.json`，也不落盘任何簿记。
    ///
    /// 供特权服务使用：服务按客户端在内存中管理端口对，并依据驱动真实状态
    /// （`setupc list`）清理孤儿，不再需要在 ProgramData 里持久化 bookkeeping——
    /// 那正是卸载时难以删除、导致空目录残留的来源。
    pub fn new_stateless(resource_dir: PathBuf) -> Self {
        Self {
            driver_installed: false,
            active_pairs: HashSet::new(),
            resource_dir: normalize_windows_path(&resource_dir),
            state_dir: None,
        }
    }

    pub fn resource_dir(&self) -> &PathBuf { &self.resource_dir }
    pub fn setupc_path(&self) -> PathBuf { self.resource_dir.join("setupc.exe") }

    /// 返回 `com0com_state.json` 中记录的待清理 bus 数量。
    /// 前端据此决定是否显示"清理残留端口"按钮。
    pub fn pending_orphan_count(&self) -> u32 {
        self.load_active_buses().len() as u32
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
        let mut sc_cmd = Command::new("sc");
        sc_cmd.args(["query", "com0com"]);
        #[cfg(target_os = "windows")]
        {
            sc_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(output) = sc_cmd.output() {
            if output.status.success() { return true; }
        }
        let setupc = self.setupc_path();
        if !setupc.exists() { return false; }
        let mut cmd = Command::new(&setupc);
        cmd.current_dir(&self.resource_dir).arg("list");
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    }

    /// 通过 `setupc list` 查询 com0com 驱动内部状态（只读，无需管理员权限）。
    ///
    /// 返回:
    /// - `occupied_ports`: 驱动中已注册的 COM 端口号集合
    /// - `max_bus`: 驱动中存在的最大 bus 号（None 表示驱动中无端口对）
    /// - `active_buses`: 驱动中所有 bus 号列表（用于清理阶段去重）
    fn query_driver_state(&self) -> (HashSet<u32>, Option<u32>, Vec<u32>) {
        let mut occupied_ports = HashSet::new();
        let mut max_bus: Option<u32> = None;
        let mut active_buses = Vec::new();

        match run_setupc(&self.resource_dir, &["list"]) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    // 解析 CNCA<n> / CNCB<n> → bus 号
                    for prefix in &["CNCA", "CNCB"] {
                        if let Some(rest) = line.trim().strip_prefix(prefix) {
                            if let Ok(n) = rest.split_whitespace().next()
                                .unwrap_or("").parse::<u32>()
                            {
                                // 预留 bus 段仅供测试脚本使用：产品分配 bus（max+1）
                                // 与启动时的孤儿清理都要避开，故不纳入 max_bus/active_buses，
                                // 但其 PortName 仍会走下面的解析进入 occupied_ports 供端口避让。
                                if is_reserved_bus(n) {
                                    continue;
                                }
                                max_bus = Some(max_bus.map_or(n, |m| m.max(n)));
                                if !active_buses.contains(&n) {
                                    active_buses.push(n);
                                }
                            }
                        }
                    }
                    // 解析 PortName=COMxx
                    if let Some(port_part) = line.split("PortName=").nth(1) {
                        let name = port_part.split(',').next().unwrap_or("").trim();
                        if let Some(n) = name.strip_prefix("COM")
                            .and_then(|s| s.parse::<u32>().ok())
                        {
                            occupied_ports.insert(n);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "query_driver_state: setupc list failed ({}) — \
                     falling back to com0com_state.json to avoid bus 0 collision",
                    e
                );
                // Fallback: read known bus numbers from persistence file
                // to prevent falling back to bus 0 when orphan pairs exist
                let state_buses = self.load_active_buses();
                if !state_buses.is_empty() {
                    log::warn!(
                        "query_driver_state: falling back to {} bus(es) from com0com_state.json: {:?}",
                        state_buses.len(), state_buses
                    );
                    for &b in &state_buses {
                        max_bus = Some(max_bus.map_or(b, |m| m.max(b)));
                        if !active_buses.contains(&b) {
                            active_buses.push(b);
                        }
                    }
                }
            }
        }
        (occupied_ports, max_bus, active_buses)
    }

    /// 安装 com0com 内核驱动。通过创建临时端口对触发，随即用 remove <n> 删除。
    pub fn install_driver(&mut self) -> Result<(), String> {
        if self.detect_driver() {
            self.driver_installed = true;
            return Ok(());
        }
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }
        log::info!("Installing com0com driver...");

        // 查询驱动中已用的 bus 号，选择第一个空闲 bus 创建临时端口对
        let (_, driver_max_bus, _) = self.query_driver_state();
        let free_bus = driver_max_bus.map_or(0, |m| m + 1);
        let install_out = run_setupc(&self.resource_dir, &["install", &free_bus.to_string(), "-", "-"])?;
        if !install_out.status.success() {
            let stderr = String::from_utf8_lossy(&install_out.stderr);
            let stdout = String::from_utf8_lossy(&install_out.stdout);
            let detail = if stderr.trim().is_empty() { stdout.to_string() } else { stderr.to_string() };
            return Err(format!(
                "com0com driver install failed (exit {:?}): {}",
                install_out.status.code(),
                detail.trim()
            ));
        }
        // 删除临时端口对，驱动保留
        let _ = run_setupc(&self.resource_dir, &["remove", &free_bus.to_string()]);

        self.driver_installed = true;
        log::info!("com0com driver installed successfully");
        Ok(())
    }

    /// 扫描系统 COM 端口，找到 `count` 对空闲的连续端口号。
    ///
    /// 通过 `serialport::available_ports()` 枚举当前被占用的所有 COM 端口，
    /// 同时合并 `extra_occupied`（来自 com0com 内部状态的端口号），
    /// 从 `max(20, 最高已用端口+2)` 开始向上扫描，找到未占用的连续端口对。
    /// 返回值: Vec<(port_a_num, port_b_num)>，例如 [(22, 23), (24, 25)]。
    pub fn find_available_port_pairs(count: u32, extra_occupied: &HashSet<u32>) -> Vec<(u32, u32)> {
        let mut in_use: HashSet<u32> = match serialport::available_ports() {
            Ok(ports) => ports.iter()
                .filter_map(|p| {
                    p.port_name.strip_prefix("COM")
                        .and_then(|n| n.parse::<u32>().ok())
                })
                .collect(),
            Err(e) => {
                log::warn!("Failed to enumerate system COM ports: {} — assuming none in use", e);
                HashSet::new()
            }
        };

        // 合并 com0com 驱动内部已注册的端口号
        // （PlugInMode=yes 时对 Windows 不可见，但驱动中仍占用）
        in_use.extend(extra_occupied);

        log::info!(
            "Scanning available COM port pairs (in-use: {:?})",
            in_use.iter().collect::<Vec<_>>()
        );

        // 起始点: max(20, 最高已用端口+2)，避开常用低位端口和已知占用区域
        let max_in_use = in_use.iter().max().copied().unwrap_or(0);
        let start = (max_in_use + 2).max(COM_PORT_SCAN_START);
        let mut pairs = Vec::new();
        let mut candidate = start;
        // 是否已完成一次"高位扫尽后回绕到低位再扫"。当起点被测试脚本的预留端口
        // （COM200/201）推到预留段内时，高位扫描会被预留段整体跳过而扫不到端口，
        // 需回绕到低位区段继续找，避免预留段让产品无可用端口。
        let mut wrapped = false;

        while pairs.len() < count as usize {
            if candidate >= MAX_COM_PORT {
                if wrapped {
                    break;
                }
                wrapped = true;
                candidate = COM_PORT_SCAN_START;
                continue;
            }
            // 预留端口段仅供测试脚本使用：即便驱动中暂无该段端口（测试未运行），
            // 产品也绝不占用，否则会在测试运行时与其冲突。
            if is_reserved_port(candidate) || is_reserved_port(candidate + 1) {
                // 跳过整个预留段，继续向上（高位扫尽后由上面的 wrapped 回绕到低位）。
                candidate = RESERVED_PORT_END + 1;
                continue;
            }
            if !in_use.contains(&candidate) && !in_use.contains(&(candidate + 1)) {
                pairs.push((candidate, candidate + 1));
                // 标记为已预留，避免后续迭代选中同一端口
                in_use.insert(candidate);
                in_use.insert(candidate + 1);
            }
            candidate += 2;
        }

        if pairs.len() < count as usize {
            log::warn!(
                "Found only {} COM port pair(s) (requested {}), check system port usage",
                pairs.len(),
                count
            );
        }

        pairs
    }

    /// 创建虚拟串口端口对。
    ///
    /// 动态扫描系统 COM 端口后分配空闲端口号，每对使用
    /// `setupc install <bus> PortName=COMxx PortName=COMxx`。
    ///
    /// 如果某个候选端口被 Windows COM 端口数据库占用
    /// （幽灵设备，`available_ports()` 检测不到），自动跳过该端口
    /// 并尝试下一个候选，而非立即失败。
    /// 仅在所有候选端口都失败时才返回错误。
    pub fn create_pairs(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        let count = config.count.clamp(1, 4);

        // ── Pre-query com0com driver internal state ──
        // setupc list is a read-only operation, no admin required.
        // Retrieves registered port names + bus numbers to avoid conflicts.
        let (com0com_ports, driver_max_bus, _) = self.query_driver_state();
        let candidates = Self::find_available_port_pairs(count * CANDIDATE_MULTIPLIER, &com0com_ports);

        if candidates.is_empty() {
            return Err("No available COM port pairs — all port numbers are in use".into());
        }

        // 起始 bus = 所有已知来源的最大值 + 1
        // （active_pairs / com0com_state.json / 驱动真实状态）
        let active_max = self.active_pairs.iter().map(|p| p.bus_number).max();
        let state_max = self.load_active_buses().iter().max().copied();
        let mut bus = [active_max, state_max, driver_max_bus]
            .iter()
            .filter_map(|&m| m)
            .max()
            .map_or(0, |m| m + 1);
        let mut pairs: Vec<PortPair> = Vec::new();
        let mut skipped_ports: Vec<String> = Vec::new();

        for (port_a_num, port_b_num) in &candidates {
            // 已创建足够数量的端口对
            if pairs.len() >= count as usize {
                break;
            }

            let port_a = format!("COM{}", port_a_num);
            let port_b = format!("COM{}", port_b_num);

            let output = run_setupc(
                &self.resource_dir,
                &[
                    "install",
                    &bus.to_string(),
                    &format!("PortName={}", port_a),
                    &format!("PortName={},PlugInMode=yes", port_b),
                ],
            );

            match output {
                Ok(out) if out.status.success() => {
                    log::info!(
                        "  Virtual port pair created: {} ↔ {} (bus {})",
                        port_a, port_b, bus
                    );
                }
                Ok(out) if out.status.code() == Some(1) => {
                    // exit code 1 → port pair with same name already exists, reuse it
                    log::info!(
                        "  Reusing existing port pair: {} ↔ {} (bus {})",
                        port_a, port_b, bus
                    );
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let detail = if stderr.is_empty() { stdout } else { stderr };
                    let detail_lower = detail.to_lowercase();

                    // Port name marked as "in use" by Windows COM port database
                    // (ghost device, uninstall residue, etc.) — skip and try next candidate
                    if detail_lower.contains("in use")
                        || detail_lower.contains("already logged")
                        || detail_lower.contains("already exists")
                    {
                        log::warn!(
                            "  Skipping port COM{}/COM{}: {}",
                            port_a_num, port_b_num, detail.trim()
                        );
                        skipped_ports.push(format!("COM{}/COM{}", port_a_num, port_b_num));
                        // bus 不递增 — 当前 bus 上 install 已完整失败，
                        // 下一个候选端口重用同一 bus（com0com install 是原子的）
                        continue;
                    }

                    // Non-recoverable error: roll back created pairs and return error
                    for p in &pairs {
                        let _ = run_setupc(
                            &self.resource_dir,
                            &["remove", &p.bus_number.to_string()],
                        );
                    }
                    return Err(format!(
                        "Failed to create port pair {}↔{} (exit {:?}): {}",
                        port_a, port_b, out.status.code(), detail.trim()
                    ));
                }
                Err(e) => {
                    for p in &pairs {
                        let _ = run_setupc(
                            &self.resource_dir,
                            &["remove", &p.bus_number.to_string()],
                        );
                    }
                    return Err(format!("Failed to spawn setupc.exe: {}", e));
                }
            }

            pairs.push(PortPair {
                port_a,
                port_b,
                bus_number: bus,
            });
            bus += 1;
        }

        if !skipped_ports.is_empty() {
            log::warn!(
                "Skipped {} occupied port pairs: {:?}",
                skipped_ports.len(),
                skipped_ports
            );
        }

        if pairs.is_empty() {
            return Err(format!(
                "All candidate port pairs are occupied (attempted {} pairs)",
                candidates.len()
            ));
        }

        if (pairs.len() as u32) < count {
            log::warn!(
                "Requested {} port pairs, but only {} were created",
                count,
                pairs.len()
            );
        }

        self.driver_installed = true;
        for p in &pairs {
            self.active_pairs.insert(p.clone());
        }
        self.sync_state_file();
        Ok(pairs)
    }

    /// 通过 UAC 提权创建端口对，同时一并清理所有已知的残留端口对（同一 UAC 内先删后建）。
    ///
    /// 当普通权限下 `create_pairs()` 因 os error 740 失败且驱动已安装时调用。
    /// 单个提权的 PowerShell 脚本完成以下操作：
    /// 1. 读取 `com0com_state.json` + 当前 active_pairs 中的所有 bus 号
    /// 2. 逐一 `remove <bus>` + 解绑端口名重试
    /// 3. 创建新的端口对
    ///
    /// 仅触发一次 UAC 弹窗。
    #[cfg(target_os = "windows")]
    pub fn create_pairs_elevated(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        let count = config.count.clamp(1, 4);

        // ── Pre-query com0com driver internal state ──
        let (com0com_ports, driver_max_bus, driver_buses) = self.query_driver_state();
        let candidates = Self::find_available_port_pairs(count, &com0com_ports);
        if candidates.is_empty() {
            return Err("No available COM port pairs".into());
        }

        let setupc = self.setupc_path();
        let setupc_str = setupc.display().to_string();
        let resource_str = self.resource_dir.display().to_string();

        // 收集所有需要清理的 bus 号
        // = 簿记中的 bus ∪ 驱动中真实存在的 bus ∪ com0com_state.json 中的 orphans
        let mut stale_buses: Vec<u32> = self.active_pairs.iter().map(|p| p.bus_number).collect();
        let orphans = self.load_active_buses();
        for bus in driver_buses.iter().chain(orphans.iter()) {
            if !stale_buses.contains(bus) {
                stale_buses.push(*bus);
            }
        }

        // 批处理：先清理所有旧端口对（两阶段），再创建新端口对
        let mut batch = format!("@echo off\r\nchcp 65001 >nul\r\ncd /d \"{}\"\r\n", resource_str);

        // 阶段 0: 清理所有已知端口对（先删 → 失败则解绑端口名后再删）
        for bus in &stale_buses {
            batch.push_str(&format!(
                "\"{setupc}\" remove {bus} >nul 2>&1\r\nif errorlevel 1 (\r\n  \"{setupc}\" change CNCA{bus} PortName=- >nul 2>&1\r\n  \"{setupc}\" change CNCB{bus} PortName=- >nul 2>&1\r\n  ping -n 2 127.0.0.1 >nul\r\n  \"{setupc}\" remove {bus} >nul 2>&1\r\n)\r\n",
                setupc = setupc_str, bus = bus
            ));
        }

        // 阶段 1: 创建新端口对（bus 号从驱动真实最大值之后开始）
        let first_bus = driver_max_bus.map_or(0, |m| m + 1);
        for (i, (port_a_num, port_b_num)) in candidates.iter().enumerate() {
            let bus = first_bus + i as u32;
            batch.push_str(&format!(
                "\"{setupc}\" install {bus} PortName=COM{pa} PortName=COM{pb},PlugInMode=yes\r\nif errorlevel 1 exit /b 1\r\n",
                setupc = setupc_str, bus = bus, pa = port_a_num, pb = port_b_num
            ));
        }
        batch.push_str("exit /b 0\r\n");

        log::info!(
            "Elevated cleanup of {} stale port pairs + creation of {} new pairs",
            stale_buses.len(), candidates.len()
        );

        run_elevated(&batch).map_err(|e| {
            if e.contains("cancel") {
                "User cancelled the UAC elevation prompt".to_string()
            } else {
                format!("Elevated port pair creation failed: {}", e)
            }
        })?;

        // 清理旧的追踪记录
        self.active_pairs.clear();
        self.persist_active_buses(&[]);

        // 构建返回的 PortPair 列表
        let pairs: Vec<PortPair> = candidates.iter().enumerate().map(|(i, (a, b))| {
            PortPair { port_a: format!("COM{}", a), port_b: format!("COM{}", b), bus_number: (first_bus + i as u32) }
        }).collect();

        self.driver_installed = true;
        for p in &pairs {
            self.active_pairs.insert(p.clone());
        }
        self.sync_state_file();
        log::info!("Elevated port pair creation succeeded: {} pair(s)", pairs.len());
        Ok(pairs)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn create_pairs_elevated(&mut self, _config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        Err("UAC elevation is only supported on Windows".into())
    }

    /// 通过 UAC 提权批量清理残留的虚拟端口对。
    ///
    /// 收集所有已知 bus 号（active_pairs + com0com_state.json + 驱动真实状态），
    /// 构建单个 PowerShell 脚本逐对清理（remove → 失败则解绑端口名后重试），
    /// 仅触发**一次** UAC 弹窗。
    ///
    /// 返回成功清理的端口对数量。
    #[cfg(target_os = "windows")]
    pub fn cleanup_pairs_elevated(&mut self) -> Result<u32, String> {
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        let stale_buses = self.collect_stale_buses();
        if stale_buses.is_empty() {
            log::info!("cleanup_pairs_elevated: no port pairs to clean up");
            return Ok(0);
        }

        let setupc = self.setupc_path();
        let setupc_str = setupc.display().to_string();
        let resource_str = self.resource_dir.display().to_string();

        // 批处理：逐对 remove，失败则解绑端口名后重试（两阶段清理）
        let mut batch = format!("@echo off\r\nchcp 65001 >nul\r\ncd /d \"{}\"\r\n", resource_str);
        for bus in &stale_buses {
            batch.push_str(&format!(
                "\"{setupc}\" remove {bus} >nul 2>&1\r\nif errorlevel 1 (\r\n  \"{setupc}\" change CNCA{bus} PortName=- >nul 2>&1\r\n  \"{setupc}\" change CNCB{bus} PortName=- >nul 2>&1\r\n  ping -n 2 127.0.0.1 >nul\r\n  \"{setupc}\" remove {bus} >nul 2>&1\r\n)\r\n",
                setupc = setupc_str, bus = bus
            ));
        }
        batch.push_str("exit /b 0\r\n");

        log::info!(
            "cleanup_pairs_elevated: batch cleaning {} port pairs via UAC",
            stale_buses.len()
        );

        run_elevated(&batch).map_err(|e| {
            if e.contains("cancel") {
                "User cancelled the UAC elevation prompt".to_string()
            } else {
                format!("Elevated port pair cleanup failed: {}", e)
            }
        })?;

        // Cleanup succeeded — clear all bookkeeping
        let cleaned = stale_buses.len() as u32;
        self.active_pairs.clear();
        self.persist_active_buses(&[]);
        log::info!(
            "cleanup_pairs_elevated: successfully cleaned up {} port pairs",
            cleaned
        );
        Ok(cleaned)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn cleanup_pairs_elevated(&mut self) -> Result<u32, String> {
        Err("UAC elevation is only supported on Windows".into())
    }

    /// 删除一个虚拟端口对（两阶段清理策略）。
    ///
    /// 1. 直接 `remove <n>` — 端口未被占用时立即成功。
    /// 2. `change CNCA<n>/CNCB<n> PortName=-` 解绑两侧 COM 端口名 → 重试 `remove <n>` —
    ///    端口名变更后外部工具的 COM 引用失效，使 remove 成功。
    ///
    /// 如果当前进程没有管理员权限（os error 740），两阶段都会失败。
    /// 此时仅更新本地簿记（移除 active_pairs，保留 state 文件），
    /// 实际的驱动级清理由下次 `create_pairs_elevated()` 在同一次 UAC 中完成。
    pub fn destroy_pair(&mut self, pair: &PortPair) -> Result<(), String> {
        let bus = pair.bus_number.to_string();
        let cnc_a = format!("CNCA{}", pair.bus_number);
        let cnc_b = format!("CNCB{}", pair.bus_number);

        // ── Stage 1: 直接删除 ──
        match run_setupc(&self.resource_dir, &["remove", &bus]) {
            Ok(out) if out.status.success() => {
                log::info!("Virtual port pair destroyed: {} ↔ {}", pair.port_a, pair.port_b);
                self.active_pairs.remove(pair);
                self.sync_state_file();
                return Ok(());
            }
            // exit code 1: port pair already gone (may have been deleted externally
            // or previously deleted but bookkeeping not updated)
            Ok(out) if out.status.code() == Some(1) => {
                log::info!(
                    "Port pair already gone (exit code 1): {} ↔ {} (bus {})",
                    pair.port_a, pair.port_b, pair.bus_number
                );
                self.active_pairs.remove(pair);
                self.sync_state_file();
                return Ok(());
            }
            Err(ref e) if is_elevation_error(e) => {
                // Elevation required: retain bus in state file, driver-level cleanup
                // deferred to next UAC operation
                log::warn!(
                    "Cannot destroy {}↔{} (elevation required) — deferred to next elevated operation",
                    pair.port_a, pair.port_b
                );
                self.mark_for_deferred_cleanup(pair.bus_number);
                return Ok(());
            }
            Ok(ref out) if is_elevation_output(out) => {
                // setupc launched successfully but kernel driver denied the operation
                // (stderr contains elevation error) — defer
                log::warn!(
                    "Cannot destroy {}↔{} (elevation required, stderr detection) — deferred to next elevated operation",
                    pair.port_a, pair.port_b
                );
                self.mark_for_deferred_cleanup(pair.bus_number);
                return Ok(());
            }
            Ok(_) => {
                // setupc ran but returned non-zero exit code → proceed to Stage 2
            }
            Err(e) => {
                // Non-elevation error (e.g. file not found) → propagate up
                return Err(e);
            }
        }

        // ── Stage 2: unbind COM port names then retry remove ──
        // change CNCA<n>/CNCB<n> PortName=- restores port names to internal
        // identifiers; external tools' COM references become invalid, allowing
        // remove to succeed
        log::info!(
            "Port {} ↔ {} is in use, attempting unbind + remove...",
            pair.port_a, pair.port_b
        );
        let _ = run_setupc(&self.resource_dir, &["change", &cnc_a, "PortName=-"]);
        let _ = run_setupc(&self.resource_dir, &["change", &cnc_b, "PortName=-"]);
        // Brief wait for system to propagate port name changes
        std::thread::sleep(std::time::Duration::from_millis(DESTROY_UNBIND_WAIT_MS));

        // Retry remove (up to DESTROY_STAGE2_RETRY_COUNT attempts with delay)
        for attempt in 0..DESTROY_STAGE2_RETRY_COUNT {
            match run_setupc(&self.resource_dir, &["remove", &bus]) {
                Ok(out2) if out2.status.success() => {
                    log::info!(
                        "Virtual port pair destroyed (unbind + remove): {} ↔ {}",
                        pair.port_a, pair.port_b
                    );
                    self.active_pairs.remove(pair);
                    self.sync_state_file();
                    return Ok(());
                }
                Err(ref e) if is_elevation_error(e) => {
                    log::warn!(
                        "Stage 2 destroy {}↔{} elevation required — deferred to next elevated operation",
                        pair.port_a, pair.port_b
                    );
                    self.mark_for_deferred_cleanup(pair.bus_number);
                    return Ok(());
                }
                Ok(ref out2) if is_elevation_output(out2) => {
                    log::warn!(
                        "Stage 2 destroy {}↔{} elevation required (stderr detection) — deferred to next elevated operation",
                        pair.port_a, pair.port_b
                    );
                    self.mark_for_deferred_cleanup(pair.bus_number);
                    return Ok(());
                }
                Ok(_) | Err(_) => {
                    if attempt < DESTROY_STAGE2_RETRY_COUNT - 1 {
                        log::debug!("Stage 2 destroy attempt {}/{} failed, retrying after delay...", attempt + 1, DESTROY_STAGE2_RETRY_COUNT);
                        std::thread::sleep(std::time::Duration::from_millis(DESTROY_STAGE2_RETRY_DELAY_MS));
                    }
                }
            }
        }

        // Both cleanup stages failed — retain bus in state file,
        // deferred to next UAC operation or cleanup_orphans
        log::error!(
            "Cannot destroy {}↔{} (bus {}) — both cleanup stages failed, \
             deferred to next startup or elevated operation",
            pair.port_a, pair.port_b, pair.bus_number
        );
        self.mark_for_deferred_cleanup(pair.bus_number);
        Ok(())
    }

    /// 退出时清理端口对（驱动保留供下次快速复用）。
    ///
    /// 优先尝试直接删除（当前进程已提权时立即生效）。
    /// 如果 `destroy_pair` 将 bus 号写入了 state 文件（`mark_for_deferred_cleanup`），
    /// 说明权限不足 → 通过 UAC 提权批量清理（单次弹窗）。
    /// UAC 被取消时保留 bus 号到 state 文件供下次启动处理。
    pub fn cleanup_all(&mut self) {
        let pairs: Vec<PortPair> = self.active_pairs.iter().cloned().collect();

        for pair in &pairs {
            if let Err(e) = self.destroy_pair(pair) {
                log::warn!(
                    "cleanup_all: failed to destroy {}↔{} (bus {}): {}",
                    pair.port_a, pair.port_b, pair.bus_number, e
                );
            }
        }

        // destroy_pair returns Ok(()) for elevation errors but writes bus
        // numbers to state file via mark_for_deferred_cleanup →
        // check state file to determine if UAC is needed
        let remaining = self.load_active_buses();
        if !remaining.is_empty() {
            log::info!(
                "cleanup_all: {} port pair(s) require admin, attempting UAC batch cleanup...",
                remaining.len()
            );
            match self.cleanup_pairs_elevated() {
                Ok(cleaned) => {
                    log::info!(
                        "cleanup_all: UAC batch cleanup succeeded: {} port pair(s)",
                        cleaned
                    );
                    return; // cleanup_pairs_elevated already cleared state file
                }
                Err(e) => {
                    log::warn!(
                        "cleanup_all: UAC batch cleanup failed: {} — {} bus(es) retained until next startup",
                        e, remaining.len()
                    );
                    // state file preserved for next startup cleanup_orphans or
                    // frontend [cleanup] button
                }
            }
        }

        // No remaining orphans → ensure state file is cleared
        self.persist_active_buses(&[]);
    }

    // ── 孤儿端口对持久化追踪 ─────────────────────────

    fn state_path(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|d| d.join("com0com_state.json"))
    }

    fn load_active_buses(&self) -> Vec<u32> {
        let Some(path) = self.state_path() else { return Vec::new(); };
        if !path.exists() {
            return Vec::new();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| match serde_json::from_str::<Vec<u32>>(&s) {
                Ok(buses) => Some(buses),
                Err(e) => {
                    let bak = path.with_extension("json.bak");
                    let _ = std::fs::copy(&path, &bak);
                    log::warn!(
                        "com0com_state.json corrupted ({}), backed up to {:?}, starting fresh",
                        e, bak
                    );
                    None
                }
            })
            .unwrap_or_default()
    }

    fn persist_active_buses(&self, buses: &[u32]) {
        let Some(path) = self.state_path() else { return; };
        let tmp_path = path.with_extension("json.tmp");
        let json = match serde_json::to_string(buses) {
            Ok(s) => s,
            Err(e) => {
                log::error!("persist_active_buses: serialization failed: {}", e);
                return;
            }
        };
        // Write to temp file first, then atomic rename to prevent corruption on crash
        if let Err(e) = std::fs::write(&tmp_path, &json) {
            log::warn!("Failed to write state temp file: {}", e);
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, &path) {
            log::warn!("Failed to rename state file: {}", e);
            // Fallback: write directly to target file
            let _ = std::fs::write(&path, json);
        }
    }

    /// 将当前 active_pairs 的 bus 编号写入持久化文件。
    fn sync_state_file(&self) {
        let buses: Vec<u32> = self.active_pairs.iter().map(|p| p.bus_number).collect();
        self.persist_active_buses(&buses);
    }

    /// 权限不足时：从 active_pairs 移除端口对，但将 bus 号追加到持久化文件，
    /// 确保下次 UAC 提权操作（`create_pairs_elevated` / `cleanup_pairs_elevated`）
    /// 能找到并清理该端口对。
    ///
    /// 与 `sync_state_file()` 的区别：
    /// - `sync_state_file()` 从 active_pairs 重建 state 文件（覆盖式）
    /// - 本方法在 state 文件中**保留**已知的残留 bus 号（追加式）
    fn mark_for_deferred_cleanup(&mut self, bus_number: u32) {
        self.active_pairs.retain(|p| p.bus_number != bus_number);
        let mut buses = self.load_active_buses();
        if !buses.contains(&bus_number) {
            buses.push(bus_number);
        }
        self.persist_active_buses(&buses);
    }

    /// 收集所有已知需要清理的 bus 号。
    ///
    /// 来源：active_pairs ∪ com0com_state.json 中的 orphans ∪ 驱动中真实存在的 bus
    fn collect_stale_buses(&self) -> Vec<u32> {
        let (_, _, driver_buses) = self.query_driver_state();
        let mut buses: Vec<u32> = self.active_pairs.iter().map(|p| p.bus_number).collect();
        let orphans = self.load_active_buses();
        for bus in driver_buses.iter().chain(orphans.iter()) {
            if !buses.contains(bus) {
                buses.push(*bus);
            }
        }
        buses
    }

    /// 启动时清理上次异常退出遗留的端口对（仅直接清理，不弹 UAC）。
    ///
    /// 无状态模式（特权服务）：无 `com0com_state.json` 可读，改为通过 `setupc list`
    /// 枚举驱动真机状态，剔除当前仍活跃的 bus 后逐一尝试 `setupc remove <n>`。
    /// 有状态模式：仍从 `com0com_state.json` 读取记录的 bus 编号。
    ///
    /// 如果 remove 失败（端口被占用），则先解绑两侧端口名再重试。
    /// 权限不足时保留 bus 号到 state 文件，由前端"清理残留端口"按钮
    /// 或下次连接的 `create_pairs_elevated` 统一处理。
    /// 返回成功清理的端口对数量。
    pub fn cleanup_orphans(&mut self) -> u32 {
        let orphans: Vec<u32> = if self.state_dir.is_none() {
            // 无状态模式：以驱动真机状态（setupc list）为准，剔除当前仍活跃的 bus
            self.query_driver_state().2
                .into_iter()
                .filter(|b| !self.active_pairs.iter().any(|p| p.bus_number == *b))
                .collect()
        } else {
            self.load_active_buses()
        };
        if orphans.is_empty() {
            return 0;
        }
        log::info!(
            "Found {} possibly orphaned port pairs, checking and cleaning up...",
            orphans.len()
        );
        let mut cleaned = 0u32;
        let mut remaining = Vec::new();

        for bus in &orphans {
            let bus_str = bus.to_string();
            match run_setupc(&self.resource_dir, &["remove", &bus_str]) {
                Err(ref e) if is_elevation_error(e) => {
                    log::warn!(
                        "  Orphan bus={} requires admin — use status bar [cleanup] button",
                        bus
                    );
                    remaining.push(*bus);
                }
                Ok(ref out) if is_elevation_output(out) => {
                    log::warn!(
                        "  Orphan bus={} requires admin (stderr detection)",
                        bus
                    );
                    remaining.push(*bus);
                }
                Ok(out) if out.status.success() => {
                    log::info!("  Cleaned up orphan bus={}", bus);
                    cleaned += 1;
                }
                Ok(_) => {
                    log::info!(
                        "  Orphan bus={} is in use, attempting unbind + remove...",
                        bus
                    );
                    let cnc_a = format!("CNCA{}", bus);
                    let cnc_b = format!("CNCB{}", bus);
                    let _ = run_setupc(&self.resource_dir, &["change", &cnc_a, "PortName=-"]);
                    let _ = run_setupc(&self.resource_dir, &["change", &cnc_b, "PortName=-"]);
                    std::thread::sleep(std::time::Duration::from_millis(DESTROY_UNBIND_WAIT_MS));

                    match run_setupc(&self.resource_dir, &["remove", &bus_str]) {
                        Ok(out) if out.status.success() => {
                            log::info!("  Cleaned up orphan bus={} (unbind + remove)", bus);
                            cleaned += 1;
                        }
                        Ok(ref out) if is_elevation_output(out) => {
                            log::warn!(
                                "  Orphan bus={} requires admin (stderr detection)",
                                bus
                            );
                            remaining.push(*bus);
                        }
                        _ => {
                            log::error!(
                                "  Cannot clean up orphan bus={} — retained until next startup",
                                bus
                            );
                            remaining.push(*bus);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("  Error cleaning up orphan bus={}: {}", bus, e);
                    remaining.push(*bus);
                }
            }
        }

        if cleaned > 0 {
            log::info!("Orphan port cleanup completed: {}/{}", cleaned, orphans.len());
        }

        // 只保留未成功清理的 bus — 由前端[清理残留端口]按钮或下次连接时统一 UAC 清理
        if !remaining.is_empty() {
            self.persist_active_buses(&remaining);
            log::info!(
                "{} orphan port pair(s) require admin — use status bar [cleanup] button or next connection",
                remaining.len()
            );
        } else {
            self.persist_active_buses(&[]);
        }
        cleaned
    }
}

// ── VirtualPortBackend trait 实现 ────────────────────

use super::backend::VirtualPortBackend;

impl VirtualPortBackend for VirtualPortManager {
    fn are_files_present(&self) -> bool {
        self.are_files_present()
    }

    fn detect_driver(&self) -> bool {
        self.detect_driver()
    }

    fn install_driver(&mut self) -> Result<(), String> {
        self.install_driver()
    }

    fn install_driver_elevated(&mut self) -> Result<(), String> {
        self.install_driver_elevated()
    }

    fn create_pairs(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        self.create_pairs(config)
    }

    fn create_pairs_elevated(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        self.create_pairs_elevated(config)
    }

    fn destroy_pair(&mut self, pair: &PortPair) -> Result<(), String> {
        self.destroy_pair(pair)
    }

    fn cleanup_all(&mut self) {
        self.cleanup_all()
    }

    fn cleanup_orphans(&mut self) -> u32 {
        self.cleanup_orphans()
    }

    fn cleanup_pairs_elevated(&mut self) -> Result<u32, String> {
        self.cleanup_pairs_elevated()
    }

    fn pending_orphan_count(&self) -> u32 {
        self.pending_orphan_count()
    }
}

// ── 提权安装驱动（从 commands.rs 迁入） ──

impl VirtualPortManager {
    /// 通过 UAC 提权安装 com0com 内核驱动。
    ///
    /// 当 `install_driver()` 因权限不足失败时，通过 PowerShell
    /// `Start-Process -Verb RunAs` 触发 UAC 弹窗，在提权环境中
    /// 创建临时端口对以触发驱动安装，随后删除。
    #[cfg(target_os = "windows")]
    pub fn install_driver_elevated(&mut self) -> Result<(), String> {
        if !self.are_files_present() {
            return Err("com0com driver files missing".into());
        }

        let setupc_str = self.setupc_path().display().to_string();
        let resource_str = self.resource_dir().display().to_string();

        // 选择空闲 bus，避免硬编码 bus 0 与已有端口对冲突
        let (_, driver_max_bus, _) = self.query_driver_state();
        let free_bus = driver_max_bus.map_or(0, |m| m + 1);
        let bus_str = free_bus.to_string();

        // 批处理：创建临时端口对触发驱动安装，成功后删除临时端口对（驱动保留）。
        let batch = format!(
            "@echo off\r\nchcp 65001 >nul\r\ncd /d \"{res}\"\r\n\"{setupc}\" install {bus} - -\r\nif errorlevel 1 exit /b 1\r\n\"{setupc}\" remove {bus}\r\nexit /b 0\r\n",
            res = resource_str, setupc = setupc_str, bus = bus_str
        );

        log::info!("Elevated install of com0com driver (bus {})", free_bus);
        run_elevated(&batch)?;
        self.driver_installed = true;
        log::info!("com0com driver elevated install succeeded");
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn install_driver_elevated(&mut self) -> Result<(), String> {
        Err("UAC elevation is only supported on Windows".into())
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn test_count_clamping() {
        assert_eq!(0u32.clamp(1, 4), 1);
        assert_eq!(5u32.clamp(1, 4), 4);
    }

    #[test]
    fn test_find_port_pairs_empty() {
        // `find_available_port_pairs` 是纯逻辑函数，不依赖 I/O。
        // 如果当前机器没有可用端口，返回空 vec 是合法行为。
        let extra = HashSet::new();
        let pairs = VirtualPortManager::find_available_port_pairs(1, &extra);
        // 不崩溃即可 — 空集和单对都是合法结果
        assert!(pairs.len() <= 1);
    }

    #[test]
    fn test_find_port_pairs_excludes_reserved() {
        // 预留端口段仅供测试脚本使用：无论扫描起点被推到哪（模拟测试脚本占用
        // COM200/201 把起点顶入预留段），产品都不应返回预留段内的端口。
        let extra: HashSet<u32> = [200u32, 201u32, 240u32].into_iter().collect();
        let pairs = VirtualPortManager::find_available_port_pairs(4, &extra);
        for (a, b) in &pairs {
            assert!(
                !is_reserved_port(*a) && !is_reserved_port(*b),
                "reserved port leaked into product allocation: {}↔{}",
                a,
                b
            );
        }
    }

    // ── elevation 检测单元测试 ────────────────────────

    #[test]
    fn test_elevation_spawn_error_740() {
        assert!(contains_elevation_indicator(
            "Failed to spawn setupc.exe: Access is denied. (os error 740)"
        ));
    }

    #[test]
    fn test_elevation_stderr_access_denied() {
        assert!(contains_elevation_indicator("Access is denied."));
        assert!(contains_elevation_indicator("access denied"));
        assert!(contains_elevation_indicator("Error: Access is denied"));
    }

    #[test]
    fn test_elevation_chinese() {
        assert!(contains_elevation_indicator("需要提升权限"));
        assert!(contains_elevation_indicator("权限提升"));
    }

    #[test]
    fn test_elevation_keywords() {
        assert!(contains_elevation_indicator("elevation required"));
        assert!(contains_elevation_indicator("requires elevated privileges"));
        assert!(contains_elevation_indicator("run as administrator"));
    }

    #[test]
    fn test_elevation_false_positives() {
        // 超时、端口占用等不应被识别为权限错误
        assert!(!contains_elevation_indicator("setupc.exe execution timed out"));
        assert!(!contains_elevation_indicator("PortName in use"));
        assert!(!contains_elevation_indicator("already exists"));
        assert!(!contains_elevation_indicator("already logged"));
        assert!(!contains_elevation_indicator(""));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_is_elevation_output_with_output() {
        use std::os::windows::process::ExitStatusExt;
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(5),
            stdout: Vec::new(),
            stderr: b"Access is denied.\r\n".to_vec(),
        };
        assert!(is_elevation_output(&output));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_is_elevation_output_clean_output() {
        use std::os::windows::process::ExitStatusExt;
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: b"PortName COM22 already logged\r\n".to_vec(),
        };
        assert!(!is_elevation_output(&output));
    }
}
