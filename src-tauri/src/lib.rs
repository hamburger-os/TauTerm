//! TauTerm - 跨平台全功能终端模拟器
//!
//! 基于 Tauri v2 构建。首发版本支持串口终端，
//! 架构设计支持未来扩展 SSH、Telnet 等连接类型。
//!
//! ## 架构
//!
//! 所有连接类型通过 `TermSession` trait 统一抽象：
//! - `SerialSession`：串口连接（首发）
//! - SSH / Telnet：未来版本实现
//!
//! 前端通过 Tauri 命令与当前会话交互，不感知具体连接类型。

mod commands;
mod serial;
mod session;
mod transfer;

use std::sync::Mutex;
use session::serial::SerialSession;

/// 全局应用状态
pub struct AppState {
    /// 当前活跃会话（首发仅串口，未来可切换）
    pub session: Mutex<SerialSession>,
}

/// TauTerm 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            session: Mutex::new(SerialSession::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_connection_types,
            commands::enumerate_endpoints,
            commands::connect_session,
            commands::disconnect_session,
            commands::write_data,
            commands::send_files_ymodem,
            commands::receive_files_ymodem,
            commands::cancel_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("启动 TauTerm 时发生错误");
}
