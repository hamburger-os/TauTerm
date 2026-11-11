//! Windows ConPTY engine selection.
//!
//! TauTerm ships the Microsoft Windows Console ConPTY redistributable
//! (`conpty.dll` plus its `OpenConsole.exe` host; MIT, version pinned in
//! `THIRD_PARTY_LICENSES.md`) and pins it for the process instead of relying on
//! the in-box conhost engine.
//!
//! The in-box engine forwards mouse-tracking modes (`DECSET 1000/1002/1003/1006`)
//! only when the console input mode already carries
//! `ENABLE_VIRTUAL_TERMINAL_INPUT` at the moment the child writes them. VT-first
//! full-screen programs (pi and other Node/Bun/Deno/Python TUIs) write those
//! modes before they switch stdin to raw mode, so the modes were dropped and no
//! terminal could ever see the mouse reports. Native console programs (Codex,
//! crossterm) take a different path: they call `SetConsoleMode` and the engine
//! synthesizes the modes itself.
//!
//! The redistributable engine is the implementation Windows Terminal uses and
//! forwards those modes for every VT program, so TauTerm needs no launcher
//! process, no runtime whitelist and no console-mode polling: terminal fidelity
//! belongs to the terminal engine, not to a per-application workaround.
//!
//! `portable-pty` opens `conpty.dll` by base name on the first PTY creation.
//! Preloading the reviewed copy by absolute path pins the engine for the whole
//! process: Windows resolves later loads of the same module base name to the
//! image that is already loaded.

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{GetLastError, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
    };

    /// Redistributed engine version; keep in sync with the vendored directory and
    /// the `Microsoft.Windows.Console.ConPTY` package recorded in
    /// `THIRD_PARTY_LICENSES.md`.
    pub const ENGINE_VERSION: &str = "1.25.260930003";

    const ENGINE_HOST_FILE: &str = "OpenConsole.exe";
    const ENGINE_DLL_FILE: &str = "conpty.dll";

    static INSTALLED: OnceLock<Option<PathBuf>> = OnceLock::new();

    enum LoadError {
        /// The candidate directory does not contain the reviewed pair (normal for
        /// the fallback locations that were never staged).
        NotFound,
        /// The pair exists but Windows rejected the image.
        LoadFailed(u32),
    }

    fn platform_dir() -> &'static str {
        if cfg!(target_arch = "aarch64") {
            "win-arm64"
        } else if cfg!(target_arch = "x86") {
            "win-x86"
        } else {
            "win-x64"
        }
    }

    /// Search order: the bundled payload next to the executable (installed app),
    /// then the repository vendor directory (development builds), then a flat
    /// next-to-exe copy for diagnostics. The first complete pair wins; the host
    /// executable must sit beside the DLL because the DLL launches it. Keep the
    /// reviewed vendor pair ahead of the flat diagnostic copy so a stray pair in a
    /// development target directory cannot shadow the SHA-256-pinned payload.
    fn candidate_directories() -> Vec<PathBuf> {
        let mut directories = Vec::new();
        let mut exe_dir = None;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                directories.push(parent.join("conpty").join(platform_dir()));
                exe_dir = Some(parent.to_path_buf());
            }
        }
        #[cfg(debug_assertions)]
        directories.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("vendor")
                .join("conpty")
                .join(ENGINE_VERSION)
                .join(platform_dir()),
        );
        if let Some(directory) = exe_dir {
            directories.push(directory);
        }
        directories
    }

    fn load_engine(directory: &Path) -> Result<PathBuf, LoadError> {
        let dll = directory.join(ENGINE_DLL_FILE);
        let host = directory.join(ENGINE_HOST_FILE);
        if !dll.is_file() || !host.is_file() {
            return Err(LoadError::NotFound);
        }

        let wide: Vec<u16> = dll
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call and
        // `hFile` is unused by LoadLibraryExW.
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if module.is_null() {
            // SAFETY: no preconditions for GetLastError.
            return Err(LoadError::LoadFailed(unsafe { GetLastError() }));
        }

        // The loader maps one image per module base name. If a different
        // `conpty.dll` was already mapped, this load returned that image and the
        // reviewed copy is not the active engine; report it instead of pretending
        // otherwise.
        let loaded_path = module_path(module).unwrap_or_default();
        if !same_file(&loaded_path, &dll) {
            log::warn!(
                "ConPTY engine load resolved to {} instead of {}",
                loaded_path.display(),
                dll.display()
            );
        }
        Ok(loaded_path)
    }

    fn module_path(module: HMODULE) -> Option<PathBuf> {
        let mut buffer = [0u16; 1024];
        // SAFETY: buffer is a valid writable UTF-16 buffer of the given length.
        let length =
            unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return None;
        }
        let end = (length as usize).min(buffer.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
    }

    fn same_file(left: &Path, right: &Path) -> bool {
        match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }

    /// Pins the bundled ConPTY engine for this process. Idempotent: the first
    /// successful load is kept for the process lifetime (the module is never
    /// unloaded, because `portable-pty` holds function pointers into it).
    pub fn install() -> Option<&'static Path> {
        INSTALLED
            .get_or_init(|| {
                for directory in candidate_directories() {
                    match load_engine(&directory) {
                        Ok(loaded) => {
                            log::info!(
                                "ConPTY engine {ENGINE_VERSION} pinned from {}",
                                loaded.display()
                            );
                            return Some(loaded);
                        }
                        Err(LoadError::NotFound) => continue,
                        Err(LoadError::LoadFailed(code)) => log::warn!(
                            "Bundled ConPTY engine at {} failed to load (win32 error {code})",
                            directory.display()
                        ),
                    }
                }
                log::warn!(
                    "Bundled ConPTY engine {ENGINE_VERSION} not found; falling back to the in-box \
                     engine. VT-first full-screen programs may not receive mouse input."
                );
                None
            })
            .as_deref()
    }

    pub fn installed_engine_path() -> Option<&'static Path> {
        INSTALLED.get().and_then(|value| value.as_deref())
    }
}

#[cfg(windows)]
pub(crate) use platform::{install, installed_engine_path};
