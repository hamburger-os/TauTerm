//! Machine-level Windows ownership state for virtual serial endpoints.
//!
//! Both TauTermService and the direct-UAC helper use the same protected store. The GUI may read
//! ownership records to hide internal bridge ports and report orphan counts, but it never writes
//! this directory. This keeps persisted ownership usable as privileged deletion evidence without
//! trusting user-writable AppData.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{
    SetFileSecurityW, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

fn program_data_dir() -> PathBuf {
    unsafe {
        let mut raw: PWSTR = std::ptr::null_mut();
        let result = SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut raw);
        if result == 0 && !raw.is_null() {
            let mut len = 0usize;
            while *raw.add(len) != 0 {
                len += 1;
            }
            let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(raw, len)));
            CoTaskMemFree(raw.cast());
            return path;
        }
        if !raw.is_null() {
            CoTaskMemFree(raw.cast());
        }
    }

    // Fail to a fixed machine path rather than a caller-controlled environment variable.
    PathBuf::from(r"C:\ProgramData")
}

fn machine_state_root() -> PathBuf {
    program_data_dir().join("TauTerm")
}

pub fn ownership_state_dir() -> PathBuf {
    machine_state_root().join("virtual-port")
}

/// Create/repair the ownership directory from a privileged process.
///
/// The machine-state root and ownership child are assigned to the built-in Administrators group
/// and receive a protected DACL: Authenticated Users get read access only, while SYSTEM and
/// Administrators retain full control. Files created inside inherit the same policy, so an
/// unprivileged process cannot forge endpoint ownership records or retain control by pre-creating
/// the parent directory.
pub fn ensure_ownership_state_dir() -> Result<PathBuf, String> {
    let root = machine_state_root();
    std::fs::create_dir_all(&root).map_err(|error| {
        format!(
            "failed to create TauTerm machine-state directory {}: {error}",
            root.display()
        )
    })?;
    validate_machine_path(&root)?;
    apply_protected_machine_acl(&root)?;

    let directory = ownership_state_dir();
    std::fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "failed to create virtual-port ownership directory {}: {error}",
            directory.display()
        )
    })?;
    validate_machine_path(&directory)?;
    apply_protected_machine_acl(&directory)?;
    Ok(directory)
}

fn validate_machine_path(path: &Path) -> Result<(), String> {
    let program_data = program_data_dir();
    let canonical_root = program_data
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize ProgramData: {error}"))?;
    let canonical_path = path.canonicalize().map_err(|error| {
        format!(
            "failed to canonicalize virtual-port ownership directory {}: {error}",
            path.display()
        )
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(format!(
            "virtual-port ownership directory escaped ProgramData: {}",
            canonical_path.display()
        ));
    }
    Ok(())
}

fn apply_protected_machine_acl(path: &Path) -> Result<(), String> {
    // O:BA prevents a pre-created user-owned directory from retaining implicit owner control.
    // D:P protects the DACL from parent inheritance. AU gets read-only access; SYSTEM and the
    // built-in Administrators group retain full control. OI/CI propagate to state files.
    let sddl = wide("O:BAD:P(A;OICI;GR;;;AU)(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)");
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
            "failed to build virtual-port ownership DACL (Win32 {})",
            unsafe { GetLastError() }
        ));
    }

    let path = wide(&path.to_string_lossy());
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    let apply_error = (applied == 0).then(|| unsafe { GetLastError() });
    unsafe {
        let _ = LocalFree(descriptor);
    }
    if let Some(error) = apply_error {
        return Err(format!(
            "failed to protect virtual-port ownership directory (Win32 {error})"
        ));
    }
    Ok(())
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
    fn ownership_path_is_machine_level() {
        let path = ownership_state_dir();
        assert!(path.ends_with(Path::new("TauTerm").join("virtual-port")));
        assert!(!path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("appdata"));
    }
}
