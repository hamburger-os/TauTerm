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
pub mod virtual_port;

#[cfg(windows)]
pub fn maybe_run_elevated_shell_helper() -> bool {
    channel::elevated_shell_channel::maybe_run_helper()
}

use kernel::config_store::ConfigStore;
use kernel::i18n_engine::I18nEngine;
use kernel::ipc_bridge::IpcBridge;
use kernel::log_engine::{LogBridge, LogConfig, LogEngine};
use kernel::plugin_host::PluginHost;
use kernel::session_store::SessionStore;
use kernel::shortcut_engine::ShortcutEngine;
use kernel::tab_host::TabHost;
use kernel::theme_engine::ThemeEngine;
use kernel::window_manager::WindowManager;
use plugins::iperf::IperfAdapter;
use plugins::local_shell::LocalShellAdapter;
use plugins::network::NetworkAdapter;
use plugins::serial::SerialAdapter;
use plugins::ssh::HostKeyVerifier;
use plugins::ssh::SshAdapter;
use plugins::telnet::TelnetAdapter;
use plugins::tftp::TftpAdapter;
use security::CredentialStore;
use std::sync::Mutex;
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
    pub serial_adapter: SerialAdapter,
    pub ssh_adapter: SshAdapter,
    pub tftp_adapter: TftpAdapter,
    pub telnet_adapter: TelnetAdapter,
    pub local_shell_adapter: LocalShellAdapter,
    pub iperf_adapter: IperfAdapter,
    pub network_adapter: NetworkAdapter,
    pub host_key_verifier: HostKeyVerifier,
    pub config_store: ConfigStore,
    pub ipc_bridge: IpcBridge,
    pub tab_host: TabHost,
    pub plugin_host: Mutex<PluginHost>,
    pub shortcut_engine: ShortcutEngine,
    pub theme_engine: ThemeEngine,
    pub i18n_engine: I18nEngine,
    pub window_manager: WindowManager,
    pub credential_store: CredentialStore,
    pub log_engine: Mutex<LogEngine>,
    pub virtual_port_manager: Mutex<Box<dyn VirtualPortBackend>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::set_logger(&LogBridge)
        .map(|()| log::set_max_level(log::LevelFilter::Info))
        .ok();

    let mut plugin_host = PluginHost::new();
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "serial".into(),
            name: "Serial".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            content_type: "terminal".into(),
            capabilities: vec![
                "connection".into(),
                "transfer".into(),
                "endpoint_discovery".into(),
            ],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 Serial 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "ssh".into(),
            name: "SSH".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            content_type: "terminal".into(),
            capabilities: vec![
                "connection".into(),
                "transfer".into(),
                "endpoint_discovery".into(),
            ],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 SSH 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "telnet".into(),
            name: "Telnet".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            content_type: "terminal".into(),
            capabilities: vec!["connection".into()],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 Telnet 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "local-shell".into(),
            name: "Local Shell".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            content_type: "terminal".into(),
            capabilities: vec!["connection".into(), "endpoint_discovery".into()],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 Local Shell 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "tftp".into(),
            name: "TFTP".into(),
            version: "1.0.0".into(),
            category: "file_transfer".into(),
            content_type: "custom".into(),
            capabilities: vec!["connection".into(), "transfer".into()],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 TFTP 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "iperf".into(),
            name: "iperf".into(),
            version: "1.0.0".into(),
            category: "network_tool".into(),
            content_type: "custom".into(),
            capabilities: vec!["connection".into()],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 iperf 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "network".into(),
            name: "Network Debug".into(),
            version: "1.0.0".into(),
            category: "network_tool".into(),
            content_type: "custom".into(),
            capabilities: vec![
                "connection".into(),
                "network_outbound".into(),
                "network_listen".into(),
            ],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 network 插件失败");
    plugin_host
        .register_plugin(kernel::plugin_host::PluginDescriptor {
            id: "trdp".into(),
            name: "TRDP".into(),
            version: "1.0.0".into(),
            category: "network_tool".into(),
            content_type: "custom".into(),
            capabilities: vec![
                "connection".into(),
                "network_outbound".into(),
                "network_listen".into(),
            ],
            state: kernel::plugin_host::PluginState::Ready,
        })
        .expect("注册 TRDP 插件失败");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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

            let log_dir = {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("logs");
                let _ = std::fs::create_dir_all(&exe_dir);
                let test_file = exe_dir.join(".write_test");
                if std::fs::write(&test_file, b"tau").is_ok() {
                    let _ = std::fs::remove_file(&test_file);
                    exe_dir
                } else {
                    let app_data = app
                        .path()
                        .app_data_dir()
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
                let _ = state
                    .config_store
                    .set("log.dir", &log_dir.to_string_lossy().to_string());
            }
            let _ = std::fs::create_dir_all(&log_dir);
            log::info!("TauTerm v{} 已启动", env!("CARGO_PKG_VERSION"));
            log::info!("日志目录: {:?}", log_dir);

            if let Some(state) = app.try_state::<AppState>() {
                state.telnet_adapter.inject_app_handle(app.handle().clone());
                if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                    #[cfg(target_os = "windows")]
                    {
                        let resource_dir = app
                            .path()
                            .resource_dir()
                            .unwrap_or_else(|_| std::path::PathBuf::from("."));
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
                        let state_dir = app
                            .path()
                            .app_data_dir()
                            .unwrap_or_else(|_| std::path::PathBuf::from("."));
                        let _ = std::fs::create_dir_all(&state_dir);
                        let service_backend = virtual_port::service_backend::ServiceBackend::new();
                        if service_backend.connect().is_ok() {
                            log::info!("虚拟串口特权服务已连接");
                            *vpm = Box::new(service_backend);
                        } else {
                            log::warn!("虚拟串口特权服务不可用，回退到直连模式（按需 UAC）");
                            *vpm = Box::new(VirtualPortManager::new(vpm_dir, state_dir));
                        }
                        let orphan_count = vpm.cleanup_orphans();
                        if orphan_count > 0 {
                            log::info!("已清理 {} 个孤儿虚拟端口对", orphan_count);
                        }
                        if !vpm.are_files_present() {
                            log::warn!("com0com 驱动文件缺失，虚拟串口功能不可用");
                        } else if vpm.detect_driver() {
                            log::info!("com0com 驱动已就绪（安装时已自动安装或先前已安装）");
                        } else {
                            log::info!("com0com 驱动文件已找到但驱动未安装 \u{2014} 首次连接时将通过 NSIS 安装或需管理员权限运行时安装");
                        }
                        let driver_installed = vpm.detect_driver();
                        let files_present = vpm.are_files_present();
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
            serial_adapter: SerialAdapter::new(),
            ssh_adapter: SshAdapter::new(),
            tftp_adapter: TftpAdapter::new(),
            telnet_adapter: TelnetAdapter::new(),
            local_shell_adapter: LocalShellAdapter::new(),
            iperf_adapter: IperfAdapter::new(),
            network_adapter: NetworkAdapter::new(),
            host_key_verifier: HostKeyVerifier::new(),
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
            #[cfg(target_os = "windows")]
            virtual_port_manager: Mutex::new(Box::new(VirtualPortManager::new(
                std::path::PathBuf::from("."),
                std::path::PathBuf::from("."),
            ))),
            #[cfg(not(target_os = "windows"))]
            virtual_port_manager: Mutex::new(Box::new(PtyBackend::new())),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_connection_types,
            commands::enumerate_endpoints,
            plugins::trdp::connect_session_trdp,
            commands::disconnect_session,
            commands::write_data,
            commands::switch_active_session,
            commands::rename_session,
            commands::reorder_tabs,
            commands::get_tabs,
            commands::open_channel,
            commands::close_channel,
            commands::connect_session_network,
            commands::list_network_peers,
            commands::close_network_peer,
            commands::network_udp_send_to,
            commands::network_udp_send,
            commands::set_network_send_target,
            plugins::trdp::trdp_command,
            plugins::trdp::trdp_open_capture,
            plugins::trdp::trdp_save_capture,
            plugins::trdp::trdp_import_xml,
            plugins::trdp::trdp_decode_dataset,
            commands::save_sessions,
            commands::load_sessions,
            commands::save_session_config,
            commands::resolve_local_shell_session_name,
            commands::delete_session_config,
            commands::file_transfer_send,
            commands::file_transfer_receive,
            commands::file_transfer_cancel,
            commands::store_credential,
            commands::get_credential,
            commands::list_credentials,
            commands::delete_credential,
            commands::credential_storage_status,
            commands::unlock_credential_vault,
            commands::lock_credential_vault,
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
            commands::install_virtual_port_driver,
            commands::check_virtual_port_driver,
            commands::cleanup_virtual_ports,
            commands::start_script_engine,
            commands::stop_script_engine,
            commands::rules_to_script,
            commands::test_match,
            commands::sftp_list_dir_cmd,
            commands::sftp_stat_cmd,
            commands::sftp_read_head_cmd,
            commands::sftp_chmod_cmd,
            commands::sftp_delete_cmd,
            commands::sftp_rename_cmd,
            commands::sftp_mkdir_cmd,
            commands::sftp_new_file_cmd,
            commands::sftp_delete_batch_cmd,
            commands::sftp_delete_recursive_cmd,
            commands::start_journald_stream,
            commands::stop_journald_stream,
            commands::journald_query_cmd,
            commands::start_journald_export,
            commands::stop_journald_export,
            commands::get_ssh_home_dir,
            commands::resize_pty,
            commands::confirm_host_key,
            commands::tftp_server_start,
            commands::tftp_server_stop,
            commands::tftp_client_get,
            commands::tftp_client_put,
            commands::tftp_update_params,
            commands::tftp_get_status,
            commands::iperf_server_start,
            commands::iperf_server_stop,
            commands::iperf_client_run,
            commands::iperf_client_stop,
            commands::iperf_update_params,
            commands::iperf_get_status,
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
                        let path = SessionStore::sessions_file_path(app_handle);
                        if let Err(e) = store.save_to_disk(&path) {
                            log::warn!("保存会话到磁盘失败: {}", e);
                        }
                    }
                    if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                        vpm.cleanup_all();
                    }
                }
            }
        });
}
