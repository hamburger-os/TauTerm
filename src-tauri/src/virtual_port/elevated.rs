//! Windows direct-UAC virtual-port executor.
//!
//! The normal GUI never runs setupc.exe. When the privileged service is unavailable (or in a
//! debug build), one explicit user action launches the current TauTerm executable in a narrow
//! one-shot helper mode. Requests travel over a random local named pipe, the helper verifies the
//! pipe server PID, derives/validates the trusted com0com resource location, and executes only
//! typed virtual-port operations.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    SetNamedPipeHandleState,
};
use windows_sys::Win32::System::Threading::{GetProcessId, TerminateProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

use super::backend::{VirtualEndpoint, VirtualPortConfig};
use super::manager::VirtualPortManager;

const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
const PIPE_TYPE_BYTE: u32 = 0;
const PIPE_READMODE_BYTE: u32 = 0;
const PIPE_WAIT: u32 = 0;
const PIPE_NOWAIT: u32 = 1;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
const ERROR_PIPE_LISTENING: u32 = 536;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const HELPER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HELPER_EXIT_TIMEOUT_MS: u32 = 120_000;
const MAX_FRAME: usize = 1024 * 1024;
const MAX_ENDPOINTS: usize = 4;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ElevatedOperation {
    EnsureDriver,
    EnsureEndpoints {
        count: u32,
        cleanup: Vec<VirtualEndpoint>,
    },
    CleanupEndpoints {
        endpoints: Vec<VirtualEndpoint>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ElevatedRequest {
    resource_dir: PathBuf,
    owner_pid: u32,
    operation: ElevatedOperation,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct ElevatedResult {
    pub(crate) endpoints: Vec<VirtualEndpoint>,
    pub(crate) cleaned_endpoints: Vec<VirtualEndpoint>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ElevatedReply {
    ok: bool,
    #[serde(default)]
    result: ElevatedResult,
    #[serde(default)]
    error: Option<String>,
}

pub(crate) fn ensure_driver(resource_dir: &Path) -> Result<(), String> {
    invoke(resource_dir, ElevatedOperation::EnsureDriver).map(|_| ())
}

pub(crate) fn ensure_endpoints(
    resource_dir: &Path,
    count: u32,
    cleanup: Vec<VirtualEndpoint>,
) -> Result<ElevatedResult, String> {
    invoke(
        resource_dir,
        ElevatedOperation::EnsureEndpoints { count, cleanup },
    )
}

pub(crate) fn cleanup_endpoints(
    resource_dir: &Path,
    endpoints: Vec<VirtualEndpoint>,
) -> Result<Vec<VirtualEndpoint>, String> {
    invoke(
        resource_dir,
        ElevatedOperation::CleanupEndpoints { endpoints },
    )
    .map(|result| result.cleaned_endpoints)
}

fn invoke(resource_dir: &Path, operation: ElevatedOperation) -> Result<ElevatedResult, String> {
    let pipe_name = format!(
        r"\\.\pipe\TauTermVirtualPort-{}",
        uuid::Uuid::new_v4().simple()
    );
    let pipe = create_server_pipe(&pipe_name)?;
    let helper = match launch_helper(&pipe_name) {
        Ok(helper) => helper,
        Err(error) => {
            unsafe { CloseHandle(pipe) };
            return Err(error);
        }
    };

    if let Err(error) = wait_for_helper_connection(pipe, helper) {
        unsafe {
            TerminateProcess(helper, 1);
            CloseHandle(helper);
            CloseHandle(pipe);
        }
        return Err(error);
    }

    let helper_pid = unsafe { GetProcessId(helper) };
    let mut client_pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut client_pid) } == 0
        || client_pid == 0
        || client_pid != helper_pid
    {
        unsafe {
            TerminateProcess(helper, 1);
            CloseHandle(helper);
            CloseHandle(pipe);
        }
        return Err("virtual-port helper pipe client identity mismatch".into());
    }

    let mut stream = unsafe { std::fs::File::from_raw_handle(pipe as RawHandle) };
    let request = ElevatedRequest {
        resource_dir: resource_dir.to_path_buf(),
        owner_pid: std::process::id(),
        operation,
    };
    let payload = serde_json::to_vec(&request)
        .map_err(|error| format!("failed to serialize virtual-port helper request: {error}"))?;
    write_frame(&mut stream, &payload).map_err(|error| error.to_string())?;
    let reply_payload = read_frame(&mut stream).map_err(|error| error.to_string())?;
    let reply: ElevatedReply = serde_json::from_slice(&reply_payload)
        .map_err(|error| format!("invalid virtual-port helper response: {error}"))?;

    let wait = unsafe { WaitForSingleObject(helper, HELPER_EXIT_TIMEOUT_MS) };
    if wait != 0 {
        unsafe { TerminateProcess(helper, 1) };
    }
    unsafe { CloseHandle(helper) };

    if reply.ok {
        Ok(reply.result)
    } else {
        Err(reply
            .error
            .unwrap_or_else(|| "virtual-port helper failed".into()))
    }
}

/// Called before Tauri startup. Returns true when this process handled a privileged request.
pub fn maybe_run_helper() -> bool {
    let mut args = std::env::args_os();
    let _exe = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--tauterm-vport-helper")) {
        return false;
    }

    let result = args
        .next()
        .ok_or_else(|| "missing virtual-port helper pipe name".to_string())
        .and_then(|pipe| run_helper(&pipe.to_string_lossy()));
    if let Err(error) = result {
        log::error!("virtual-port elevated helper failed: {error}");
    }
    true
}

fn run_helper(pipe_name: &str) -> Result<(), String> {
    let pipe = open_client_pipe(pipe_name)?;
    let mut stream = unsafe { std::fs::File::from_raw_handle(pipe as RawHandle) };

    let mut server_pid = 0u32;
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut server_pid) } == 0 || server_pid == 0 {
        return Err("failed to verify virtual-port helper pipe server".into());
    }

    let payload = read_frame(&mut stream).map_err(|error| error.to_string())?;
    let request: ElevatedRequest = serde_json::from_slice(&payload)
        .map_err(|error| format!("invalid virtual-port helper request: {error}"))?;
    if request.owner_pid != server_pid {
        return Err("virtual-port helper owner PID does not match pipe server".into());
    }

    let reply = match execute_request(request) {
        Ok(result) => ElevatedReply {
            ok: true,
            result,
            error: None,
        },
        Err(error) => ElevatedReply {
            ok: false,
            result: ElevatedResult::default(),
            error: Some(error),
        },
    };
    let payload = serde_json::to_vec(&reply)
        .map_err(|error| format!("failed to serialize virtual-port helper response: {error}"))?;
    write_frame(&mut stream, &payload).map_err(|error| error.to_string())
}

fn execute_request(request: ElevatedRequest) -> Result<ElevatedResult, String> {
    let resource_dir = validate_resource_dir(&request.resource_dir)?;
    let state_dir = super::windows_state::ensure_ownership_state_dir()?;

    let mut manager =
        VirtualPortManager::new_privileged_for_owner(resource_dir, state_dir, request.owner_pid);

    match request.operation {
        ElevatedOperation::EnsureDriver => {
            manager.install_driver()?;
            Ok(ElevatedResult::default())
        }
        ElevatedOperation::EnsureEndpoints { count, cleanup } => {
            if !(1..=MAX_ENDPOINTS as u32).contains(&count) {
                return Err(format!("invalid virtual endpoint count: {count}"));
            }
            validate_cleanup_set(&manager, &cleanup)?;

            // Create the new endpoints first. This keeps the GUI's old orphan visibility state
            // truthful if creation fails: no old endpoint has been removed behind its back.
            let endpoints = manager
                .ensure_endpoints(&VirtualPortConfig {
                    enabled: true,
                    count,
                })
                .map_err(|error| error.to_string())?;

            let mut cleaned_endpoints = Vec::new();
            for endpoint in cleanup {
                match manager.destroy_endpoint(&endpoint) {
                    Ok(()) => cleaned_endpoints.push(endpoint),
                    Err(error) => {
                        log::warn!(
                            "deferred cleanup for old direct-UAC endpoint {} (bus {}): {}",
                            endpoint.external_path,
                            endpoint.resource_id,
                            error
                        );
                    }
                }
            }

            Ok(ElevatedResult {
                endpoints,
                cleaned_endpoints,
            })
        }
        ElevatedOperation::CleanupEndpoints { endpoints } => {
            if endpoints.len() > 64 {
                return Err("too many virtual endpoints in cleanup request".into());
            }
            validate_cleanup_set(&manager, &endpoints)?;

            let mut cleaned_endpoints = Vec::new();
            for endpoint in endpoints {
                match manager.destroy_endpoint(&endpoint) {
                    Ok(()) => cleaned_endpoints.push(endpoint),
                    Err(error) => {
                        log::warn!(
                            "virtual-port helper could not clean {} (bus {}): {}",
                            endpoint.external_path,
                            endpoint.resource_id,
                            error
                        );
                    }
                }
            }
            Ok(ElevatedResult {
                endpoints: Vec::new(),
                cleaned_endpoints,
            })
        }
    }
}

fn validate_cleanup_set(
    manager: &VirtualPortManager,
    endpoints: &[VirtualEndpoint],
) -> Result<(), String> {
    for endpoint in endpoints {
        validate_endpoint(endpoint)?;
        if !manager.is_reclaimable_owned_endpoint(endpoint) {
            return Err(format!(
                "refusing to remove unowned or active virtual endpoint {} (resource {})",
                endpoint.external_path, endpoint.resource_id
            ));
        }
    }
    Ok(())
}

fn validate_endpoint(endpoint: &VirtualEndpoint) -> Result<(), String> {
    fn parse_port(path: &str) -> Option<u32> {
        path.strip_prefix("COM")?.parse::<u32>().ok()
    }
    let bridge = parse_port(&endpoint.bridge_path)
        .ok_or_else(|| format!("invalid virtual bridge path: {}", endpoint.bridge_path))?;
    let external = parse_port(&endpoint.external_path)
        .ok_or_else(|| format!("invalid virtual external path: {}", endpoint.external_path))?;
    if bridge == external
        || !(20..200).contains(&bridge)
        || !(20..200).contains(&external)
        || (200..=255).contains(&endpoint.resource_id)
    {
        return Err("virtual endpoint is outside TauTerm allocation bounds".into());
    }
    Ok(())
}

fn validate_resource_dir(path: &Path) -> Result<PathBuf, String> {
    let requested = normalize_path(path);
    let mut allowed = Vec::new();

    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            allowed.push(normalize_path(parent));
        }
    }

    #[cfg(debug_assertions)]
    {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(root) = manifest.parent() {
            allowed.push(normalize_path(&root.join("resources").join("com0com")));
        }
    }

    let valid = allowed
        .iter()
        .any(|candidate| windows_path_eq(&requested, candidate));
    if !valid {
        return Err(format!(
            "virtual-port helper rejected untrusted resource directory: {}",
            path.display()
        ));
    }
    if !requested.join("setupc.exe").is_file() {
        return Err(format!(
            "trusted virtual-port resource directory has no setupc.exe: {}",
            requested.display()
        ));
    }
    Ok(requested)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        let value = path.to_string_lossy();
        if let Some(stripped) = value.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            path.to_path_buf()
        }
    })
}

fn windows_path_eq(left: &Path, right: &Path) -> bool {
    normalize_path(left)
        .to_string_lossy()
        .eq_ignore_ascii_case(&normalize_path(right).to_string_lossy())
}

fn create_server_pipe(name: &str) -> Result<HANDLE, String> {
    let name = wide(name);
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            std::ptr::null(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        Err(format!(
            "failed to create virtual-port helper pipe (Win32 {})",
            unsafe { GetLastError() }
        ))
    } else {
        Ok(pipe)
    }
}

fn wait_for_helper_connection(pipe: HANDLE, helper: HANDLE) -> Result<(), String> {
    let deadline = Instant::now() + HELPER_CONNECT_TIMEOUT;
    loop {
        let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
        if connected != 0 {
            break;
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_PIPE_CONNECTED {
            break;
        }
        if error != ERROR_PIPE_LISTENING {
            return Err(format!(
                "virtual-port helper pipe connect failed (Win32 {error})"
            ));
        }
        if unsafe { WaitForSingleObject(helper, 0) } == 0 {
            return Err("virtual-port helper exited before connecting".into());
        }
        if Instant::now() >= deadline {
            return Err("virtual-port helper connection timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
    if unsafe { SetNamedPipeHandleState(pipe, &mode, std::ptr::null_mut(), std::ptr::null_mut()) }
        == 0
    {
        return Err(format!(
            "failed to switch virtual-port helper pipe to blocking mode (Win32 {})",
            unsafe { GetLastError() }
        ));
    }
    Ok(())
}

fn open_client_pipe(name: &str) -> Result<HANDLE, String> {
    let name = wide(name);
    let pipe = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        Err(format!(
            "failed to open virtual-port helper pipe (Win32 {})",
            unsafe { GetLastError() }
        ))
    } else {
        Ok(pipe)
    }
}

fn launch_helper(pipe_name: &str) -> Result<HANDLE, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve TauTerm executable: {error}"))?;
    let verb = wide("runas");
    let file = wide(&executable.to_string_lossy());
    let params = wide(&format!("--tauterm-vport-helper \"{pipe_name}\""));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_CANCELLED {
            return Err("User cancelled the UAC elevation prompt".into());
        }
        return Err(format!(
            "failed to launch virtual-port helper (Win32 {error})"
        ));
    }
    if info.hProcess.is_null() {
        return Err("virtual-port helper did not return a process handle".into());
    }
    Ok(info.hProcess)
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> std::io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(std::io::Error::other("virtual-port helper frame too large"));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn read_frame(reader: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(std::io::Error::other(
            "invalid virtual-port helper frame length",
        ));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_validation_rejects_reserved_or_foreign_paths() {
        assert!(validate_endpoint(&VirtualEndpoint {
            bridge_path: "COM20".into(),
            external_path: "COM21".into(),
            resource_id: 1,
        })
        .is_ok());
        assert!(validate_endpoint(&VirtualEndpoint {
            bridge_path: "COM200".into(),
            external_path: "COM201".into(),
            resource_id: 200,
        })
        .is_err());
    }
}
