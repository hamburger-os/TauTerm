//! TauTerm - 面向连接系统的本地优先工程工作台
//!
//! 基于 Tauri v2 + Rust 的插件化工程工作台运行时。
//!
//! ## 架构
//!
//! - **Plugin Host**: 插件注册与发现（`kernel/plugin_host`）
//! - **Protocol Adapter**: 协议插件通过 `ProtocolAdapter` trait 管理连接
//! - **Transport Runtime**: 协议无关的物理 I/O、DataPlane 与独占租约（`transport`）
//! - **Session Runtime**: 会话生命周期、脚本 I/O 与断开语义（`session`）
//! - **Session Store**: 活跃会话注册与持久化（`kernel/session_store`）
//! - **Transfer Manager**: 文件传输调度（`transfer/manager`）
//! - **Config Store**: 版本化非敏感配置/工程资产存储（`kernel/config_store`）
//! - **Theme Engine**: CSS 变量主题切换（`kernel/theme_engine`）
//! - **Content Renderers**: content_type 驱动的渲染器系统（前端 `renderers/`）

mod channel;
mod commands;
mod diagnostics;
mod kernel;
#[cfg(test)]
mod performance_contract;
mod plugins;
mod security;
mod session;
mod transfer;
mod transport;
pub mod virtual_port;

#[cfg(windows)]
pub fn maybe_run_elevated_shell_helper() -> bool {
    channel::elevated_shell_channel::maybe_run_helper()
}

use kernel::config_store::ConfigStore;
use kernel::log_engine::{LogBridge, LogConfig, LogEngine};
use kernel::plugin_adapter::PluginManifest;
use kernel::plugin_host::PluginHost;
use kernel::session_store::SessionStore;
use kernel::theme_engine::ThemeEngine;
use plugins::iperf::IperfAdapter;
use plugins::local_shell::LocalShellAdapter;
use plugins::modbus::ModbusAdapter;
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
    pub modbus_adapter: ModbusAdapter,
    pub host_key_verifier: HostKeyVerifier,
    pub config_store: ConfigStore,
    pub plugin_host: Mutex<PluginHost>,
    pub theme_engine: ThemeEngine,
    pub credential_store: CredentialStore,
    pub log_engine: Mutex<LogEngine>,
    pub virtual_port_manager: Mutex<Box<dyn VirtualPortBackend>>,
}

fn built_in_plugin_manifests() -> Vec<PluginManifest> {
    const MANIFESTS: [&str; 9] = [
        include_str!("../../src/plugin-manifests/serial.json"),
        include_str!("../../src/plugin-manifests/ssh.json"),
        include_str!("../../src/plugin-manifests/telnet.json"),
        include_str!("../../src/plugin-manifests/local-shell.json"),
        include_str!("../../src/plugin-manifests/tftp.json"),
        include_str!("../../src/plugin-manifests/iperf.json"),
        include_str!("../../src/plugin-manifests/network.json"),
        include_str!("../../src/plugin-manifests/trdp.json"),
        include_str!("../../src/plugin-manifests/modbus.json"),
    ];

    MANIFESTS
        .into_iter()
        .map(|raw| serde_json::from_str::<PluginManifest>(raw).expect("canonical plugin manifest"))
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::set_logger(&LogBridge)
        .map(|()| log::set_max_level(log::LevelFilter::Info))
        .ok();

    let mut plugin_host = PluginHost::new();
    for manifest in built_in_plugin_manifests() {
        let plugin_id = manifest.id.clone();
        plugin_host
            .register_plugin(manifest)
            .unwrap_or_else(|error| panic!("注册插件 {plugin_id} 失败: {error}"));
    }

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
                            let system_max_file_size = state
                                .config_store
                                .get::<u64>("logging.system_max_file_size")
                                .unwrap_or(5 * 1024 * 1024);
                            let system_max_files = state
                                .config_store
                                .get::<usize>("logging.system_max_files")
                                .unwrap_or(3);
                            let session_enabled = state
                                .config_store
                                .get::<bool>("logging.session_enabled")
                                .unwrap_or(false);
                            let session_format = state
                                .config_store
                                .get::<String>("logging.session_format")
                                .unwrap_or_else(|| "text".to_string());
                            let session_max_file_size = state
                                .config_store
                                .get::<u64>("logging.session_max_file_size")
                                .unwrap_or(10 * 1024 * 1024);
                            let session_max_files = state
                                .config_store
                                .get::<usize>("logging.session_max_files")
                                .unwrap_or(5);

                            if let Ok(mut engine) = state.log_engine.lock() {
                                engine.update_config(LogConfig {
                                    system_enabled,
                                    system_level,
                                    system_max_file_size,
                                    system_max_files,
                                    session_enabled,
                                    session_format,
                                    session_max_file_size,
                                    session_max_files,
                                    ..LogConfig::default()
                                });
                            }
                        }
                    }
                    Err(error) => {
                        log::warn!("无法确定应用配置目录: {}", error);
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
            modbus_adapter: ModbusAdapter::new(),
            host_key_verifier: HostKeyVerifier::new(),
            config_store: ConfigStore::new(),
            plugin_host: Mutex::new(plugin_host),
            theme_engine: ThemeEngine::new(),
            credential_store: CredentialStore::new(),
            log_engine: Mutex::new(LogEngine::new()),
            virtual_port_manager: Mutex::new({
                #[cfg(target_os = "windows")]
                {
                    Box::new(VirtualPortManager::new())
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Box::new(PtyBackend::new())
                }
            }),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_plugins,
            commands::enumerate_endpoints,
            commands::connect_session,
            commands::disconnect_session,
            commands::write_to_session,
            commands::resize_session,
            commands::get_session_stats,
            commands::start_script,
            commands::stop_script,
            commands::save_session_config,
            commands::load_session_configs,
            commands::delete_session_config,
            commands::rename_session_config,
            commands::set_session_config_param,
            commands::open_sub_connection,
            commands::close_sub_connection,
            commands::get_session_peers,
            commands::send_file,
            commands::receive_file,
            commands::cancel_file_transfer,
            commands::get_transfer_status,
            commands::disconnect_all_sessions,
            plugins::modbus::modbus_execute,
            plugins::modbus::modbus_start_polling,
            plugins::modbus::modbus_stop_polling,
            plugins::modbus::modbus_get_transactions,
            plugins::modbus::modbus_clear_transactions,
            plugins::modbus::modbus_start_server,
            plugins::modbus::modbus_stop_server,
            plugins::modbus::modbus_server_snapshot,
            plugins::modbus::modbus_server_write,
            plugins::modbus::modbus_server_set_fault,
            plugins::network::network_send,
            plugins::network::network_send_to,
            plugins::network::network_get_peers,
            plugins::network::network_close_peer,
            plugins::tftp::tftp_list_transfers,
            plugins::tftp::tftp_send_file,
            plugins::tftp::tftp_receive_file,
            plugins::tftp::tftp_cancel_transfer,
            plugins::iperf::iperf_start,
            plugins::iperf::iperf_stop,
            plugins::iperf::iperf_status,
            plugins::trdp::trdp_connect,
            plugins::trdp::trdp_disconnect,
            plugins::trdp::trdp_start,
            plugins::trdp::trdp_stop,
            plugins::trdp::trdp_status,
            plugins::trdp::trdp_discover,
            plugins::trdp::trdp_write_dataset,
            plugins::trdp::trdp_get_dataset,
            plugins::trdp::trdp_start_capture,
            plugins::trdp::trdp_stop_capture,
            plugins::trdp::trdp_capture_status,
            plugins::trdp::trdp_capture_export,
            plugins::trdp::trdp_capture_clear,
            plugins::ssh::sftp::sftp_list_dir,
            plugins::ssh::sftp::sftp_stat,
            plugins::ssh::sftp::sftp_mkdir,
            plugins::ssh::sftp::sftp_remove,
            plugins::ssh::sftp::sftp_rename,
            plugins::ssh::sftp::sftp_upload,
            plugins::ssh::sftp::sftp_download,
            plugins::ssh::journald::journald_query,
            plugins::ssh::journald::journald_start_stream,
            plugins::ssh::journald::journald_stop_stream,
            plugins::ssh::journald::journald_export,
            plugins::ssh::journald::journald_cancel_export,
            diagnostics::get_diagnostics,
            diagnostics::export_diagnostics,
            security::store_credential,
            security::get_credential,
            security::delete_credential,
            security::list_credentials,
            commands::get_log_config,
            commands::update_log_config,
            commands::list_log_files,
            commands::open_log_folder,
            commands::clear_logs,
            commands::export_log,
            commands::read_log_file,
            commands::get_theme,
            commands::set_theme,
            commands::get_settings,
            commands::set_setting,
            commands::get_virtual_ports,
            commands::create_virtual_port,
            commands::remove_virtual_port,
            commands::remove_all_virtual_ports,
            commands::get_update_status,
            commands::check_for_updates,
            commands::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
