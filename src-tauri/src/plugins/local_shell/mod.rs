//! Local Shell 协议插件。
//!
//! 负责配置验证、系统 Shell 探测与 PTY 通道创建。平台 PTY/进程生命周期
//! 细节封装在 `LocalShellChannel`，调用方只接触 `ProtocolAdapter` interface。

use crate::channel::error::SessionError;
use crate::channel::local_shell_channel::LocalShellChannel;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    ChannelKind, ChannelOpenMode, EndpointInfo, PluginManifest, ProtocolAdapter,
    ProtocolConnection, SessionChannelFactory, TransferProtocolType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_LENGTH: usize = 4096;

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
        }
    }
}

impl LocalShellConfig {
    pub fn validate_for_save(&self) -> Result<(), String> {
        if !matches!(self.shell_mode.as_str(), "auto" | "path" | "custom") {
            return Err("Local shell mode must be auto, path, or custom".into());
        }
        if !matches!(self.shell_kind.as_str(), "native" | "wsl" | "custom") {
            return Err("Local shell kind must be native, wsl, or custom".into());
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
            return Err("WSL distribution name is invalid".into());
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

        let (cwd, args) = if self.shell_kind == "wsl" {
            #[cfg(not(windows))]
            return Err(SessionError::ConnectionFailed {
                reason: "WSL sessions are only available on Windows".into(),
            });

            #[cfg(windows)]
            {
                let wsl_cwd = if self.cwd.trim().is_empty() {
                    "~"
                } else {
                    self.cwd.trim()
                };
                validate_wsl_working_directory(&executable, &self.preset_args, wsl_cwd)?;
                let mut args = self.preset_args.clone();
                args.push("--cd".into());
                args.push(wsl_cwd.into());
                args.extend(self.args.clone());
                (host_home()?, args)
            }
        } else {
            let cwd = if self.cwd.trim().is_empty() {
                host_home()
            } else {
                Ok(PathBuf::from(self.cwd.trim()))
            }?;
            let mut args = self.preset_args.clone();
            args.extend(self.args.clone());
            (cwd, args)
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
            label,
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
    label: String,
}

fn shell_display_label(executable: &Path) -> String {
    let stem = executable
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Shell");
    match stem.to_ascii_lowercase().as_str() {
        "pwsh" => "PowerShell 7".into(),
        "powershell" => "Windows PowerShell".into(),
        "cmd" => "CMD".into(),
        "wsl" => "WSL".into(),
        "nu" => "NuShell".into(),
        _ => stem.to_string(),
    }
}

pub struct LocalShellAdapter;

impl LocalShellAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn manifest() -> PluginManifest {
        PluginManifest {
            id: "local-shell".into(),
            name: "Local Shell".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            description: "Local PTY shell".into(),
            icon: "ssh-shell".into(),
            content_type: "terminal".into(),
            capabilities: vec![
                "connection".into(),
                "endpoint_discovery".into(),
                "multi_session".into(),
                "elevated_session".into(),
            ],
            transfer_protocols: vec![],
        }
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
        let channel = factory.open_channel(mode).await?;
        Ok(ProtocolConnection {
            channel: Some(channel),
            comm_handle: None,
            side_channel: None,
            channel_factory: Some(factory),
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
    async fn open_channel(&self, mode: ChannelOpenMode) -> Result<ChannelKind, SessionError> {
        let executable = self.resolved.executable.to_string_lossy().to_string();
        match mode {
            ChannelOpenMode::Standard => {
                let channel =
                    LocalShellChannel::spawn(&executable, &self.resolved.args, &self.resolved.cwd)
                        .map_err(SessionError::ChannelError)?;
                Ok(ChannelKind::Sync(Box::new(channel)))
            }
            ChannelOpenMode::Elevated if self.elevated_supported => {
                #[cfg(windows)]
                {
                    let channel =
                        crate::channel::elevated_shell_channel::ElevatedShellChannel::spawn(
                            &executable,
                            &self.resolved.args,
                            &self.resolved.cwd,
                        )
                        .map_err(SessionError::ChannelError)?;
                    Ok(ChannelKind::Sync(Box::new(channel)))
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

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![]
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
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

    push_resolved_native(&mut presets, "powershell-core", "pwsh.exe", "PowerShell 7");
    push_resolved_native(
        &mut presets,
        "windows-powershell",
        "powershell.exe",
        "Windows PowerShell",
    );
    push_resolved_native(&mut presets, "cmd", "cmd.exe", "CMD");

    if let Some(wsl) = resolve_executable("wsl.exe") {
        presets.push(ShellPreset::wsl(
            "wsl-default".into(),
            wsl.clone(),
            "WSL (default distribution)".into(),
            String::new(),
        ));
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
    push_resolved_native(&mut presets, "nushell", "nu.exe", "NuShell");

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
    Err("WSL working directory must be an absolute Linux path or start with ~/".into())
}

#[cfg(windows)]
fn detect_wsl_distributions(wsl: &Path) -> Vec<String> {
    let output = match Command::new(wsl).args(["--list", "--quiet"]).output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            log::warn!(
                "WSL distribution discovery failed: {}",
                decode_windows_output(&output.stderr)
            );
            return vec![];
        }
        Err(error) => {
            log::warn!("Unable to enumerate WSL distributions: {error}");
            return vec![];
        }
    };

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

#[cfg(windows)]
fn validate_wsl_working_directory(
    executable: &Path,
    preset_args: &[String],
    cwd: &str,
) -> Result<(), SessionError> {
    let output = Command::new(executable)
        .args(preset_args)
        .args(["--cd", cwd, "--exec", "/bin/sh", "-c", "exit 0"])
        .output()
        .map_err(|error| SessionError::ConnectionFailed {
            reason: format!("Unable to validate WSL working directory: {error}"),
        })?;
    if output.status.success() {
        return Ok(());
    }

    let reason = decode_windows_output(&output.stderr);
    Err(SessionError::ConnectionFailed {
        reason: if reason.trim().is_empty() {
            format!("WSL working directory is unavailable: {cwd}")
        } else {
            format!(
                "WSL working directory is unavailable: {cwd} ({})",
                reason.trim()
            )
        },
    })
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
        assert_eq!(shell_display_label(Path::new("pwsh.exe")), "PowerShell 7");
        assert_eq!(
            shell_display_label(Path::new("powershell.exe")),
            "Windows PowerShell"
        );
        assert_eq!(shell_display_label(Path::new("cmd.exe")), "CMD");
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
    fn wsl_default_precedes_distributions_when_available() {
        if resolve_executable("wsl.exe").is_none() {
            return;
        }
        let presets = detect_shell_presets();
        let default_index = presets
            .iter()
            .position(|preset| preset.id == "wsl-default")
            .expect("WSL default preset");
        assert!(presets
            .iter()
            .skip(default_index + 1)
            .filter(|preset| preset.id.starts_with("wsl-distro:"))
            .all(|preset| preset.kind == "wsl"));
    }

    #[cfg(windows)]
    #[test]
    fn decodes_utf16_wsl_output() {
        let bytes = "Ubuntu-22.04\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(decode_windows_output(&bytes), "Ubuntu-22.04\r\n");
    }
}
