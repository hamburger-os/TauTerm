//! TauTerm - 跨平台全功能终端模拟器
//!
//! 基于 Tauri v2 的微内核插件架构终端模拟器。
//!
//! ## 架构
//!
//! - **Plugin Host**: 插件注册与发现（`kernel/plugin_host`）
//! - **Protocol Adapter**: 协议插件通过 `ProtocolAdapter` trait 管理连接
//! - **Channel**: 统一 I/O 抽象，`SerialChannel` 包装串口端口
//! - **Session Store**: 管理活跃会话的 I/O 线程生命周期（`kernel/session_store`）
//! - **Transfer Manager**: 三策略传输路由（`transfer/manager`）
//! - **Config Store**: 类型安全配置存储（`kernel/config_store`）
//! - **Theme Engine**: CSS 变量主题切换（`kernel/theme_engine`）
//! - **Tab Host**: 标签页 CRUD（`kernel/tab_host`）
//! - **Content Renderers**: content_type 驱动的渲染器系统（前端 `renderers/`）

mod channel;
mod commands;
mod kernel;
mod plugins;
mod security;
mod transfer;

use std::sync::Mutex;
use tauri::Manager;
use tauri::image::Image;
use kernel::config_store::ConfigStore;
use kernel::ipc_bridge::IpcBridge;
use kernel::tab_host::TabHost;
use kernel::plugin_host::PluginHost;
use kernel::session_store::SessionStore;
use kernel::shortcut_engine::ShortcutEngine;
use kernel::theme_engine::ThemeEngine;
use kernel::i18n_engine::I18nEngine;
use kernel::window_manager::WindowManager;
use kernel::log_engine::{LogEngine, LogConfig, LogBridge};
use security::CredentialStore;
use plugins::serial::SerialAdapter;

/// 全局应用状态
pub struct AppState {
    /// 会话存储（管理所有活跃终端会话的 I/O 生命周期）
    pub session_store: Mutex<SessionStore>,
    /// 串口协议适配器
    pub serial_adapter: SerialAdapter,
    /// 类型安全配置存储
    pub config_store: ConfigStore,
    /// IPC 桥接器
    pub ipc_bridge: IpcBridge,
    /// 标签页宿主
    pub tab_host: TabHost,
    /// 插件宿主
    pub plugin_host: Mutex<PluginHost>,
    /// 快捷键引擎
    pub shortcut_engine: ShortcutEngine,
    /// 主题引擎
    pub theme_engine: ThemeEngine,
    /// 国际化引擎
    pub i18n_engine: I18nEngine,
    /// 窗口管理器
    pub window_manager: WindowManager,
    /// 凭据存储
    pub credential_store: CredentialStore,
    /// 日志引擎（生产者-消费者异步日志系统）
    pub log_engine: Mutex<LogEngine>,
}

/// TauTerm 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 用 LogBridge 替代 env_logger：所有 log::info!/warn!/error!
    // 自动转发到 LogEngine，写入 TauTerm_{date}.log
    log::set_logger(&LogBridge)
        .map(|()| log::set_max_level(log::LevelFilter::Info))
        .ok();

    // 初始化 Plugin Host 并注册内建插件
    let mut plugin_host = PluginHost::new();
    plugin_host.register_plugin(kernel::plugin_host::PluginDescriptor {
        id: "serial".into(),
        name: "Serial Port".into(),
        version: "1.0.0".into(),
        category: "terminal".into(),
        content_type: "terminal".into(),
        capabilities: vec!["connection".into(), "transfer".into(), "endpoint_discovery".into()],
        state: kernel::plugin_host::PluginState::Ready,
    }).expect("注册 Serial 插件失败");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let window = app.get_webview_window("main")
                .expect("main window not found");
            // 设置窗口图标（任务栏 + 标题栏）
            if let Ok(icon) = Image::from_path("icons/icon.png") {
              let _ = window.set_icon(icon);
            }
            // Windows 平台无边框窗口丢失原生阴影，手动开启
            #[cfg(target_os = "windows")]
            let _ = window.set_shadow(true);

            // 初始化日志目录
            // 优先使用 exe 同级目录；不可写时回退到应用数据目录
            let log_dir = {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("logs");
                let _ = std::fs::create_dir_all(&exe_dir);
                // 通过写入测试文件验证可写性
                let test_file = exe_dir.join(".write_test");
                if std::fs::write(&test_file, b"tau").is_ok() {
                    let _ = std::fs::remove_file(&test_file);
                    exe_dir
                } else {
                    let app_data = app.path().app_data_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let fallback = app_data.join("logs");
                    let _ = std::fs::create_dir_all(&fallback);
                    log::warn!("exe 同级日志目录不可写，回退到: {:?}", fallback);
                    fallback
                }
            };
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(log_engine) = state.log_engine.lock() {
                    log_engine.set_log_dir(log_dir.clone());
                }
                // 同步到 ConfigStore 供前端查询
                let _ = state.config_store.set(
                    "log.dir",
                    &log_dir.to_string_lossy().to_string(),
                );
            }
            let _ = std::fs::create_dir_all(&log_dir);
            log::info!("TauTerm v{} 已启动", env!("CARGO_PKG_VERSION"));
            log::info!("日志目录: {:?}", log_dir);

            Ok(())
        })
        .manage(AppState {
            session_store: Mutex::new(SessionStore::new()),
            serial_adapter: SerialAdapter::new(),
            config_store: ConfigStore::new(),
            ipc_bridge: IpcBridge::new(),
            tab_host: TabHost::new(10),
            plugin_host: Mutex::new(plugin_host),
            shortcut_engine: ShortcutEngine::new(),
            theme_engine: ThemeEngine::new(),
            i18n_engine: I18nEngine::new(),
            window_manager: WindowManager::new(),
            credential_store: CredentialStore::new(),
            log_engine: Mutex::new(LogEngine::new(LogConfig::default())),
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
            commands::save_session_config,
            commands::delete_session_config,
            commands::send_files,
            commands::receive_files,
            commands::cancel_transfer,
            commands::store_credential,
            commands::get_credential,
            commands::list_credentials,
            commands::delete_credential,
            commands::get_config,
            commands::set_config,
            commands::delete_config,
            commands::get_theme_list,
            commands::get_active_theme,
            commands::set_theme,
            commands::start_session_log,
            commands::stop_session_log,
            commands::log_event,
            commands::get_log_status,
            commands::set_system_log_config,
            commands::get_log_dir,
            commands::get_log_config,
            commands::open_log_dir,
            commands::update_log_config,
            commands::clear_all_logs,
        ])
        .build(tauri::generate_context!())
        .expect("启动 TauTerm 时发生错误")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let path = SessionStore::sessions_file_path(app_handle);
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if let Ok(store) = state.session_store.lock() {
                        if let Err(e) = store.save_to_disk(&path) {
                            log::warn!("保存会话到磁盘失败: {}", e);
                        }
                    }
                }
            }
        });
}
