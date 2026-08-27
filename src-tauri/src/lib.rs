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
use virtual_port::socat::PtyBackend;

/// 全局应用状态
pub struct AppState {
    /// 会话存储（管理所有活跃终端会话的 I/O 生命周期）
    pub session_store: Mutex<SessionStore>,
    /// 串口协议适配器
    pub serial_adapter: SerialAdapter,
    /// SSH 协议适配器
    pub ssh_adapter: SshAdapter,
    /// TFTP 协议适配器
    pub tftp_adapter: TftpAdapter,
    /// Telnet 协议适配器
    pub telnet_adapter: TelnetAdapter,
    /// iperf 协议适配器
    pub iperf_adapter: IperfAdapter,
    /// 网络调试协议适配器（TCP/UDP 调试会话）
    pub network_adapter: NetworkAdapter,
    /// SSH 主机密钥验证器（管理待确认的 host key）
    pub host_key_verifier: HostKeyVerifier,
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
    /// 虚拟串口设备管理器（com0com 驱动 + 端口对生命周期）
    pub virtual_port_manager: Mutex<Box<dyn VirtualPortBackend>>,
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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

            // ── 窗口尺寸钳制：默认 1440×900 在小屏（如 1366×768）上会溢出屏幕，
            // 启动时按主显示器工作区钳制并居中，避免无边框窗口的控制按钮/边角跑到屏幕外。
            // 尺寸取自窗口当前值（由 tauri.conf.json 定义），避免在 Rust 侧重复硬编码。
            #[cfg(target_os = "windows")]
            let work_area: Option<(u32, u32)> = {
                use windows_sys::Win32::Foundation::RECT;
                use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
                let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                let ok = unsafe {
                    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut RECT as *mut core::ffi::c_void, 0)
                };
                if ok != 0 && rect.right > rect.left && rect.bottom > rect.top {
                    Some(((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32))
                } else {
                    None
                }
            };
            #[cfg(not(target_os = "windows"))]
            let work_area: Option<(u32, u32)> = app
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| {
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

            // ── 虚拟串口后端初始化（按平台选择实现） ──
            if let Some(state) = app.try_state::<AppState>() {
                // Telnet 回显事件回调 emit 需要 AppHandle：setup 在所有命令
                // 处理器就绪前运行，任何 connect 命令执行前必然完成注入，无竞态。
                state.telnet_adapter.inject_app_handle(app.handle().clone());

                if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                    #[cfg(target_os = "windows")]
                    {
                        let resource_dir = app.path().resource_dir()
                            .unwrap_or_else(|_| std::path::PathBuf::from("."));

                        // 检测 com0com 驱动文件路径：
                        // - 生产模式（NSIS 打包）: bundle.resources 映射 → resource_dir/
                        // - 开发模式: resource_dir = src-tauri/，com0com 文件在 ../resources/com0com/
                        let vpm_dir = if resource_dir.join("setupc.exe").exists() {
                            resource_dir
                        } else {
                            let dev_path = resource_dir.join("../resources/com0com");
                            if dev_path.join("setupc.exe").exists() {
                                log::info!(
                                    "开发模式: com0com 驱动文件位于 {:?}",
                                    dev_path.canonicalize().unwrap_or_else(|_| dev_path.clone())
                                );
                                dev_path
                            } else {
                                log::warn!("com0com 驱动文件未找到（resource_dir 和 dev_path 均无 setupc.exe）");
                                resource_dir // 回退，后续 are_files_present() 会返回 false
                            }
                        };

                        let state_dir = app.path().app_data_dir()
                            .unwrap_or_else(|_| std::path::PathBuf::from("."));
                        let _ = std::fs::create_dir_all(&state_dir);

                        // 优先使用特权服务（asInvoker 下零 UAC、崩溃无孤儿）；
                        // 服务不可用时回退到直连模式（按需 UAC）。
                        let service_backend = virtual_port::service_backend::ServiceBackend::new();
                        if service_backend.connect().is_ok() {
                            log::info!("虚拟串口特权服务已连接");
                            *vpm = Box::new(service_backend);
                        } else {
                            log::warn!("虚拟串口特权服务不可用，回退到直连模式（按需 UAC）");
                            *vpm = Box::new(VirtualPortManager::new(vpm_dir, state_dir));
                        }

                        // 清理上次异常退出可能遗留的孤儿端口对
                        let orphan_count = vpm.cleanup_orphans();
                        if orphan_count > 0 {
                            log::info!("已清理 {} 个孤儿虚拟端口对", orphan_count);
                        }

                        // 分层检测 com0com 状态：
                        if !vpm.are_files_present() {
                            log::warn!("com0com 驱动文件缺失，虚拟串口功能不可用");
                        } else if vpm.detect_driver() {
                            log::info!("com0com 驱动已就绪（安装时已自动安装或先前已安装）");
                        } else {
                            log::info!("com0com 驱动文件已找到但驱动未安装 \u{2014} 首次连接时将通过 NSIS 安装或需管理员权限运行时安装");
                        }

                        // 启动时向前端报告驱动状态
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

                        // 清理上次异常退出可能遗留的孤儿 symlink
                        let orphan_count = vpm.cleanup_orphans();
                        if orphan_count > 0 {
                            log::info!("已清理 {} 个孤儿虚拟端口对 (socat)", orphan_count);
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
                        // 其他平台：虚拟串口暂未支持
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
            // 占位 VPM — setup() 闭包中立即用平台正确的实现替换。
            // setup() 在所有命令处理器就绪前运行，不存在竞态条件。
            #[cfg(target_os = "windows")]
            virtual_port_manager: Mutex::new(Box::new(VirtualPortManager::new(std::path::PathBuf::from("."), std::path::PathBuf::from(".")))),
            #[cfg(not(target_os = "windows"))]
            virtual_port_manager: Mutex::new(Box::new(PtyBackend::new())),
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
            commands::open_channel,
            commands::close_channel,
            commands::connect_session_network,
            commands::list_network_peers,
            commands::close_network_peer,
            commands::network_udp_send_to,
            commands::network_udp_send,
            commands::set_network_send_target,
            commands::save_sessions,
            commands::load_sessions,
            commands::save_session_config,
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
                    // 1. 关闭所有活跃会话（释放串口 + 关闭桥接线程）
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
                    // 2. 清理 com0com 驱动和所有虚拟端口（会话已关闭，设备不再被占用）
                    if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                        vpm.cleanup_all();
                    }
                }
            }
        });
}
