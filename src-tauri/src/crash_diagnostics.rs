//! Process-level crash diagnostics.
//!
//! System/Session logs can only describe failures that return through Rust control flow. Native
//! faults and abrupt process termination may bypass those log paths entirely, so TauTerm keeps a
//! small, local-only crash artifact directory. Nothing in this module uploads data.
//!
//! On Windows the crashing GUI never calls MiniDumpWriteDump itself. A same-binary, unprivileged
//! helper is started during normal initialization and blocks on a private inherited pipe. The
//! exception filter only writes one fixed-size packet and waits briefly for that helper to exit;
//! the helper opens the crashing process and writes the minidump out-of-process.

use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_CRASH_ARTIFACTS: usize = 12;
static CRASH_DIR: OnceLock<PathBuf> = OnceLock::new();

#[cfg(target_os = "windows")]
const CRASH_HELPER_ARG: &str = "--tauterm-crash-handler";
#[cfg(target_os = "windows")]
const CRASH_PACKET_MAGIC: u32 = 0x5441_5543; // "TAUC"
#[cfg(target_os = "windows")]
const CRASH_HELPER_WAIT_MS: u32 = 5_000;
#[cfg(target_os = "windows")]
const CRASH_HELPER_READY: u8 = 0xA5;

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
static NATIVE_MINIDUMP_READY: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static CRASH_CHANNEL: OnceLock<CrashChannel> = OnceLock::new();

#[cfg(target_os = "windows")]
struct CrashChannel {
    pipe_handle: usize,
    helper_process_handle: usize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy)]
struct CrashPacket {
    magic: u32,
    process_id: u32,
    thread_id: u32,
    _reserved: u32,
    exception_pointers: usize,
}

pub fn install() {
    let directory = crash_directory();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!(
            "TauTerm: failed to create crash diagnostics directory {}: {error}",
            directory.display()
        );
    } else {
        harden_directory_permissions(&directory);
        prune_old_artifacts(&directory);
    }

    install_panic_hook();

    #[cfg(target_os = "windows")]
    match start_native_crash_helper() {
        Ok(()) => unsafe {
            use windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
            SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
            NATIVE_MINIDUMP_READY.store(true, Ordering::Release);
        },
        Err(error) => {
            eprintln!("TauTerm: native crash helper unavailable: {error}");
        }
    }
}

#[cfg(target_os = "windows")]
pub fn maybe_run_helper() -> bool {
    let mut args = std::env::args_os();
    let _program = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new(CRASH_HELPER_ARG)) {
        return false;
    }

    let expected_pid = args
        .next()
        .and_then(|value| value.to_string_lossy().parse::<u32>().ok());
    if let Some(expected_pid) = expected_pid {
        match crash_helper_parent_pid() {
            Ok(parent_pid) if parent_pid == expected_pid => {
                use std::io::Write;
                let mut stdout = std::io::stdout();
                if stdout.write_all(&[CRASH_HELPER_READY]).is_ok() && stdout.flush().is_ok() {
                    let directory = crash_directory();
                    let _ = run_native_crash_helper(expected_pid, &directory);
                }
            }
            Ok(_) | Err(_) => {
                // Hidden helper mode is intentionally fail-closed. A command-line PID alone is
                // never authority to inspect another process.
            }
        }
    }
    true
}

#[cfg(target_os = "windows")]
fn crash_helper_parent_pid() -> Result<u32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "CreateToolhelp32Snapshot failed (Win32 {})",
            unsafe { GetLastError() }
        ));
    }

    let current_pid = unsafe { GetCurrentProcessId() };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        if entry.th32ProcessID == current_pid {
            let parent_pid = entry.th32ParentProcessID;
            unsafe {
                CloseHandle(snapshot);
            }
            return Ok(parent_pid);
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    unsafe {
        CloseHandle(snapshot);
    }
    Err("crash helper process was not present in ToolHelp snapshot".to_string())
}

pub fn directory() -> PathBuf {
    crash_directory()
}

pub fn artifact_count() -> u64 {
    std::fs::read_dir(crash_directory())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| matches!(extension, "txt" | "dmp"))
        })
        .count() as u64
}

pub fn native_minidump_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        NATIVE_MINIDUMP_READY.load(Ordering::Acquire)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

fn crash_directory() -> PathBuf {
    CRASH_DIR
        .get_or_init(|| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.data_local_dir().join("TauTerm").join("crash"))
                .unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("TauTerm-crash-{}", std::process::id()))
                })
        })
        .clone()
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_panic_report(info);
        previous(info);
    }));
}

fn write_panic_report(info: &std::panic::PanicHookInfo<'_>) {
    let directory = crash_directory();
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    harden_directory_permissions(&directory);

    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    let payload = if let Some(message) = info.payload().downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    };
    let payload = crate::security::log_sanitizer::sanitize_log(&payload);
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".to_string());
    let timestamp = chrono::Utc::now();
    let file_name = format!(
        "TauTerm_{}_panic_p{}.txt",
        timestamp.format("%Y%m%d_%H%M%S_%3f"),
        std::process::id()
    );
    let report = format!(
        "TauTerm panic report\nversion={}\ntimestamp={}\npid={}\nthread={}\nlocation={}\npayload={}\n\nbacktrace:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        timestamp.to_rfc3339(),
        std::process::id(),
        thread_name,
        location,
        payload,
        Backtrace::force_capture()
    );

    let path = directory.join(file_name);
    if std::fs::write(&path, report).is_ok() {
        harden_file_permissions(&path);
    }
    prune_old_artifacts(&directory);
}

#[cfg(unix)]
fn harden_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn harden_directory_permissions(_path: &Path) {}

#[cfg(unix)]
fn harden_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn harden_file_permissions(_path: &Path) {}

fn prune_old_artifacts(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?;
            if !matches!(extension, "txt" | "dmp") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    if files.len() <= MAX_CRASH_ARTIFACTS {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove_count = files.len().saturating_sub(MAX_CRASH_ARTIFACTS);
    for (_, path) in files.into_iter().take(remove_count) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(target_os = "windows")]
fn start_native_crash_helper() -> Result<(), String> {
    if CRASH_CHANNEL.get().is_some() {
        return Ok(());
    }

    use std::io::Read;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let executable =
        std::env::current_exe().map_err(|error| format!("resolve current executable: {error}"))?;
    let mut child = Command::new(executable)
        .arg(CRASH_HELPER_ARG)
        .arg(std::process::id().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("start crash helper: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "crash helper stdin pipe was not created".to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "crash helper readiness pipe was not created".to_string())?;
    let mut ready = [0u8; 1];
    if stdout.read_exact(&mut ready).is_err() || ready[0] != CRASH_HELPER_READY {
        let _ = child.kill();
        let _ = child.wait();
        return Err("crash helper failed readiness handshake".to_string());
    }
    drop(stdout);

    let channel = CrashChannel {
        pipe_handle: stdin.as_raw_handle() as usize,
        helper_process_handle: child.as_raw_handle() as usize,
    };
    CRASH_CHANNEL
        .set(channel)
        .map_err(|_| "crash helper channel already initialized".to_string())?;

    // These handles intentionally live until process teardown. On a normal exit Windows closes the
    // pipe, the helper observes EOF and exits; on a crash the exception filter uses both handles.
    let _ = Box::leak(Box::new(stdin));
    let _ = Box::leak(Box::new(child));
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn unhandled_exception_filter(
    exception_pointers: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, GetCurrentThreadId, WaitForSingleObject,
    };

    let Some(channel) = CRASH_CHANNEL.get() else {
        return EXCEPTION_CONTINUE_SEARCH;
    };
    let packet = CrashPacket {
        magic: CRASH_PACKET_MAGIC,
        process_id: GetCurrentProcessId(),
        thread_id: GetCurrentThreadId(),
        _reserved: 0,
        exception_pointers: exception_pointers as usize,
    };
    let mut written = 0u32;
    let success = WriteFile(
        channel.pipe_handle as _,
        (&packet as *const CrashPacket).cast(),
        std::mem::size_of::<CrashPacket>() as u32,
        &mut written,
        std::ptr::null_mut(),
    );
    if success != 0 && written == std::mem::size_of::<CrashPacket>() as u32 {
        let _ = WaitForSingleObject(channel.helper_process_handle as _, CRASH_HELPER_WAIT_MS);
    }

    // Preserve the normal Windows/WER crash path after the helper captured best-effort evidence.
    EXCEPTION_CONTINUE_SEARCH
}

#[cfg(target_os = "windows")]
fn run_native_crash_helper(expected_pid: u32, directory: &Path) -> Result<(), String> {
    use std::io::Read;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        MiniDumpNormal, MiniDumpWriteDump, EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let mut packet = CrashPacket {
        magic: 0,
        process_id: 0,
        thread_id: 0,
        _reserved: 0,
        exception_pointers: 0,
    };
    let packet_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut packet as *mut CrashPacket).cast::<u8>(),
            std::mem::size_of::<CrashPacket>(),
        )
    };
    if std::io::stdin().read_exact(packet_bytes).is_err() {
        // Normal parent exit closes the pipe without a packet.
        return Ok(());
    }
    if packet.magic != CRASH_PACKET_MAGIC || packet.process_id != expected_pid {
        return Err("invalid crash helper packet".to_string());
    }

    std::fs::create_dir_all(directory)
        .map_err(|error| format!("create crash directory: {error}"))?;
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            0,
            packet.process_id,
        )
    };
    if process.is_null() {
        return Err("open crashing process failed".to_string());
    }

    let timestamp = chrono::Utc::now();
    let path = directory.join(format!(
        "TauTerm_{}_native_p{}.dmp",
        timestamp.format("%Y%m%d_%H%M%S_%3f"),
        packet.process_id
    ));
    let result = (|| {
        let file =
            std::fs::File::create(&path).map_err(|error| format!("create minidump: {error}"))?;
        let exception = MINIDUMP_EXCEPTION_INFORMATION {
            ThreadId: packet.thread_id,
            ExceptionPointers: packet.exception_pointers as *mut EXCEPTION_POINTERS,
            ClientPointers: 1,
        };
        let ok = unsafe {
            MiniDumpWriteDump(
                process,
                packet.process_id,
                file.as_raw_handle() as _,
                MiniDumpNormal,
                &exception,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let _ = std::fs::remove_file(&path);
            return Err("MiniDumpWriteDump failed".to_string());
        }
        Ok(())
    })();

    unsafe {
        CloseHandle(process);
    }
    if result.is_ok() {
        prune_old_artifacts(directory);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_artifacts_use_a_user_local_directory() {
        let directory = crash_directory();
        assert!(
            directory.ends_with(Path::new("TauTerm").join("crash"))
                || directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("TauTerm-crash-"))
        );
    }

    #[test]
    fn crash_artifact_retention_is_bounded() {
        let configured = MAX_CRASH_ARTIFACTS;
        assert!((1..=32).contains(&configured));
    }
}
