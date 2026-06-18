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
use kernel::config_store::ConfigStore;
use kernel::ipc_bridge::IpcBridge;
use kernel::tab_host::TabHost;
use kernel::plugin_host::PluginHost;
use kernel::session_store::SessionStore;
use kernel::shortcut_engine::ShortcutEngine;
use kernel::theme_engine::ThemeEngine;
use kernel::i18n_engine::I18nEngine;
use kernel::window_manager::WindowManager;
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
}

/// TauTerm 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

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
            commands::send_files_ymodem,
            commands::receive_files_ymodem,
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
