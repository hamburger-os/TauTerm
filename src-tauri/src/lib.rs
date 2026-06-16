//! TauTerm - 跨平台全功能终端模拟器
//!
//! 基于 Tauri v2 构建。首发版本支持串口终端，
//! 架构设计支持未来扩展 SSH、Telnet 等连接类型。
//!
//! ## 架构
//!
//! `SessionManager` 管理所有活跃会话的生命周期：
//! - 每标签页独立 I/O 线程（带缓冲通道）
//! - 支持最多 10 个并发会话
//! - 会话配置持久化到 JSON
//!
//! 前端通过 Tauri 命令与 SessionManager 交互。

mod commands;
mod serial;
mod session;
mod transfer;

use std::sync::Mutex;
use tauri::Manager;
use session::manager::SessionManager;

/// 全局应用状态
pub struct AppState {
    /// 会话管理器（管理所有活跃的终端会话）
    pub manager: Mutex<SessionManager>,
}

/// TauTerm 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            manager: Mutex::new(SessionManager::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_connection_types,
            commands::enumerate_endpoints,
            commands::connect_session,
            commands::disconnect_session,
            commands::write_data,
            commands::switch_active_session,
            commands::rename_session,
            commands::reorder_tabs,
            commands::get_tabs,
            commands::save_sessions,
            commands::load_sessions,
            commands::send_files_ymodem,
            commands::receive_files_ymodem,
            commands::cancel_transfer,
        ])
        .build(tauri::generate_context!())
        .expect("启动 TauTerm 时发生错误")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let path = SessionManager::sessions_file_path(app_handle);
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if let Ok(manager) = state.manager.lock() {
                        if let Err(e) = manager.save_to_disk(&path) {
                            log::warn!("保存会话到磁盘失败: {}", e);
                        }
                    }
                }
            }
        });
}
