//! Local Shell 协议插件。
//!
//! 负责配置验证、系统 Shell 探测与 PTY 通道创建。平台 PTY/进程生命周期
//! 细节封装在 `LocalShellChannel`，调用方只接触 `ProtocolAdapter` interface。

pub const PLUGIN_ID: &str = "local-shell";

pub(crate) mod commands;

pub(crate) fn session_config_handler() -> crate::plugin_application::SessionConfigHandler {
    crate::plugin_application::SessionConfigHandler {
        validate: Some(LocalShellAdapter::validate_params),
        prepare: crate::plugin_application::unchanged_session_config,
        default_name: Some(|params, _endpoint| LocalShellAdapter::default_session_name(params)),
        sanitize_saved: None,
        delete: None,
    }
}

#[tauri::command]
pub fn resolve_local_shell_session_name(params: serde_json::Value) -> Result<String, String> {
    LocalShellAdapter::default_session_name(&params)
}

#[cfg(windows)]
pub(crate) mod conpty_engine;
mod driver;
#[cfg(windows)]
pub(crate) mod elevated;

use crate::kernel::plugin_adapter::{
    ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionChannelFactory,
};
use crate::session::SessionError;
use crate::transport::DataPlaneRuntime;
use driver::LocalShellDriver;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Stdio;

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_LENGTH: usize = 4096;
const MAX_ENV_VARS: usize = 64;
const MAX_ENV_VAR_LENGTH: usize = 8192;

/// One environment variable injected into the spawned shell process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalShellEnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShellConfig {
    #[serde(default = "default_shell_mode")]
    pub shell_mode: String,
    /// 空字符串表示 Auto。
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// 由发现到的 Shell 预设管理，不在“用户参数”列表中展示。
    #[serde(default)]
    pub preset_args: Vec<String>,
    #[serde(default)]
    pub preset_id: String,
    #[serde(default)]
    pub shell_label: String,
    #[serde(default = "default_shell_kind")]
    pub shell_kind: String,
    #[serde(default)]
    pub wsl_distro: String,
    /// 空字符串表示当前用户主目录。
    #[serde(default)]
    pub cwd: String,
    /// 注入到 shell 进程的环境变量；不经过 shell 语法解析。
    #[serde(default)]
    pub env: Vec<LocalShellEnvVar>,
}

impl Default for LocalShellConfig {
    fn default() -> Self {
        Self {
            shell_mode: default_shell_mode(),
            executable: String::new(),
            args: vec![],
            preset_args: vec![],
            preset_id: String::new(),
            shell_label: String::new(),
            shell_kind: default_shell_kind(),
            wsl_distro: String::new(),
            cwd: String::new(),
            env: vec![],
        }
    }
}

impl LocalShellConfig {
    pub fn validate_for_save(&self) -> Result<(), String> {
        if !matches!(self.shell_mode.as_str(), "auto" | "path" | "custom") {
            return Err("Local shell mode must be auto, path, or custom".into());
        }
        if !matches!(self.shell_kind.as_str(), "native" | "wsl" | "custom") {
            return Err("Local shell kind is invalid".into());
        }
        if self.executable.contains('\0') || self.cwd.contains('\0') {
            return Err("Shell executable and working directory cannot contain NUL".into());
        }
        if self.args.len() + self.preset_args.len() > MAX_ARGUMENTS {
            return Err(format!(
                "Too many shell arguments (maximum {MAX_ARGUMENTS})"
            ));
        }
        if self
            .args
            .iter()
            .chain(self.preset_args.iter())
            .any(|arg| arg.contains('\0') || arg.len() > MAX_ARGUMENT_LENGTH)
        {
            return Err(format!(
                "Shell arguments cannot contain NUL or exceed {MAX_ARGUMENT_LENGTH} bytes"
            ));
        }

        if self.wsl_distro.contains('\0') || self.wsl_distro.len() > MAX_ARGUMENT_LENGTH {
            return Err("Linux subsystem distribution name is invalid".into());
        }

        if !self.env.is_empty() {
            if self.shell_kind == "wsl" {
                return Err("Environment variables are not supported for WSL shells".into());
            }
            if self.env.len() > MAX_ENV_VARS {
                return Err(format!(
                    "Too many environment variables (maximum {MAX_ENV_VARS})"
                ));
            }
            for entry in &self.env {
                if entry.key.is_empty()
                    || entry.key.len() > MAX_ENV_VAR_LENGTH
                    || !entry
                        .key
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    return Err(
                        "Environment variable names may only contain A-Z, a-z, 0-9 and _".into(),
                    );
                }
                if entry.value.contains('\0') || entry.value.len() > MAX_ENV_VAR_LENGTH {
                    return Err(format!(
                        "Environment variable values cannot contain NUL or exceed {MAX_ENV_VAR_LENGTH} bytes"
                    ));
                }
            }
        }

        if self.shell_kind == "wsl" {
            validate_wsl_path_syntax(self.cwd.trim())?;
        } else if !self.cwd.trim().is_empty() {
            let cwd = Path::new(self.cwd.trim());
            if !cwd.exists() {
                return Err("Local shell working directory does not exist".into());
            }
            if !cwd.is_dir() {
                return Err("Local shell working directory is not a directory".into());
            }
        }

        let executable = self.executable.trim();
        if matches!(self.shell_mode.as_str(), "path" | "custom") && executable.is_empty() {
            return Err("A selected or custom shell executable is required".into());
        }
        if !executable.is_empty() && resolve_executable(executable).is_none() {
            return Err("Local shell executable was not found".into());
        }
        Ok(())
    }

    fn resolve(&self) -> Result<ResolvedLocalShellConfig, SessionError> {
        self.validate_for_save()
            .map_err(SessionError::InvalidParameter)?;

        let executable = if self.executable.trim().is_empty() {
            detect_default_shell().ok_or_else(|| SessionError::ConnectionFailed {
                reason: "No supported local shell was found".into(),
            })?
        } else {
            resolve_executable(self.executable.trim()).ok_or_else(|| {
                SessionError::ConnectionFailed {
                    reason: format!("Local shell executable was not found: {}", self.executable),
                }
            })?
        };

        let host_home = || {
            directories::UserDirs::new()
                .map(|dirs| dirs.home_dir().to_path_buf())
                .or_else(|| std::env::current_dir().ok())
                .ok_or_else(|| SessionError::ConnectionFailed {
                    reason: "Unable to resolve the user home directory".into(),
                })
        };

        let (cwd, args, wsl_cwd_probe) = if self.shell_kind == "wsl" {
            #[cfg(not(windows))]
            return Err(SessionError::ConnectionFailed {
                reason: "Linux subsystem sessions are not available on this platform".into(),
            });

            #[cfg(windows)]
            {
                let wsl_cwd = if self.cwd.trim().is_empty() {
                    "~"
                } else {
                    self.cwd.trim()
                };
                let mut args = self.preset_args.clone();
                args.push("--cd".into());
                args.push(wsl_cwd.into());
                args.extend(self.args.clone());
                // `resolve()` 同时服务于默认名称生成，必须保持纯计算（不启动任何进程）。
                // WSL 工作目录的实际存在性由 `connect_with_mode` 在建立 PTY 前用一次
                // 隐藏探针确认，原因见 `WslCwdProbe`。
                let probe = WslCwdProbe {
                    executable: executable.clone(),
                    preset_args: self.preset_args.clone(),
                    cwd: wsl_cwd.to_string(),
                };
                (host_home()?, args, Some(probe))
            }
        } else {
            let cwd = if self.cwd.trim().is_empty() {
                host_home()
            } else {
                Ok(PathBuf::from(self.cwd.trim()))
            }?;
            let mut args = self.preset_args.clone();
            args.extend(self.args.clone());
            (cwd, args, None)
        };
        if !cwd.is_dir() {
            return Err(SessionError::ConnectionFailed {
                reason: format!(
                    "Local shell working directory does not exist: {}",
                    cwd.display()
                ),
            });
        }

        let label = if self.shell_label.trim().is_empty() {
            shell_display_label(&executable)
        } else {
            self.shell_label.trim().to_string()
        };

        Ok(ResolvedLocalShellConfig {
            executable,
            args,
            cwd,
            env: self.env.clone(),
            label,
            wsl_cwd_probe,
        })
    }
}

fn default_shell_mode() -> String {
    "auto".into()
}

fn default_shell_kind() -> String {
    "native".into()
}

#[derive(Clone)]
struct ResolvedLocalShellConfig {
    executable: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<LocalShellEnvVar>,
    label: String,
    /// WSL 会话在建立 PTY 前必须执行的一次隐藏工作目录校验；其它 Shell 为 `None`。
    wsl_cwd_probe: Option<WslCwdProbe>,
}

/// WSL 工作目录探针的输入。
///
/// `wsl.exe --cd <dir>` 对**不存在**的目录不会失败：实测它只在 stderr 报一行
/// `chdir(...) failed`，然后在 `/` 里启动进程并返回 0。所以“让 `--cd` 自己失败”
/// 不能替代校验，否则用户会静默拿到一个住在 `/` 的会话。校验必须显式做，但
/// 它只允许在真正连接时发生一次，并且必须隐藏启动（见 `kernel::process`）。
#[derive(Clone)]
#[cfg_attr(not(windows), allow(dead_code))] // only read by the Windows probe
struct WslCwdProbe {
    executable: PathBuf,
    preset_args: Vec<String>,
    cwd: String,
}

fn shell_display_label(executable: &Path) -> String {
    let stem = executable
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Shell");
    match stem.to_ascii_lowercase().as_str() {
        "pwsh" => "PowerShell".into(),
        "powershell" => "Windows PowerShell".into(),
        "cmd" => "Command Prompt".into(),
        "wsl" => "WSL".into(),
        "nu" => "Nushell".into(),
        _ => stem.to_string(),
    }
}

pub struct LocalShellAdapter;

impl LocalShellAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn validate_params(params: &serde_json::Value) -> Result<(), String> {
        let config: LocalShellConfig = serde_json::from_value(params.clone())
            .map_err(|error| format!("Invalid local shell configuration: {error}"))?;
        config.validate_for_save()
    }

    pub fn default_session_name(params: &serde_json::Value) -> Result<String, String> {
        let config: LocalShellConfig = serde_json::from_value(params.clone())
            .map_err(|error| format!("Invalid local shell configuration: {error}"))?;
        let resolved = config.resolve().map_err(|error| error.to_string())?;
        Ok(format!("Shell @ {}", resolved.label))
    }

    pub async fn connect_with_mode(
        &self,
        params: &serde_json::Value,
        mode: ChannelOpenMode,
    ) -> Result<ProtocolConnection, SessionError> {
        let config: LocalShellConfig = serde_json::from_value(params.clone()).map_err(|error| {
            SessionError::ConnectionFailed {
                reason: format!("Invalid local shell configuration: {error}"),
            }
        })?;
        let resolved = config.resolve()?;
        if let Some(probe) = resolved.wsl_cwd_probe.clone() {
            check_wsl_working_directory_blocking(probe).await?;
        }
        let elevated_supported = cfg!(windows) && config.shell_kind != "wsl";
        let factory = std::sync::Arc::new(LocalShellFactory {
            resolved,
            elevated_supported,
        });
        if !factory.supports_mode(mode) {
            return Err(SessionError::CapabilityDenied {
                capability: "elevated_shell".into(),
            });
        }
        let data_plane = factory.open_channel(mode).await?;
        Ok(ProtocolConnection {
            data_plane: Some(data_plane),
            service: None,
            file_transfer: None,
            channel_factory: Some(factory),
            on_attached: None,
            teardown_delay: std::time::Duration::ZERO,
        })
    }
}

#[derive(Clone)]
struct LocalShellFactory {
    resolved: ResolvedLocalShellConfig,
    elevated_supported: bool,
}

#[async_trait::async_trait]
impl SessionChannelFactory for LocalShellFactory {
    async fn open_channel(&self, mode: ChannelOpenMode) -> Result<DataPlaneRuntime, SessionError> {
        let executable = self.resolved.executable.to_string_lossy().to_string();
        match mode {
            ChannelOpenMode::Standard => {
                let channel = LocalShellDriver::spawn(
                    &executable,
                    &self.resolved.args,
                    &self.resolved.cwd,
                    &self.resolved.env,
                )
                .map_err(SessionError::Io)?;
                Ok(DataPlaneRuntime::spawn(Box::new(channel)))
            }
            ChannelOpenMode::Elevated if self.elevated_supported => {
                #[cfg(windows)]
                {
                    let driver = elevated::ElevatedShellDriver::spawn(
                        &executable,
                        &self.resolved.args,
                        &self.resolved.cwd,
                        &self.resolved.env,
                    )
                    .map_err(SessionError::Io)?;
                    Ok(DataPlaneRuntime::spawn(Box::new(driver)))
                }
                #[cfg(not(windows))]
                {
                    Err(SessionError::CapabilityDenied {
                        capability: "elevated_shell".into(),
                    })
                }
            }
            ChannelOpenMode::Elevated => Err(SessionError::CapabilityDenied {
                capability: "elevated_shell".into(),
            }),
        }
    }

    fn supports_mode(&self, mode: ChannelOpenMode) -> bool {
        mode == ChannelOpenMode::Standard
            || (mode == ChannelOpenMode::Elevated && self.elevated_supported)
    }

    fn child_name_prefix(&self) -> &'static str {
        "Shell"
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for LocalShellAdapter {
    fn plugin_id(&self) -> Option<&'static str> {
        Some(PLUGIN_ID)
    }

    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        self.connect_with_mode(params, ChannelOpenMode::Standard)
            .await
    }

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(detect_shell_presets()
            .into_iter()
            .map(ShellPreset::into_endpoint)
            .collect())
    }
}

fn detect_default_shell() -> Option<PathBuf> {
    #[cfg(unix)]
    if let Some(shell) = std::env::var_os("SHELL") {
        if let Some(path) = resolve_executable(&shell.to_string_lossy()) {
            return Some(path);
        }
    }

    platform_shell_candidates()
        .into_iter()
        .find_map(resolve_executable)
}

#[derive(Debug, Clone)]
struct ShellPreset {
    id: String,
    executable: PathBuf,
    label: String,
    preset_args: Vec<String>,
    kind: &'static str,
    wsl_distro: String,
}

impl ShellPreset {
    fn native(id: &str, executable: PathBuf, label: &str) -> Self {
        Self {
            id: id.into(),
            executable,
            label: label.into(),
            preset_args: vec![],
            kind: "native",
            wsl_distro: String::new(),
        }
    }

    #[cfg(windows)]
    fn wsl(id: String, executable: PathBuf, label: String, distro: String) -> Self {
        let preset_args = if distro.is_empty() {
            vec![]
        } else {
            vec!["--distribution".into(), distro.clone()]
        };
        Self {
            id,
            executable,
            label,
            preset_args,
            kind: "wsl",
            wsl_distro: distro,
        }
    }

    fn into_endpoint(self) -> EndpointInfo {
        let executable = self.executable.to_string_lossy().to_string();
        EndpointInfo {
            name: self.id.clone(),
            description: self.label.clone(),
            params: Some(serde_json::json!({
                "preset_id": self.id,
                "shell_mode": "path",
                "executable": executable,
                "preset_args": self.preset_args,
                "shell_label": self.label,
                "shell_kind": self.kind,
                "wsl_distro": self.wsl_distro,
            })),
        }
    }
}

fn detect_shell_presets() -> Vec<ShellPreset> {
    #[cfg(windows)]
    let candidates = detect_windows_shell_presets();
    #[cfg(not(windows))]
    let candidates = detect_unix_shell_presets();

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|preset| {
            let key = format!(
                "{}\u{1f}{}",
                normalize_path_key(&preset.executable),
                preset.preset_args.join("\u{1f}")
            );
            seen.insert(key)
        })
        .collect()
}

#[cfg(windows)]
fn detect_windows_shell_presets() -> Vec<ShellPreset> {
    let mut presets = Vec::new();

    push_resolved_native(&mut presets, "powershell-core", "pwsh.exe", "PowerShell");
    push_resolved_native(
        &mut presets,
        "windows-powershell",
        "powershell.exe",
        "Windows PowerShell",
    );
    push_resolved_native(&mut presets, "cmd", "cmd.exe", "Command Prompt");

    if let Some(wsl) = resolve_executable("wsl.exe") {
        for distro in detect_wsl_distributions(&wsl) {
            presets.push(ShellPreset::wsl(
                format!("wsl-distro:{distro}"),
                wsl.clone(),
                format!("WSL · {distro}"),
                distro,
            ));
        }
    }

    if let Some(path) = first_existing_path(&git_bash_candidates()) {
        presets.push(ShellPreset::native("git-bash", path, "Git Bash"));
    }
    if let Some(path) = first_existing_path(&msys2_bash_candidates()) {
        presets.push(ShellPreset::native("msys2-bash", path, "MSYS2 Bash"));
    }
    if let Some(path) = first_existing_path(&cygwin_bash_candidates()) {
        presets.push(ShellPreset::native("cygwin-bash", path, "Cygwin Bash"));
    }
    push_resolved_native(&mut presets, "nushell", "nu.exe", "Nushell");

    presets
}

#[cfg(windows)]
fn push_resolved_native(presets: &mut Vec<ShellPreset>, id: &str, executable: &str, label: &str) {
    if let Some(path) = resolve_executable(executable) {
        presets.push(ShellPreset::native(id, path, label));
    }
}

#[cfg(windows)]
fn first_existing_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

#[cfg(windows)]
fn git_bash_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
    ]
    .into_iter()
    .flatten()
    {
        paths.push(PathBuf::from(root).join("Git").join("bin").join("bash.exe"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        paths.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    paths
}

#[cfg(windows)]
fn msys2_bash_candidates() -> Vec<PathBuf> {
    vec![PathBuf::from(r"C:\msys64\usr\bin\bash.exe")]
}

#[cfg(windows)]
fn cygwin_bash_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\cygwin64\bin\bash.exe"),
        PathBuf::from(r"C:\cygwin\bin\bash.exe"),
    ]
}

fn validate_wsl_path_syntax(path: &str) -> Result<(), String> {
    if path.is_empty() || path == "~" || path.starts_with("~/") || path.starts_with('/') {
        return Ok(());
    }
    Err("Linux subsystem working directory must be an absolute Linux path or start with ~/".into())
}

#[cfg(windows)]
fn detect_wsl_distributions(wsl: &Path) -> Vec<String> {
    // WSL 服务异常、首次初始化或发行版注册损坏时，`wsl --list` 可能长时间
    // 不返回。端点发现只用于配置 UI，不允许它成为打开会话配置页的无界阻塞点。
    const DISCOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

    let mut child = match crate::kernel::process::hidden_command(wsl)
        .args(["--list", "--quiet"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            log::warn!("Unable to enumerate Linux subsystem distributions: {error}");
            return vec![];
        }
    };

    let deadline = std::time::Instant::now() + DISCOVERY_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                log::warn!(
                    "Linux subsystem distribution discovery exceeded {:?}; skipping presets",
                    DISCOVERY_TIMEOUT
                );
                let _ = child.kill();
                let _ = child.wait();
                return vec![];
            }
            Err(error) => {
                log::warn!("Unable to poll Linux subsystem discovery: {error}");
                let _ = child.kill();
                let _ = child.wait();
                return vec![];
            }
        }
    };

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => {
            log::warn!("Unable to collect Linux subsystem discovery output: {error}");
            return vec![];
        }
    };
    if !status.success() {
        log::warn!(
            "Linux subsystem distribution discovery failed: {}",
            decode_windows_output(&output.stderr)
        );
        return vec![];
    }

    let mut seen = HashSet::new();
    decode_windows_output(&output.stdout)
        .lines()
        .map(|line| line.trim().trim_matches('\0'))
        .filter(|name| !name.is_empty())
        .filter(|name| !name.eq_ignore_ascii_case("docker-desktop"))
        .filter(|name| !name.eq_ignore_ascii_case("docker-desktop-data"))
        .filter(|name| seen.insert(name.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

/// WSL 工作目录校验的超时。它只覆盖一次发行版内 `sh` 的启动，冷启动偶尔需要
/// 数秒，因此比端点发现（1.5s，纯列表）宽得多。
#[cfg(windows)]
const WSL_CWD_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(windows)]
async fn check_wsl_working_directory_blocking(probe: WslCwdProbe) -> Result<(), SessionError> {
    tauri::async_runtime::spawn_blocking(move || check_wsl_working_directory(&probe))
        .await
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("Linux subsystem working directory check task failed: {error}"),
        })?
}

#[cfg(not(windows))]
async fn check_wsl_working_directory_blocking(_probe: WslCwdProbe) -> Result<(), SessionError> {
    Ok(())
}

/// 探针在目标目录不可用（不存在 / 无访问权限）时返回的专用退出码。
#[cfg_attr(not(windows), allow(dead_code))]
const WSL_CWD_PROBE_DIR_UNAVAILABLE: i32 = 42;

/// 在 WSL 内确认目标工作目录可用。
///
/// 为什么不是"让 `--cd` 自己失败"：实测 `wsl.exe --cd <missing>` **返回 0**，只在
/// stderr 报一行 `chdir(...) failed`，然后把进程放进 `/`（或默认家目录）。退出码
/// 不可信，`$PWD` 与请求值的比较又要处理符号链接/尾斜杠/`~` 展开，容易漏判。
///
/// 因此探针改为**直接执行与真实会话完全相同的动作**：在同一个发行版、同一用户下
/// `cd` 到目标目录。`cd` 成功就等于 `--cd` 会成功（同一个 chdir），失败则给出
/// 专用退出码。`~` 与 `~/…` 在脚本内用 `$HOME` 展开，路径以位置参数传入，
/// 不参与任何 shell 语法拼接。
#[cfg(windows)]
const WSL_CWD_PROBE_SCRIPT: &str = r#"case "$1" in
  "~") p="$HOME" ;;
  "~/"*) p="$HOME/${1#~/}" ;;
  *) p="$1" ;;
esac
if cd "$p" 2>/dev/null; then printf %s "$PWD"; else exit 42; fi"#;

#[cfg(windows)]
fn check_wsl_working_directory(probe: &WslCwdProbe) -> Result<(), SessionError> {
    let mut child = crate::kernel::process::hidden_command(&probe.executable)
        .args(&probe.preset_args)
        .args([
            "--exec",
            "/bin/sh",
            "-c",
            WSL_CWD_PROBE_SCRIPT,
            "sh",
            probe.cwd.as_str(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("Unable to validate Linux subsystem working directory: {error}"),
        })?;

    // 输出收集也必须有时限：`try_wait` 只证明 `wsl.exe` 退出了，若某个继承管道写端
    // 的 WSL 子进程仍存活，直接 `wait_with_output()` 会无界阻塞。
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (pipe_tx, pipe_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("wsl-cwd-probe-output".into())
        .spawn(move || {
            use std::io::Read;
            let mut out = Vec::new();
            let mut err = Vec::new();
            if let Some(mut pipe) = stdout {
                let _ = pipe.read_to_end(&mut out);
            }
            if let Some(mut pipe) = stderr {
                let _ = pipe.read_to_end(&mut err);
            }
            let _ = pipe_tx.send((out, err));
        })
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("Unable to start Linux subsystem working directory reader: {error}"),
        })?;

    let deadline = std::time::Instant::now() + WSL_CWD_PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SessionError::ConnectionFailed {
                    reason: format!(
                        "Linux subsystem working directory check timed out after {:?}: {}",
                        WSL_CWD_PROBE_TIMEOUT, probe.cwd
                    ),
                });
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SessionError::ConnectionFailed {
                    reason: format!(
                        "Unable to poll Linux subsystem working directory check: {error}"
                    ),
                });
            }
        }
    };

    let remaining = deadline
        .saturating_duration_since(std::time::Instant::now())
        .max(std::time::Duration::from_millis(50));
    let (_stdout, stderr) = match pipe_rx.recv_timeout(remaining) {
        Ok(collected) => collected,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SessionError::ConnectionFailed {
                reason: format!(
                    "Linux subsystem working directory check did not finish within {:?}: {}",
                    WSL_CWD_PROBE_TIMEOUT, probe.cwd
                ),
            });
        }
    };

    match wsl_cwd_probe_outcome(status.success(), status.code()) {
        WslCwdProbeOutcome::Reached => Ok(()),
        WslCwdProbeOutcome::Unavailable => Err(SessionError::ConnectionFailed {
            reason: format!(
                "Linux subsystem working directory is unavailable: {}",
                probe.cwd
            ),
        }),
        WslCwdProbeOutcome::Failed => {
            let detail = decode_windows_output(&stderr);
            Err(SessionError::ConnectionFailed {
                reason: if detail.trim().is_empty() {
                    format!(
                        "Unable to validate Linux subsystem working directory: {}",
                        probe.cwd
                    )
                } else {
                    format!(
                        "Unable to validate Linux subsystem working directory: {} ({})",
                        probe.cwd,
                        detail.trim()
                    )
                },
            })
        }
    }
}

/// 探针退出码的语义。专用退出码 42 表示“目录不可用”，其它非零退出是真正的失败。
#[derive(Debug, PartialEq, Eq)]
enum WslCwdProbeOutcome {
    Reached,
    Unavailable,
    Failed,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn wsl_cwd_probe_outcome(success: bool, exit_code: Option<i32>) -> WslCwdProbeOutcome {
    if success {
        return WslCwdProbeOutcome::Reached;
    }
    match exit_code {
        Some(WSL_CWD_PROBE_DIR_UNAVAILABLE) => WslCwdProbeOutcome::Unavailable,
        _ => WslCwdProbeOutcome::Failed,
    }
}

#[cfg(windows)]
fn decode_windows_output(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let looks_utf16 = bytes.starts_with(&[0xff, 0xfe])
        || pairs.iter().take(32).filter(|pair| pair[1] == 0).count() >= 4;
    if !looks_utf16 {
        return String::from_utf8_lossy(bytes).into_owned();
    }

    let start = usize::from(bytes.starts_with(&[0xff, 0xfe])) * 2;
    let (pairs, _) = bytes[start..].as_chunks::<2>();
    let words = pairs
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&words)
}

#[cfg(not(windows))]
fn detect_unix_shell_presets() -> Vec<ShellPreset> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(shell) = std::env::var("SHELL") {
        candidates.push(shell);
    }
    candidates.extend(platform_shell_candidates().into_iter().map(str::to_owned));

    candidates
        .into_iter()
        .filter_map(|candidate| {
            let path = resolve_executable(&candidate)?;
            let label = shell_display_name(&path);
            let id = format!("native:{}", normalize_path_key(&path));
            Some(ShellPreset::native(&id, path, &label))
        })
        .collect()
}

#[cfg(windows)]
fn platform_shell_candidates() -> Vec<&'static str> {
    vec!["pwsh.exe", "powershell.exe", "cmd.exe"]
}

#[cfg(target_os = "macos")]
fn platform_shell_candidates() -> Vec<&'static str> {
    vec!["/bin/zsh", "/bin/sh"]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_shell_candidates() -> Vec<&'static str> {
    vec!["/bin/zsh", "/bin/bash", "/bin/sh"]
}

fn resolve_executable(candidate: &str) -> Option<PathBuf> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    let direct = PathBuf::from(candidate);
    if looks_like_path(candidate) {
        return direct.is_file().then_some(direct);
    }

    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            let requested = directory.join(candidate);
            if requested.is_file() {
                return Some(requested);
            }
            if Path::new(candidate).extension().is_none() {
                let extensions =
                    std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
                for extension in extensions.split(';').filter(|ext| !ext.is_empty()) {
                    let with_extension = directory.join(format!("{candidate}{extension}"));
                    if with_extension.is_file() {
                        return Some(with_extension);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let requested = directory.join(candidate);
            if requested.is_file() {
                return Some(requested);
            }
        }
    }
    None
}

fn looks_like_path(value: &str) -> bool {
    Path::new(value).is_absolute() || value.contains('/') || value.contains('\\')
}

fn normalize_path_key(path: &Path) -> String {
    let value = path.to_string_lossy().to_string();
    #[cfg(windows)]
    return value.to_lowercase();
    #[cfg(not(windows))]
    value
}

#[cfg(not(windows))]
fn shell_display_name(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `wsl.exe --cd` 的静默回退是这条链路唯一不能靠 `--cd` 退出码发现的失败，
    /// 因此探针改为在 WSL 内直接 `cd` 到目标目录，并把退出码语义锁成契约。
    #[test]
    fn wsl_cwd_probe_maps_exit_codes_to_outcomes() {
        assert_eq!(
            wsl_cwd_probe_outcome(true, Some(0)),
            WslCwdProbeOutcome::Reached
        );
        // 目录不存在 / 无访问权限：探针脚本的专用退出码。
        assert_eq!(
            wsl_cwd_probe_outcome(false, Some(WSL_CWD_PROBE_DIR_UNAVAILABLE)),
            WslCwdProbeOutcome::Unavailable
        );
        // 发行版未注册、WSL 服务异常等：真正的失败，必须带上 stderr 细节。
        assert_eq!(
            wsl_cwd_probe_outcome(false, Some(1)),
            WslCwdProbeOutcome::Failed
        );
        assert_eq!(
            wsl_cwd_probe_outcome(false, None),
            WslCwdProbeOutcome::Failed
        );
    }

    #[test]
    fn rejects_nul_and_excessive_arguments() {
        let invalid = LocalShellConfig {
            shell_mode: "custom".into(),
            executable: "bad\0shell".into(),
            shell_kind: "custom".into(),
            ..Default::default()
        };
        assert!(invalid.validate_for_save().is_err());

        let too_many = LocalShellConfig {
            shell_mode: "auto".into(),
            args: vec!["x".into(); MAX_ARGUMENTS + 1],
            ..Default::default()
        };
        assert!(too_many.validate_for_save().is_err());
    }

    #[test]
    fn detects_at_least_one_default_shell() {
        assert!(detect_default_shell().is_some());
        assert!(!detect_shell_presets().is_empty());
    }

    #[test]
    fn accepts_independent_arguments_with_spaces() {
        let config = LocalShellConfig {
            args: vec!["argument with spaces".into()],
            ..Default::default()
        };
        assert!(config.validate_for_save().is_ok());
    }

    #[test]
    fn derives_stable_shell_labels_from_executable_names() {
        assert_eq!(shell_display_label(Path::new("pwsh.exe")), "PowerShell");
        assert_eq!(
            shell_display_label(Path::new("powershell.exe")),
            "Windows PowerShell"
        );
        assert_eq!(shell_display_label(Path::new("cmd.exe")), "Command Prompt");
        assert_eq!(
            shell_display_label(Path::new("custom-shell.exe")),
            "custom-shell"
        );
    }

    #[test]
    fn rejects_unknown_mode_and_missing_custom_executable() {
        let mut config = LocalShellConfig {
            shell_mode: "unknown".into(),
            ..Default::default()
        };
        assert!(config.validate_for_save().is_err());

        config.shell_mode = "custom".into();
        config.shell_kind = "custom".into();
        assert!(config.validate_for_save().is_err());
    }

    #[test]
    fn validates_wsl_working_directory_syntax_without_starting_wsl() {
        assert!(validate_wsl_path_syntax("").is_ok());
        assert!(validate_wsl_path_syntax("~").is_ok());
        assert!(validate_wsl_path_syntax("~/project").is_ok());
        assert!(validate_wsl_path_syntax("/home/user/project").is_ok());
        assert!(validate_wsl_path_syntax(r"C:\project").is_err());
        assert!(validate_wsl_path_syntax("relative/project").is_err());
    }

    #[test]
    fn discovered_presets_have_unique_ids_and_managed_parameters() {
        let presets = detect_shell_presets();
        let mut ids = HashSet::new();
        for preset in presets {
            assert!(
                ids.insert(preset.id.clone()),
                "duplicate preset: {}",
                preset.id
            );
            let endpoint = preset.into_endpoint();
            let params = endpoint.params.expect("preset parameters");
            assert_eq!(params["preset_id"], endpoint.name);
            assert!(params["preset_args"].is_array());
            assert!(params["executable"]
                .as_str()
                .is_some_and(|path| !path.is_empty()));
        }
    }

    #[cfg(windows)]
    #[test]
    fn wsl_presets_use_registered_distribution_names() {
        let Some(wsl) = resolve_executable("wsl.exe") else {
            return;
        };
        let distros = detect_wsl_distributions(&wsl);
        let presets = detect_windows_shell_presets();
        for distro in distros {
            let preset = presets
                .iter()
                .find(|preset| preset.id == format!("wsl-distro:{distro}"))
                .expect("registered WSL distribution preset");
            assert_eq!(preset.label, format!("WSL · {distro}"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn decodes_utf16_wsl_output() {
        let bytes = "Distro-22.04\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(decode_windows_output(&bytes), "Distro-22.04\r\n");
    }
}
