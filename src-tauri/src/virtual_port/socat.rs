//! SocatBackend — Linux/macOS virtual serial port backend
//!
//! Uses the socat user-space tool to create interconnected PTY pairs,
//! providing the same bidirectional data bridge model as com0com on Windows.
//!
//! ## How it works
//!
//! Each port pair is created by spawning a background socat process:
//! ```text
//! socat PTY,link=/tmp/tauterm_vport_A0,mode=666 PTY,link=/tmp/tauterm_vport_B0,mode=666
//! ```
//!
//! socat creates two PTYs (e.g., `/dev/pts/5` and `/dev/pts/6`) and
//! symlinks at the specified paths. TauTerm opens port A while external
//! tools open port B — data flows bidirectionally through socat.
//!
//! ## Platform support
//!
//! - Linux: primary target (socat is available via apt)
//! - macOS: supported when socat is installed (e.g., via Homebrew)

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::backend::VirtualPortBackend;
use super::manager::{PortPair, VirtualPortConfig};

/// Symlink prefix for virtual port pairs in `/tmp`.
const VPORT_SYMLINK_PREFIX: &str = "tauterm_vport_";

/// Time to wait (ms) for socat to create PTY symlinks after spawning.
const SOCAT_PTY_WAIT_MS: u64 = 50;
/// Max retry attempts for PTY symlink discovery.
const SOCAT_PTY_MAX_RETRIES: u32 = 20;

/// How many port pairs to support (matches com0com's 1–4 range).
const MAX_PAIR_COUNT: u32 = 4;

/// A tracked socat process for one port pair.
struct SocatProcess {
    /// The socat child process handle.
    child: Child,
    /// Symlink path for port A (e.g., `/tmp/tauterm_vport_A0`).
    symlink_a: PathBuf,
    /// Symlink path for port B (e.g., `/tmp/tauterm_vport_B0`).
    symlink_b: PathBuf,
    /// The ID number used in the symlink names.
    id: u32,
}

pub struct SocatBackend {
    /// Active port pairs keyed by ID number.
    processes: HashMap<u32, SocatProcess>,
    /// Tracked symlink paths for orphan cleanup.
    symlink_paths: HashSet<PathBuf>,
    /// Monotonically increasing ID for new port pairs.
    next_id: u32,
}

impl SocatBackend {
    pub fn new() -> Self {
        // 扫描 /tmp 中已有符号链接，从 max_id + 1 开始分配，
        // 避免与同时运行的其他 TauTerm 实例发生 ID 碰撞。
        let next_id = Self::scan_max_existing_id().unwrap_or(0);
        Self {
            processes: HashMap::new(),
            symlink_paths: HashSet::new(),
            next_id,
        }
    }

    // ── Helpers ───────────────────────────────────────────────

    /// 扫描 `/tmp` 中 `tauterm_vport_*` 符号链接，提取最大 ID 值。
    /// 返回 `max_id + 1` 作为下一个可用 ID，若无已有符号链接则返回 0。
    fn scan_max_existing_id() -> Option<u32> {
        let dir = std::fs::read_dir("/tmp").ok()?;
        let mut max_id: Option<u32> = None;
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // 格式: tauterm_vport_A{id} 或 tauterm_vport_B{id}
            if let Some(rest) = name.strip_prefix(VPORT_SYMLINK_PREFIX) {
                // 跳过 'A' 或 'B' 前缀字符，提取数字部分
                let num_str: String = rest.chars().skip(1).collect();
                if let Ok(num) = num_str.parse::<u32>() {
                    max_id = Some(max_id.map_or(num, |m| m.max(num)));
                }
            }
        }
        // 从 max_id + 1 开始，但保留 0 作为无已有符号链接时的回退
        max_id.map(|m| m + 1)
    }

    /// Check if `socat` binary is available in PATH.
    fn socat_available() -> bool {
        Command::new("socat")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Generate symlink paths for a port pair with the given ID.
    fn symlink_paths(id: u32) -> (PathBuf, PathBuf) {
        let a = PathBuf::from(format!("/tmp/{}A{}", VPORT_SYMLINK_PREFIX, id));
        let b = PathBuf::from(format!("/tmp/{}B{}", VPORT_SYMLINK_PREFIX, id));
        (a, b)
    }

    /// Resolve a symlink to its target PTY path (e.g., `/tmp/tauterm_vport_A0` → `/dev/pts/5`).
    fn resolve_pty(symlink: &PathBuf) -> Option<String> {
        std::fs::read_link(symlink)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Wait for socat to create the symlinks, with retries.
    fn wait_for_symlinks(symlink_a: &PathBuf, symlink_b: &PathBuf) -> bool {
        for _ in 0..SOCAT_PTY_MAX_RETRIES {
            if symlink_a.exists() && symlink_b.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(SOCAT_PTY_WAIT_MS));
        }
        false
    }

    /// Kill a socat process and remove its symlinks.
    fn kill_and_cleanup(proc: &mut SocatProcess) {
        // Try SIGTERM first, then SIGKILL
        let pid = proc.child.id();
        if let Err(e) = Command::new("kill")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            log::debug!("Failed to kill socat PID {}: {}", pid, e);
        }

        // Give it a moment to exit
        std::thread::sleep(Duration::from_millis(100));

        // Force kill if still running
        let _ = proc.child.kill();
        let _ = proc.child.wait();

        // Remove symlinks (best effort)
        let _ = std::fs::remove_file(&proc.symlink_a);
        let _ = std::fs::remove_file(&proc.symlink_b);
    }
}

impl VirtualPortBackend for SocatBackend {
    fn are_files_present(&self) -> bool {
        Self::socat_available()
    }

    fn detect_driver(&self) -> bool {
        // socat is a user-space tool — no kernel driver to detect
        Self::socat_available()
    }

    fn install_driver(&mut self) -> Result<(), String> {
        if Self::socat_available() {
            log::info!("socat is already installed");
            Ok(())
        } else {
            Err("socat is not installed. Install it via: sudo apt install socat".into())
        }
    }

    fn install_driver_elevated(&mut self) -> Result<(), String> {
        // socat doesn't need elevated privileges to install
        self.install_driver()
    }

    fn create_pairs(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        if !Self::socat_available() {
            return Err("socat is not installed. Install it via: sudo apt install socat".into());
        }

        let count = config.count.clamp(1, MAX_PAIR_COUNT);
        let mut pairs = Vec::new();

        for _ in 0..count {
            let id = self.next_id;
            self.next_id += 1;

            let (symlink_a, symlink_b) = Self::symlink_paths(id);

            // Clean up any stale symlinks from previous runs
            let _ = std::fs::remove_file(&symlink_a);
            let _ = std::fs::remove_file(&symlink_b);

            // Spawn socat: creates two interconnected PTYs with symlinks
            let mut child = Command::new("socat")
                .args([
                    &format!("PTY,link={},mode=666", symlink_a.display()),
                    &format!("PTY,link={},mode=666", symlink_b.display()),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn socat: {}", e))?;

            // Wait for symlinks to appear
            if !Self::wait_for_symlinks(&symlink_a, &symlink_b) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&symlink_a);
                let _ = std::fs::remove_file(&symlink_b);
                return Err(format!(
                    "socat PTY symlinks did not appear within {}ms for pair {}",
                    SOCAT_PTY_WAIT_MS * SOCAT_PTY_MAX_RETRIES as u64,
                    id
                ));
            }

            // Resolve symlinks to actual PTY device paths
            let port_a = Self::resolve_pty(&symlink_a)
                .unwrap_or_else(|| symlink_a.to_string_lossy().to_string());
            let port_b = Self::resolve_pty(&symlink_b)
                .unwrap_or_else(|| symlink_b.to_string_lossy().to_string());

            // Drain stderr to prevent pipe buffer from blocking socat
            // (socat logs PTY paths to stderr; we consume them in a background thread
            // so the buffer doesn't fill and block the process)
            let stderr = child.stderr.take();
            if let Some(stderr) = stderr {
                std::thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().flatten() {
                        log::debug!("socat[id={}]: {}", id, line);
                    }
                });
            }

            log::info!(
                "socat port pair created: {} ↔ {} (id={})",
                port_a, port_b, id
            );

            self.symlink_paths.insert(symlink_a.clone());
            self.symlink_paths.insert(symlink_b.clone());

            self.processes.insert(
                id,
                SocatProcess {
                    child,
                    symlink_a,
                    symlink_b,
                    id,
                },
            );

            pairs.push(PortPair {
                port_a,
                port_b,
                bus_number: id,
            });
        }

        Ok(pairs)
    }

    fn create_pairs_elevated(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        // socat runs in user space — no elevation needed
        self.create_pairs(config)
    }

    fn destroy_pair(&mut self, pair: &PortPair) -> Result<(), String> {
        let id = pair.bus_number;
        if let Some(mut proc) = self.processes.remove(&id) {
            Self::kill_and_cleanup(&mut proc);
            self.symlink_paths.remove(&proc.symlink_a);
            self.symlink_paths.remove(&proc.symlink_b);
            log::info!(
                "socat port pair destroyed: {} ↔ {} (id={})",
                pair.port_a, pair.port_b, id
            );
        } else {
            // Port pair already gone — just clean up symlinks if they exist
            let (symlink_a, symlink_b) = Self::symlink_paths(id);
            let _ = std::fs::remove_file(&symlink_a);
            let _ = std::fs::remove_file(&symlink_b);
            log::info!(
                "socat port pair already gone: {} ↔ {} (id={})",
                pair.port_a, pair.port_b, id
            );
        }
        Ok(())
    }

    fn cleanup_all(&mut self) {
        let ids: Vec<u32> = self.processes.keys().copied().collect();
        for id in ids {
            if let Some(mut proc) = self.processes.remove(&id) {
                Self::kill_and_cleanup(&mut proc);
                self.symlink_paths.remove(&proc.symlink_a);
                self.symlink_paths.remove(&proc.symlink_b);
            }
        }
        log::info!("socat: all port pairs cleaned up");
    }

    fn cleanup_orphans(&mut self) -> u32 {
        let mut cleaned = 0u32;

        // Scan /tmp for tauterm_vport_* symlinks
        let dir = match std::fs::read_dir("/tmp") {
            Ok(d) => d,
            Err(e) => {
                log::warn!("socat: cannot read /tmp for orphan cleanup: {}", e);
                return 0;
            }
        };

        for entry in dir.flatten() {
            let path = entry.path();
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Only process files matching our prefix
            if !file_name.starts_with(VPORT_SYMLINK_PREFIX) {
                continue;
            }

            // Check if this symlink is already tracked
            if self.symlink_paths.contains(&path) {
                continue;
            }

            // 仅处理符号链接；普通文件和目录不应被删除
            if !path.is_symlink() {
                log::debug!(
                    "socat: skipping non-symlink entry {:?} (file_type check)",
                    path
                );
                continue;
            }

            // Check if the symlink target still exists (PTY may be gone)
            let is_stale = match std::fs::read_link(&path) {
                Ok(target) => !target.exists(),
                Err(_) => {
                    // 符号链接已确认，read_link 失败仅为权限问题 — 安全删除
                    true
                }
            };

            if is_stale {
                if let Err(e) = std::fs::remove_file(&path) {
                    log::debug!("socat: failed to remove stale symlink {:?}: {}", path, e);
                } else {
                    log::info!("socat: cleaned up stale symlink {:?}", path);
                    cleaned += 1;
                }
            }
        }

        if cleaned > 0 {
            log::info!("socat: cleaned up {} stale symlinks", cleaned);
        }
        cleaned
    }

    fn cleanup_pairs_elevated(&mut self) -> Result<u32, String> {
        // socat runs in user space — no elevation needed
        Ok(self.cleanup_orphans())
    }

    fn pending_orphan_count(&self) -> u32 {
        // 统计 /tmp 中未被跟踪且目标已失效的过期符号链接。
        // 对齐 cleanup_orphans 的 staleness 检测逻辑，确保前端显示的计数
        // 与实际可清理数量一致。
        let dir = match std::fs::read_dir("/tmp") {
            Ok(d) => d,
            Err(_) => return 0,
        };

        dir.flatten()
            .filter(|entry| {
                let path = entry.path();
                let file_name = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();
                file_name.starts_with(VPORT_SYMLINK_PREFIX)
                    && !self.symlink_paths.contains(&path)
                    && path.is_symlink()
                    && std::fs::read_link(&path)
                        .map(|target| !target.exists())
                        .unwrap_or(true)
            })
            .count() as u32
    }
}

// ── Drop ────────────────────────────────────────────────────────

impl Drop for SocatBackend {
    /// 应用退出时（含 panic/unwind 路径）清理所有 socat 子进程和符号链接。
    /// 若不实现此 trait，`std::process::Child` 默认 drop 不会 kill 子进程，
    /// socat 进程将变为孤儿僵尸并持续占用 PTY 资源。
    fn drop(&mut self) {
        self.cleanup_all();
    }
}
