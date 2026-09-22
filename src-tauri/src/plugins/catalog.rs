//! 内建后端插件目录。
//!
//! TauTerm 采用编译期内建插件模型：具体插件的 manifest、Adapter、application contribution、
//! 专属 IPC 与进程生命周期装配只在本模块出现。`lib.rs`/Kernel 只依赖 `PluginRuntime` 与本
//! catalog；新增使用既有扩展点的插件无需继续修改应用 bootstrap。

use super::{iperf, local_shell, modbus, network, rtt, serial, ssh, telnet, tftp, trdp};
use crate::kernel::plugin_adapter::{PluginId, PluginManifest, ProtocolAdapter};
use crate::kernel::plugin_runtime::PluginRuntime;
use crate::plugin_application::{SessionConnectHandler, SessionDisconnectedHook};
use std::{any::Any, path::Path, sync::Arc};
use tauri::AppHandle;

/// 合并公共应用命令与内建插件专属命令。
///
/// Tauri 的 root invoke handler 是单一编译期表；把插件命令清单放在 catalog 而不是
/// `lib.rs`，可以保持 composition root 协议无关，同时继续使用稳定的现有 IPC 名称。
#[macro_export]
macro_rules! tauterm_invoke_handler {
    ($($common:path),* $(,)?) => {
        tauri::generate_handler![
            $($common,)*
            $crate::plugins::network::commands::list_network_peers,
            $crate::plugins::network::commands::close_network_peer,
            $crate::plugins::network::commands::network_udp_send_to,
            $crate::plugins::network::commands::network_udp_send,
            $crate::plugins::network::commands::set_network_send_target,
            $crate::plugins::modbus::modbus_execute,
            $crate::plugins::modbus::modbus_status,
            $crate::plugins::modbus::modbus_watch_set,
            $crate::plugins::modbus::modbus_watch_start,
            $crate::plugins::modbus::modbus_watch_stop,
            $crate::plugins::modbus::modbus_watch_values,
            $crate::plugins::modbus::modbus_server_set_value,
            $crate::plugins::modbus::modbus_server_snapshot,
            $crate::plugins::trdp::trdp_command,
            $crate::plugins::trdp::trdp_capture_interfaces,
            $crate::plugins::trdp::trdp_open_capture,
            $crate::plugins::trdp::trdp_capture_packets,
            $crate::plugins::trdp::trdp_capture_summary,
            $crate::plugins::trdp::trdp_save_capture,
            $crate::plugins::trdp::trdp_release_capture,
            $crate::plugins::trdp::trdp_import_xml,
            $crate::plugins::trdp::trdp_decode_dataset,
            $crate::plugins::rtt::commands::rtt_discover_probes,
            $crate::plugins::rtt::commands::rtt_snapshot,
            $crate::plugins::rtt::commands::rtt_history,
            $crate::plugins::rtt::commands::rtt_write,
            $crate::plugins::rtt::commands::rtt_refresh_channels,
            $crate::plugins::rtt::commands::rtt_set_automation_source_channel,
            $crate::plugins::rtt::commands::rtt_set_send_channel,
            $crate::plugins::rtt::commands::rtt_systemview_attach,
            $crate::plugins::rtt::commands::rtt_systemview_detach,
            $crate::plugins::rtt::commands::rtt_systemview_snapshot,
            $crate::plugins::rtt::commands::rtt_systemview_history,
            $crate::plugins::rtt::commands::rtt_systemview_control,
            $crate::plugins::rtt::commands::rtt_systemview_clear,
            $crate::plugins::local_shell::resolve_local_shell_session_name,
            $crate::plugins::ssh::commands::sftp_list_dir_cmd,
            $crate::plugins::ssh::commands::sftp_stat_cmd,
            $crate::transfer::sftp_document::sftp_read_document_cmd,
            $crate::transfer::sftp_document::sftp_save_document_cmd,
            $crate::transfer::sftp_document::remote_document_encode_preview_cmd,
            $crate::plugins::ssh::commands::sftp_chmod_cmd,
            $crate::plugins::ssh::commands::sftp_delete_cmd,
            $crate::plugins::ssh::commands::sftp_rename_cmd,
            $crate::plugins::ssh::commands::sftp_mkdir_cmd,
            $crate::plugins::ssh::commands::sftp_new_file_cmd,
            $crate::plugins::ssh::commands::sftp_delete_batch_cmd,
            $crate::plugins::ssh::commands::sftp_delete_recursive_cmd,
            $crate::plugins::ssh::commands::start_journald_stream,
            $crate::plugins::ssh::commands::stop_journald_stream,
            $crate::plugins::ssh::commands::journald_query_cmd,
            $crate::plugins::ssh::commands::start_journald_export,
            $crate::plugins::ssh::commands::stop_journald_export,
            $crate::plugins::ssh::commands::get_ssh_home_dir,
            $crate::plugins::ssh::commands::confirm_host_key,
            $crate::plugins::ssh::commands::confirm_host_key_change,
            $crate::plugins::ssh::commands::list_ssh_known_hosts,
            $crate::plugins::ssh::commands::forget_ssh_known_host,
            $crate::plugins::ssh::commands::reset_ssh_known_hosts,
            $crate::plugins::tftp::commands::tftp_server_start,
            $crate::plugins::tftp::commands::tftp_server_stop,
            $crate::plugins::tftp::commands::tftp_client_get,
            $crate::plugins::tftp::commands::tftp_client_put,
            $crate::plugins::tftp::commands::tftp_update_params,
            $crate::plugins::tftp::commands::tftp_get_status,
            $crate::plugins::iperf::commands::iperf_server_start,
            $crate::plugins::iperf::commands::iperf_server_stop,
            $crate::plugins::iperf::commands::iperf_client_run,
            $crate::plugins::iperf::commands::iperf_client_stop,
            $crate::plugins::iperf::commands::iperf_update_params,
            $crate::plugins::iperf::commands::iperf_get_status,
        ]
    };
}

fn parse_manifest(raw: &'static str) -> PluginManifest {
    serde_json::from_str::<PluginManifest>(raw).expect("canonical plugin manifest")
}

fn register_adapter<T>(
    runtime: &mut PluginRuntime,
    raw_manifest: &'static str,
    adapter: T,
    connector: SessionConnectHandler,
) where
    T: ProtocolAdapter + Any + Send + Sync + 'static,
{
    let plugin_id = runtime
        .register_adapter(parse_manifest(raw_manifest), adapter)
        .unwrap_or_else(|error| panic!("注册内建协议插件失败: {error}"));
    runtime
        .register_contribution(&plugin_id, connector)
        .unwrap_or_else(|error| panic!("注册内建插件连接 contribution 失败: {error}"));
}

fn plugin_id(value: &'static str) -> PluginId {
    PluginId::parse(value).expect("built-in plugin id")
}

fn contribution<T>(runtime: &PluginRuntime, id: &'static str) -> Arc<T>
where
    T: Any + Send + Sync + 'static,
{
    runtime
        .contribution_by_str::<T>(id)
        .unwrap_or_else(|| panic!("built-in plugin contribution '{id}' is not registered"))
}

/// 构建唯一的后端内建插件运行时目录。
pub fn build_runtime() -> PluginRuntime {
    let mut runtime = PluginRuntime::new();

    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/serial.json"),
        serial::SerialAdapter::new(),
        serial::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/ssh.json"),
        ssh::SshAdapter::new(),
        ssh::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/telnet.json"),
        telnet::TelnetAdapter::new(),
        telnet::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/local-shell.json"),
        local_shell::LocalShellAdapter::new(),
        local_shell::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/tftp.json"),
        tftp::TftpAdapter::new(),
        tftp::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/iperf.json"),
        iperf::IperfAdapter::new(),
        iperf::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/network.json"),
        network::NetworkAdapter::new(),
        network::commands::session_connector,
    );
    register_adapter(
        &mut runtime,
        include_str!("../../../src/plugin-manifests/modbus.json"),
        modbus::ModbusAdapter::new(),
        modbus::session_connector,
    );

    for (id, hook) in [
        (
            tftp::PLUGIN_ID,
            tftp::commands::session_disconnected as SessionDisconnectedHook,
        ),
        (
            iperf::PLUGIN_ID,
            iperf::commands::session_disconnected as SessionDisconnectedHook,
        ),
    ] {
        runtime
            .register_contribution(&plugin_id(id), hook)
            .unwrap_or_else(|error| panic!("注册 Session 断开 contribution 失败: {error}"));
    }

    for (id, handler) in [
        (ssh::PLUGIN_ID, ssh::application::session_config_handler()),
        (
            local_shell::PLUGIN_ID,
            local_shell::session_config_handler(),
        ),
    ] {
        runtime
            .register_contribution(&plugin_id(id), handler)
            .unwrap_or_else(|error| panic!("注册 Session 配置 contribution 失败: {error}"));
    }

    let trdp_id = runtime
        .register_manifest(parse_manifest(include_str!(
            "../../../src/plugin-manifests/trdp.json"
        )))
        .unwrap_or_else(|error| panic!("注册 TRDP 插件失败: {error}"));
    runtime
        .register_contribution(&trdp_id, trdp::session_connector as SessionConnectHandler)
        .unwrap_or_else(|error| panic!("注册 TRDP 连接 contribution 失败: {error}"));
    runtime
        .register_contribution(&trdp_id, trdp::TrdpPlugin::new())
        .unwrap_or_else(|error| panic!("注册 TRDP runtime contribution 失败: {error}"));
    runtime
        .register_contribution(&trdp_id, trdp::session_config_handler())
        .unwrap_or_else(|error| panic!("注册 TRDP Session 配置 contribution 失败: {error}"));

    let rtt_id = runtime
        .register_manifest(parse_manifest(include_str!(
            "../../../src/plugin-manifests/rtt.json"
        )))
        .unwrap_or_else(|error| panic!("注册 RTT 插件失败: {error}"));
    runtime
        .register_contribution(&rtt_id, rtt::session_connector as SessionConnectHandler)
        .unwrap_or_else(|error| panic!("注册 RTT 连接 contribution 失败: {error}"));
    runtime
        .register_contribution(&rtt_id, rtt::RttPlugin::new())
        .unwrap_or_else(|error| panic!("注册 RTT runtime contribution 失败: {error}"));
    runtime
        .register_contribution(&rtt_id, rtt::session_config_handler())
        .unwrap_or_else(|error| panic!("注册 RTT Session 配置 contribution 失败: {error}"));

    runtime
}

/// 配置需要应用配置目录的插件持久化资源。
pub fn configure_persistence(runtime: &PluginRuntime, config_dir: &Path) -> Result<(), String> {
    contribution::<ssh::SshAdapter>(runtime, ssh::PLUGIN_ID)
        .configure_known_hosts(config_dir.join("known_hosts.json"))
        .map_err(|error| error.to_string())
}

/// 注入只在 Tauri App 建立后才能取得的宿主句柄。
pub fn attach_app_handle(runtime: &PluginRuntime, app: AppHandle) {
    contribution::<telnet::TelnetAdapter>(runtime, telnet::PLUGIN_ID).inject_app_handle(app);
}

/// Windows 提权 shell helper 在 Tauri runtime 建立前执行，因此也由插件 catalog 暴露统一入口。
#[cfg(windows)]
pub fn maybe_run_elevated_shell_helper() -> bool {
    local_shell::elevated::maybe_run_helper()
}
