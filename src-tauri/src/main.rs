//! TauTerm 应用入口点

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauterm_lib::run();
}
