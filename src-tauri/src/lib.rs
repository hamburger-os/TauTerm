//! TauTerm - 面向连接系统的本地优先工程工作台
//!
//! 基于 Tauri v2 + Rust 的插件化工程工作台运行时。
//!
//! ## 架构
//!
//! - **Plugin Runtime**: canonical manifest、Adapter 与类型化 contribution 的唯一注册目录（`kernel/plugin_runtime`）
//! - **Plugin Catalog**: 内建插件 composition、宿主生命周期注入与专属 IPC 的唯一目录（`plugins/catalog`）
//! - **Protocol Adapter**: 协议插件通过 `ProtocolAdapter` trait 管理连接
//! - **Transport Runtime**: 协议无关的物理 I/O、DataPlane 与独占租约（`transport`）
//! - **Session Runtime**: 会话生命周期、脚本 I/O 与断开语义（`session`）
//! - **Session Store**: 活跃会话注册与持久化（`kernel/session_store`）
//! - **Transfer Manager**: 文件传输调度（`transfer/manager`）
//! - **Config Store**: 版本化非敏感配置/工程资产存储（`kernel/config_store`）
//! - **Theme Engine**: CSS 变量主题切换（`kernel/theme_engine`）
//! - **Content Renderers**: content_type 驱动的渲染器系统（前端 `renderers/`）

#[cfg(test)]
mod architecture_contract;
mod commands;
mod diagnostics;
mod embedded_debug;
mod ipc_transport;
mod kernel;
#[cfg(test)]
mod performance_contract;
mod plugin_application;
mod plugins;
mod security;
mod session;
mod transfer;
mod transport;
pub mod virtual_port;

#[cfg(windows)]
pub fn maybe_run_elevated_shell_helper() -> bool {
    plugins::catalog::maybe_run_elevated_shell_helper()
}

use kernel::config_store::ConfigStore;
use kernel::log_engine::{LogBridge, LogConfig, LogEngine};
use kernel::plugin_runtime::PluginRuntime;
use kernel::session_store::SessionStore;
use kernel::theme_engine::ThemeEngine;
use security::CredentialStore;
use std::any::Any;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::{Emitter, Manager};
use virtual_port::backend::VirtualPortBackend;
#[cfg(target_os = "windows")]
use virtual_port::manager::VirtualPortManager;

#[cfg(not(target_os = "windows"))]
use virtual_port::pty::PtyBackend;

/// 全局应用状态
pub struct AppState {
    pub session_store: Mutex<SessionStore>,
    pub plugins: PluginRuntime,
    pub config_store: ConfigStore,
    pub theme_engine: ThemeEngine,
    pub credential_store: CredentialStore,
    pub log_engine: Mutex<LogEngine>,
    pub virtual_port_manager: Mutex<Box<dyn VirtualPortBackend>>,
}

impl AppState {
    /// 获取 bootstrap 已注册的类型化插件 contribution。
    ///
    /// 这是应用 composition 层的编程不变量：缺失表示内建插件注册与调用点不一致，
    /// 不属于可由用户输入恢复的运行时错误。
    pub fn plugin<T>(&self, plugin_id: &str) -> Arc<T>
    where
        T: Any + Send + Sync + 'static,
    {
        self.plugins
            .contribution_by_str::<T>(plugin_id)
            .unwrap_or_else(|| {
                panic!("built-in plugin contribution '{plugin_id}' is not registered")
            })
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::set_logger(&LogBridge)
        .map(|()| log::set_max_level(log::LevelFilter::Info))
        .ok();

    let plugin_runtime = plugins::catalog::build_runtime();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window not found");
            if let Ok(icon) = Image::from_path("icons/icon.png") {
                let _ = window.set_icon(icon);
            }
            #[cfg(target_os = "windows")]
            let _ = window.set_shadow(true);

            #[cfg(target_os = "windows")]
            let work_area: Option<(u32, u32)> = {
                use windows_sys::Win32::Foundation::RECT;
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    SystemParametersInfoW, SPI_GETWORKAREA,
                };
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let ok = unsafe {
                    SystemParametersInfoW(
                        SPI_GETWORKAREA,
                        0,
                        &mut rect as *mut RECT as *mut core::ffi::c_void,
                        0,
                    )
                };
                if ok != 0 && rect.right > rect.left && rect.bottom > rect.top {
                    Some(((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32))
                } else {
                    None
                }
            };
            #[cfg(not(target_os = "windows"))]
            let work_area: Option<(u32, u32)> = app.primary_monitor().ok().flatten().map(|m| {
                let s = m.size();
                (s.width, s.height)
            });
            if let (Some((work_w, work_h)), Ok(current)) = (work_area, window.outer_size()) {
                let w = current.width.min(work_w);
                let h = current.height.min(work_h);
                if w != current.width || h != current.height {
                    let _ = window.set_size(tauri::PhysicalSize::new(w, h));
                }
            }
            let _ = window.center();

            if let Some(state) = app.try_state::<AppState>() {
                match app.path().app_config_dir() {
                    Ok(config_dir) => {
                        let settings_path = config_dir.join("settings.json");
                        if let Err(error) = state.config_store.configure_persistence(settings_path) {
                            log::warn!("配置存储初始化失败: {}", error);
                        } else {
                            let system_enabled = state
                                .config_store
                                .get::<bool>("logging.system_enabled")
                                .unwrap_or(true);
                            let system_level = state
                                .config_store
                                .get::<String>("logging.system_level")
                                .unwrap_or_else(|| "info".to_string());
                            if let Err(error) =
                                kernel::log_engine::set_system_log_config(system_enabled, &system_level)
                            {
                                log::warn!("系统日志运行态配置初始化失败: {}", error);
                            }

                            if let Ok(log_engine) = state.log_engine.lock() {
                                if let Err(error) =
                                    log_engine.update_config(kernel::log_engine::LogConfigUpdate {
                                        session_enabled: state
                                            .config_store
                                            .get::<bool>("logging.session_enabled"),
                                        file_max_size: state
                                            .config_store
                                            .get::<u64>("logging.file_max_size"),
                                        buffer_size: state
                                            .config_store
                                            .get::<usize>("logging.buffer_size"),
                                        flush_interval_ms: state
                                            .config_store
                                            .get::<u64>("logging.flush_interval_ms"),
                                        retention_days: state
                                            .config_store
                                            .get::<u64>("logging.retention_days"),
                                    })
                                {
                                    log::warn!("Session 日志运行态配置初始化失败: {}", error);
                                }
                            }
                        }

                        if let Err(error) =
                            plugins::catalog::configure_persistence(&state.plugins, &config_dir)
                        {
                            log::warn!("插件持久化资源初始化失败: {}", error);
                        }
                    }
                    Err(error) => {
                        log::warn!(
                            "应用配置目录不可用；ConfigStore 与插件持久化资源保持 fail-closed: {}",
                            error
                        );
                    }
                }
            }

            let log_dir = {
                let writable = |dir: std::path::PathBuf| -> Option<std::path::PathBuf> {
                    std::fs::create_dir_all(&dir).ok()?;
                    let test_file = dir.join(".write_test");
                    std::fs::write(&test_file, b"tau").ok()?;
                    let _ = std::fs::remove_file(&test_file);
                    Some(dir)
                };

                let exe_candidate = std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|dir| dir.join("logs")));
                if let Some(dir) = exe_candidate.and_then(&writable) {
                    dir
                } else {
                    let app_candidate = app.path().app_data_dir().ok().map(|dir| dir.join("logs"));
                    if let Some(dir) = app_candidate.and_then(&writable) {
                        log::warn!("exe 同级日志目录不可写，回退到应用数据目录: {:?}", dir);
                        dir
                    } else {
                        let temp = std::env::temp_dir().join("TauTerm").join("logs");
                        match writable(temp.clone()) {
                            Some(dir) => {
                                eprintln!(
                                    "TauTerm: durable log directories unavailable; using temporary directory {:?}",
                                    dir
                                );
                                dir
                            }
                            None => {
                                eprintln!(
                                    "TauTerm: no writable log directory is available; using unresolved temporary path {:?}",
                                    temp
                                );
                                temp
                            }
                        }
                    }
                }
            };
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(log_engine) = state.log_engine.lock() {
                    if let Err(error) = log_engine.set_log_dir(log_dir.clone()) {
                        log::warn!("日志目录运行态配置失败: {}", error);
                    }
                }
            }
            if let Err(error) = std::fs::create_dir_all(&log_dir) {
                eprintln!(
                    "TauTerm: failed to create resolved log directory {:?}: {}",
                    log_dir, error
                );
            }
            log::info!("TauTerm v{} 已启动", env!("CARGO_PKG_VERSION"));
            log::info!("日志目录: {:?}", log_dir);

            if let Some(state) = app.try_state::<AppState>() {
                plugins::catalog::attach_app_handle(&state.plugins, app.handle().clone());
                if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                    #[cfg(target_os = "windows")]
                    {
                        let resource_dir = match app.path().resource_dir() {
                            Ok(path) => path,
                            Err(error) => {
                                let fallback = std::env::temp_dir()
                                    .join("TauTerm")
                                    .join("missing-resources");
                                log::warn!(
                                    "应用资源目录不可用，虚拟串口驱动资源保持不可用状态: {} ({:?})",
                                    error,
                                    fallback
                                );
                                fallback
                            }
                        };
                        let vpm_dir = if resource_dir.join("setupc.exe").exists() {
                            resource_dir
                        } else {
                            let dev_path = resource_dir.join("../resources/com0com");
                            if dev_path.join("setupc.exe").exists() {
                                log::info!(
                                    "开发模式: com0com 驱动文件位于 {:?}",
                                    dev_path
                                        .canonicalize()
                                        .unwrap_or_else(|_| dev_path.clone())
                                );
                                dev_path
                            } else {
                                log::warn!("com0com 驱动文件未找到（resource_dir 和 dev_path 均无 setupc.exe）");
                                resource_dir
                            }
                        };
                        let state_dir = match app.path().app_data_dir() {
                            Ok(path) => path,
                            Err(error) => {
                                let fallback = std::env::temp_dir()
                                    .join("TauTerm")
                                    .join("virtual-port-state");
                                log::warn!(
                                    "应用数据目录不可用，虚拟串口状态使用临时隔离目录: {} ({:?})",
                                    error,
                                    fallback
                                );
                                fallback
                            }
                        };
                        if let Err(error) = std::fs::create_dir_all(&state_dir) {
                            log::warn!(
                                "无法创建虚拟串口状态目录 {:?}: {}",
                                state_dir,
                                error
                            );
                        }

                        #[cfg(debug_assertions)]
                        {
                            // Development binaries are not accepted by the installed privileged
                            // service identity boundary. Select the documented direct-UAC backend
                            // immediately instead of generating an expected pipe-handshake warning.
                            log::info!(
                                "虚拟串口管理后端: development-direct-uac（debug build）"
                            );
                            *vpm = Box::new(VirtualPortManager::new(vpm_dir, state_dir));
                        }

                        #[cfg(not(debug_assertions))]
                        {
                            let service_backend =
                                virtual_port::service_backend::ServiceBackend::new();
                            match service_backend.connect() {
                                Ok(()) => {
                                    log::info!("虚拟串口管理后端: privileged-service");
                                    *vpm = Box::new(service_backend);
                                    let orphan_count = vpm.cleanup_orphans();
                                    if orphan_count > 0 {
                                        log::info!("已清理 {} 个孤儿虚拟端口对", orphan_count);
                                    }
                                }
                                Err(error) => {
                                    log::warn!(
                                        "虚拟串口管理后端: direct-uac-on-demand（特权服务不可用: {}）",
                                        error
                                    );
                                    // Release/portable fallback remains explicit-action UAC only:
                                    // ordinary startup never enumerates or cleans setupc resources.
                                    *vpm = Box::new(VirtualPortManager::new(vpm_dir, state_dir));
                                }
                            }
                        }

                        let files_present = vpm.are_files_present();
                        let driver_installed = files_present
                            && virtual_port::windows_driver::is_com0com_driver_installed();
                        if !files_present {
                            log::warn!("com0com 驱动文件状态: missing；虚拟串口功能不可用");
                        } else if driver_installed {
                            log::info!("com0com 驱动状态: installed");
                        } else {
                            log::info!(
                                "com0com 驱动状态: not-installed；首次使用时需要安装或管理员权限"
                            );
                        }

                        drop(vpm);
                        if files_present && !driver_installed {
                            let _ = app.handle().emit("com0com-driver-missing", serde_json::json!({
                                "reason": "com0com driver not installed. Run TauTerm as administrator once to install the driver.",
                                "can_install": true,
                            }));
                        } else if !files_present {
                            let _ = app.handle().emit("com0com-driver-missing", serde_json::json!({
                                "reason": "com0com driver files missing. Virtual serial port feature unavailable.",
                                "can_install": false,
                            }));
                        }
                    }
                    #[cfg(any(target_os = "linux", target_os = "macos"))]
                    {
                        *vpm = Box::new(PtyBackend::new());
                        let orphan_count = vpm.cleanup_orphans();
                        if orphan_count > 0 {
                            log::info!("已清理 {} 个遗留虚拟端点资源", orphan_count);
                        }
                        if vpm.are_files_present() {
                            log::info!("原生 PTY 后端已就绪，虚拟串口功能可用");
                        } else {
                            log::warn!("原生 PTY 后端不可用");
                            let _ = app.handle().emit("com0com-driver-missing", serde_json::json!({
                                "reason": "Native PTY backend unavailable",
                                "can_install": false,
                            }));
                        }
                        drop(vpm);
                    }
                    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
                    {
                        log::warn!("当前平台不支持虚拟串口功能");
                        let _ = app.handle().emit("com0com-driver-missing", serde_json::json!({
                            "reason": "Virtual serial port feature not yet supported on this platform",
                            "can_install": false,
                        }));
                        drop(vpm);
                    }
                }
            }
            Ok(())
        })
        .manage(AppState {
            session_store: Mutex::new(SessionStore::new()),
            plugins: plugin_runtime,
            config_store: ConfigStore::new(),
            theme_engine: ThemeEngine::new(),
            credential_store: CredentialStore::new(),
            log_engine: Mutex::new(LogEngine::new(LogConfig::default())),
            #[cfg(target_os = "windows")]
            virtual_port_manager: Mutex::new(Box::new(VirtualPortManager::new(
                std::env::temp_dir().join("TauTerm").join("missing-resources"),
                std::env::temp_dir().join("TauTerm").join("virtual-port-state"),
            ))),
            #[cfg(not(target_os = "windows"))]
            virtual_port_manager: Mutex::new(Box::new(PtyBackend::new())),
        })
        .invoke_handler(crate::tauterm_invoke_handler![
            commands::get_connection_types,
            commands::enumerate_endpoints,
            commands::connect_session,
            commands::disconnect_session,
            ipc_transport::write_data,
            commands::switch_active_session,
            commands::rename_session,
            commands::reorder_tabs,
            commands::get_tabs,
            commands::open_channel,
            ipc_transport::close_channel,
            commands::load_sessions,
            commands::save_session_config,
            commands::delete_session_config,
            commands::file_transfer_send,
            commands::file_transfer_receive,
            commands::file_transfer_cancel,
            commands::files::import_command_set_file,
            commands::files::export_command_set_file,
            commands::credential_storage_status,
            commands::unlock_credential_vault,
            commands::lock_credential_vault,
            commands::config::get_config,
            commands::config::set_config,
            commands::config::delete_config,
            commands::config::get_theme_list,
            commands::config::get_active_theme,
            commands::config::set_theme,
            commands::start_session_log,
            commands::stop_session_log,
            commands::log_event,
            commands::get_log_status,
            commands::get_log_health,
            commands::set_system_log_config,
            commands::get_log_dir,
            commands::get_log_config,
            commands::open_log_dir,
            commands::update_log_config,
            commands::clear_all_logs,
            commands::platform::install_virtual_port_driver,
            commands::platform::check_virtual_port_driver,
            commands::platform::cleanup_virtual_ports,
            commands::start_script_engine,
            commands::stop_script_engine,
            commands::rules_to_script,
            commands::test_match,
            ipc_transport::resize_pty,
            diagnostics::export_diagnostics,
        ])
        .build(tauri::generate_context!())
        .expect("启动 TauTerm 时发生错误")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if let Ok(mut store) = state.session_store.lock() {
                        let ids: Vec<String> = store.tab_ids().to_vec();
                        for id in &ids {
                            if let Err(e) = store.close_session(id) {
                                log::warn!("退出时关闭会话 {} 失败: {}", id, e);
                            }
                        }
                        // Saved Session Library is configuration state, not an exit snapshot.
                        // Closing runtime resources must never overwrite it.
                    }
                    if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                        vpm.cleanup_all();
                    }
                }
            }
        });
}
