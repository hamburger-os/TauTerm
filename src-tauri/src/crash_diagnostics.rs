//! Process-level crash diagnostics.
//!
//! System/Session logs can only describe failures that return through Rust control flow. Native
//! faults and abrupt process termination may bypass those log paths entirely, so TauTerm keeps a
//! small, local-only crash artifact directory. Nothing in this module uploads data.

use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_CRASH_ARTIFACTS: usize = 12;
static CRASH_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn install() {
    let directory = crash_directory();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!(
            "TauTerm: failed to create crash diagnostics directory {}: {error}",
            directory.display()
        );
    } else {
        prune_old_artifacts(&directory);
    }

    install_panic_hook();

    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
        SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
    }
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

pub const fn native_minidump_enabled() -> bool {
    cfg!(target_os = "windows")
}

fn crash_directory() -> PathBuf {
    CRASH_DIR
        .get_or_init(|| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.data_local_dir().join("TauTerm").join("crash"))
                .unwrap_or_else(|| {
                    std::env::temp_dir()
                        .join(format!("TauTerm-crash-{}", std::process::id()))
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

    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    let payload = if let Some(message) = info.payload().downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    };
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

    let _ = std::fs::write(directory.join(file_name), report);
    prune_old_artifacts(&directory);
}

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
unsafe extern "system" fn unhandled_exception_filter(
    exception_pointers: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        MiniDumpNormal, MiniDumpWriteDump, EXCEPTION_CONTINUE_SEARCH,
        MINIDUMP_EXCEPTION_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
    };

    let directory = crash_directory();
    let _ = std::fs::create_dir_all(&directory);
    let process_id = GetCurrentProcessId();
    let thread_id = GetCurrentThreadId();
    let path = directory.join(format!("TauTerm_native_p{process_id}_t{thread_id}.dmp"));

    if let Ok(file) = std::fs::File::create(path) {
        let exception = MINIDUMP_EXCEPTION_INFORMATION {
            ThreadId: thread_id,
            ExceptionPointers: exception_pointers,
            ClientPointers: 0,
        };
        let _ = MiniDumpWriteDump(
            GetCurrentProcess(),
            process_id,
            file.as_raw_handle() as _,
            MiniDumpNormal,
            &exception,
            std::ptr::null(),
            std::ptr::null(),
        );
    }

    // Keep the normal Windows/WER crash path alive after taking our bounded local dump.
    EXCEPTION_CONTINUE_SEARCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_artifacts_use_a_user_local_directory() {
        let directory = crash_directory();
        assert!(directory.ends_with(Path::new("TauTerm").join("crash"))
            || directory
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("TauTerm-crash-")));
    }
}
