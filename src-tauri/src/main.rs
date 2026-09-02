//! TauTerm 应用入口点

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if tauterm_lib::maybe_run_elevated_shell_helper() {
        return;
    }
    tauterm_lib::run();
}
