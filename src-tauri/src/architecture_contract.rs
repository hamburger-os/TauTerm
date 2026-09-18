//! Architecture regression tests for dependency direction and plugin composition.

fn read_source(relative: &str) -> String {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(manifest_dir.join("src").join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

fn read_workspace_source(relative: &str) -> String {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("src-tauri must have workspace parent");
    std::fs::read_to_string(workspace_root.join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

fn workspace_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have workspace parent")
        .join(relative)
}

#[test]
fn kernel_does_not_depend_on_concrete_plugins() {
    let kernel_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/kernel");
    for entry in std::fs::read_dir(&kernel_dir).expect("kernel directory") {
        let entry = entry.expect("kernel entry");
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("kernel source");
        assert!(
            !source.contains("crate::plugins::"),
            "kernel source {} must not depend on a concrete plugin",
            path.display()
        );
    }
}

#[test]
fn app_state_does_not_own_concrete_protocol_adapters() {
    let source = include_str!("lib.rs");
    let start = source
        .find("pub struct AppState {")
        .expect("AppState start");
    let tail = &source[start..];
    let end = tail.find("\n}").expect("AppState end");
    let app_state = &tail[..end];

    assert!(
        !app_state.contains("_adapter:"),
        "AppState must store PluginRuntime instead of per-plugin adapters"
    );
    assert!(
        !app_state.contains("PluginHost"),
        "legacy manifest-only PluginHost must not return"
    );
    assert!(
        !app_state.contains("HostKeyVerifier"),
        "plugin-private state must remain inside its plugin"
    );
}

#[test]
fn common_connection_router_is_registry_driven() {
    let source = include_str!("commands.rs");
    let start = source
        .find("pub async fn connect_session(")
        .expect("connect_session start");
    let router = &source[start..];

    assert!(router.contains("contribution::<SessionConnectHandler>"));
    assert!(
        !router.contains("match pid.as_str()"),
        "common connection router must not branch on concrete plugin IDs"
    );
}

#[test]
fn plugin_session_runtime_indices_are_plugin_owned() {
    fn visit(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("plugin directory") {
            let path = entry.expect("plugin entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugins");
    let mut files = Vec::new();
    visit(&plugins_dir, &mut files);
    for path in files {
        let source = std::fs::read_to_string(&path).expect("plugin source");
        assert!(
            !source.contains("fn runtime_registry(") && !source.contains("static REGISTRY: std::sync::OnceLock"),
            "plugin source {} must keep Session runtime indices on a registered plugin/Adapter instance",
            path.display()
        );
    }
}

#[test]
fn common_session_config_commands_are_plugin_driven() {
    let source = read_source("commands.rs");
    assert!(source.contains("SessionConfigHandler"));
    for forbidden in [
        r#"pid == "ssh""#,
        r#"pid == "trdp""#,
        r#"pid == "local-shell""#,
        "SSH_CREDENTIAL_ACCOUNT_KEY",
        "prepare_ssh_session_params",
        "scrub_ssh_secrets_from_saved_sessions",
    ] {
        assert!(
            !source.contains(forbidden),
            "common Session config commands must not own plugin policy: {forbidden}"
        );
    }
}

#[test]
fn kernel_transfer_protocol_id_is_provider_agnostic() {
    let source = read_source("kernel/plugin_adapter.rs");
    for protocol in ["xmodem", "ymodem", "zmodem", "sftp", "ftp"] {
        assert!(
            !source.contains(protocol),
            "kernel/plugin_adapter.rs must not encode concrete transfer provider '{protocol}'"
        );
    }
    assert!(
        !source.contains("TransferExecutionMode"),
        "transfer execution policy belongs to transfer/, not kernel/"
    );
}

#[test]
fn common_session_ipc_has_no_serial_compatibility_default() {
    let commands = read_source("commands.rs");
    assert!(
        !commands.contains("unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID"),
        "generic session IPC must require plugin_id instead of defaulting to Serial"
    );
    assert!(
        !commands.contains(r#"plugin_id.unwrap_or_else(|| "serial""#),
        "saved-session IPC must require plugin_id instead of defaulting to Serial"
    );

    let frontend = read_workspace_source("src/context/SessionContext.tsx");
    assert!(
        !frontend.contains(r#"pluginId || "serial""#),
        "frontend generic session API must require pluginId"
    );
    assert!(
        !frontend.contains(r#"transferProtocol || "ymodem""#),
        "frontend generic session API must not inject a Serial transfer protocol"
    );
}

#[test]
fn frontend_plugin_registry_does_not_expose_protocol_private_status_state() {
    let source = read_workspace_source("src/core/plugin-registry.ts");
    for private_field in [
        "virtualVirtualEndpoints",
        "virtualPortError",
        "virtualPortErrorKind",
        "journaldEnabled",
        "fileServiceEnabled",
    ] {
        assert!(
            !source.contains(private_field),
            "generic frontend plugin registry must not expose protocol-private field '{private_field}'"
        );
    }
}

#[test]
fn frontend_session_presentation_is_plugin_driven() {
    let source = read_workspace_source("src/components/Layout/sessionPresentation.ts");
    assert!(
        source.contains("pluginRegistry") && source.contains("sessionPresentation"),
        "common session presentation must delegate to PluginRegistration.sessionPresentation"
    );
    for plugin_id in ["ssh", "iperf", "trdp", "rtt", "network", "serial", "modbus"] {
        assert!(
            !source.contains(&format!(r#"pluginId === "{plugin_id}""#)),
            "common session presentation must not branch on built-in plugin '{plugin_id}'"
        );
    }
}

#[test]
fn frontend_session_policy_is_plugin_driven() {
    let source = read_workspace_source("src/context/SessionContext.tsx");
    assert!(
        source.contains("persistedConnectionParams") && source.contains("reconnectGuard"),
        "common SessionContext must consume plugin-owned persistence/reconnect contributions"
    );
    assert!(
        !source.contains(r#"pluginId === "tftp""#)
            && !source.contains(r#"tab.pluginId === "tftp""#),
        "TFTP reconnect policy must stay inside the TFTP plugin"
    );
    assert!(
        !source.contains(r#"pluginId !== "ssh""#)
            && !source.contains("delete sanitized.password")
            && !source.contains("delete sanitized.private_key"),
        "SSH persistence policy must stay inside the SSH plugin"
    );
}

#[test]
fn frontend_has_single_plugin_registry_for_presentation() {
    assert!(
        !workspace_path("src/core/session-presentation-registry.ts").exists(),
        "presentation metadata must live in PluginRegistry; mirrored registries are forbidden"
    );
    let contracts = read_workspace_source("src/core/plugin-contracts.ts");
    assert!(
        !contracts.contains("from \"react\"")
            && !contracts.contains("from 'react'")
            && !contracts.contains("from \"../context/SessionContext")
            && !contracts.contains("from '../../context/SessionContext")
            && !contracts.contains("from \"../i18n")
            && !contracts.contains("from '../../i18n"),
        "frontend plugin contracts must stay dependency-free"
    );
}

#[test]
fn frontend_app_shell_is_plugin_driven() {
    let app = read_workspace_source("src/App.tsx");
    for plugin_id in [
        "ssh",
        "network",
        "local-shell",
        "serial",
        "telnet",
        "trdp",
        "rtt",
        "modbus",
    ] {
        assert!(
            !app.contains(&format!(r#"manifest.id === "{plugin_id}""#))
                && !app.contains(&format!(r#"pluginId === "{plugin_id}""#)),
            "App Shell must not branch on built-in plugin '{plugin_id}'"
        );
    }
    assert!(!app.contains("ssh-host-key-verify"));
    assert!(!app.contains("file_service_enabled"));
    assert!(!app.contains("journald_enabled"));
}

#[test]
fn common_commands_do_not_own_protocol_application_surfaces() {
    let source = read_source("commands.rs");
    for forbidden in [
        "crate::plugins::",
        "connect_session_serial",
        "connect_session_ssh",
        "connect_session_telnet",
        "connect_session_local_shell",
        "connect_session_tftp",
        "connect_session_iperf",
        "connect_session_network",
        "confirm_host_key",
        "sftp_list_dir_cmd",
        "journald_query_cmd",
        "network_udp_send",
        "tftp_server_start",
        "iperf_server_start",
    ] {
        assert!(
            !source.contains(forbidden),
            "common commands.rs must stay protocol-agnostic: {forbidden}"
        );
    }
}

#[test]
fn plugin_connectors_depend_on_application_contract_not_common_commands() {
    for relative in ["plugins/modbus/mod.rs", "plugins/trdp.rs"] {
        let source = read_source(relative);
        assert!(
            !source.contains("crate::commands::ConnectSessionRequest"),
            "{relative} must consume plugin_application connector contracts"
        );
    }
}

#[test]
fn generic_session_application_payload_has_no_protocol_private_fields() {
    let source = read_source("plugin_application.rs");
    for forbidden in [
        "file_service_enabled",
        "file_service_protocol",
        "journald_enabled",
    ] {
        assert!(
            !source.contains(forbidden),
            "generic Session application orchestration must not project SSH-private field {forbidden}"
        );
    }
}

#[test]
fn session_data_plane_registration_is_two_phase() {
    let runtime = read_source("session/runtime.rs");
    assert!(runtime.contains("pub fn attach_paused("));
    assert!(runtime.contains("pub fn activate(&self)"));

    let store = read_source("kernel/session_store.rs");
    assert!(store.contains("SessionDataPlane::attach_paused"));
    assert!(store.contains("pub fn activate_data_plane(&self"));

    let application = read_source("plugin_application.rs");
    assert!(application.contains("SessionDataPlane::attach_paused"));
    assert!(application.contains("activate_data_plane(&channel_id)"));

    let network = read_source("plugins/network/mod.rs");
    assert!(network.contains("SessionDataPlane::attach_paused"));
    assert!(network.contains("activate_data_plane(&channel_id)"));
}

#[test]
fn frontend_session_error_presentation_is_plugin_driven() {
    let session_context = read_workspace_source("src/context/SessionContext.tsx");
    assert!(session_context.contains("formatSessionError"));
    assert!(!session_context.contains("User cancelled the UAC elevation prompt"));
    assert!(!session_context.contains("localShell.elevationCancelled"));

    let local_shell = read_workspace_source("src/plugins/local-shell/index.ts");
    assert!(local_shell.contains("formatSessionError"));
    assert!(local_shell.contains("localShell.elevationCancelled"));
}

#[test]
fn network_connector_rolls_back_failed_runtime_startup() {
    let source = read_source("plugins/network/commands.rs");
    assert!(source.contains("fn rollback_startup_session("));
    assert!(source.contains("network_runtime.start(app.clone(), &sid)"));
    assert!(
        source
            .matches("rollback_startup_session(&state, &sid")
            .count()
            >= 2,
        "Network connector must clean up both missing-runtime and runtime-start failures"
    );
}

#[test]
fn frontend_plugins_are_installed_from_one_explicit_catalog() {
    let main = read_workspace_source("src/main.tsx");
    assert!(main.contains("installBuiltinPlugins()"));
    for legacy_import in [
        "./plugins/serial",
        "./plugins/ssh",
        "./plugins/telnet",
        "./plugins/local-shell",
        "./plugins/tftp",
        "./plugins/iperf",
        "./plugins/network",
        "./plugins/trdp",
        "./plugins/modbus",
    ] {
        assert!(
            !main.contains(legacy_import),
            "main.tsx must not register concrete built-in plugin through side-effect import: {legacy_import}"
        );
    }

    let registry = read_workspace_source("src/core/plugin-registry.ts");
    assert!(registry.contains("export function definePlugin("));
    assert!(registry.contains("export function installPlugins("));
    assert!(
        !registry.contains("unregisterPlugin") && !registry.contains("unregister(pluginId"),
        "compile-time built-ins must not expose fake dynamic-unload APIs"
    );
}

#[test]
fn frontend_and_backend_catalogs_cover_every_canonical_manifest() {
    let frontend_catalog = read_workspace_source("src/plugins/catalog.ts");
    let backend_catalog = read_source("plugins/catalog.rs");
    let manifests_dir = workspace_path("src/plugin-manifests");

    for entry in std::fs::read_dir(manifests_dir).expect("plugin manifests directory") {
        let path = entry.expect("plugin manifest entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("manifest filename");
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .expect("manifest stem");

        assert!(
            frontend_catalog.contains(&format!(r#"from "./{stem}""#)),
            "frontend catalog must include canonical manifest plugin '{stem}'"
        );
        assert!(
            backend_catalog.contains(&format!("src/plugin-manifests/{file_name}")),
            "backend catalog must include canonical manifest '{file_name}'"
        );
    }
}

#[test]
fn application_bootstrap_does_not_assemble_concrete_protocol_adapters() {
    let source = include_str!("lib.rs");
    assert!(source.contains("plugins::catalog::build_runtime()"));
    assert!(source.contains("plugins::catalog::configure_persistence"));
    assert!(source.contains("plugins::catalog::attach_app_handle"));

    for forbidden in [
        "SerialAdapter::new",
        "SshAdapter::new",
        "TelnetAdapter::new",
        "TftpAdapter::new",
        "IperfAdapter::new",
        "NetworkAdapter::new",
        "ModbusAdapter::new",
        ".configure_known_hosts(",
        ".inject_app_handle(",
    ] {
        assert!(
            !source.contains(forbidden),
            "lib.rs must delegate concrete plugin assembly/lifecycle to plugins::catalog: {forbidden}"
        );
    }
}

#[test]
fn connect_dialog_uses_registry_session_option_policy() {
    let source = read_workspace_source("src/components/Layout/ConnectDialog.tsx");
    assert!(source.contains("pluginRegistry.getDefaultSessionOptions(modeId)"));
    assert!(source.contains("pluginRegistry.resolveSessionOptions"));
    assert!(
        !source.contains("function defaultSessionOptions("),
        "ConnectDialog must not maintain a second copy of plugin default-session policy"
    );
}

#[test]
fn native_rtt_uses_shared_embedded_debug_service_capability() {
    let backend = read_source("plugins/rtt/backend/probe_rs.rs");
    assert!(
        backend.contains("DebugServiceLease") && backend.contains(".service"),
        "native RTT must execute target I/O through an embedded-debug service lease"
    );
    assert!(
        !backend.contains("DebugProbeRuntime"),
        "RTT backend must not own the concrete probe runtime directly"
    );

    let runtime = read_source("embedded_debug/runtime.rs");
    assert!(
        runtime.contains("resolve_probe_config(config)?")
            && runtime.contains("targets: Mutex<HashMap<String, TargetSlot>>"),
        "embedded-debug ownership must be keyed by canonical physical probe identity"
    );
    assert!(
        runtime.contains("TargetConfigConflict"),
        "one physical probe must reject a second incompatible target configuration"
    );
    assert!(
        runtime.contains("TargetSlotState::Opening")
            && runtime.contains("worker_exited: Arc<AtomicBool>"),
        "probe ownership must remain reserved while a timed-out startup worker is still exiting"
    );
    assert!(
        runtime.contains("SERVICE_QUEUE_CAPACITY")
            && runtime.contains("service_order: VecDeque<String>")
            && runtime.contains("state.service_order.push_back"),
        "shared target scheduling must keep bounded per-service queues with round-robin order"
    );

    let observation = read_source("embedded_debug/observation.rs");
    assert!(
        observation.contains("pub struct ObservationSource")
            && observation.contains("mpsc::sync_channel")
            && observation.contains("subscriber.sender.try_send")
            && observation.contains("pub fn dropped(&self) -> u64"),
        "embedded observation consumers must share a typed bounded source instead of starting duplicate hardware readers"
    );

    let rtt_runtime = read_source("plugins/rtt/runtime.rs");
    assert!(
        rtt_runtime.contains("raw_source: ObservationSource<StoredRttChunk>")
            && rtt_runtime.contains("self.raw_source.publish(&chunk)")
            && rtt_runtime.contains("pub fn subscribe_observations"),
        "RTT acquisition must publish canonical frames to the shared typed observation source"
    );
}

#[test]
fn virtual_port_backend_hides_platform_elevation_mechanics() {
    let backend = read_source("virtual_port/backend.rs");
    for forbidden in [
        "install_driver_elevated",
        "create_endpoints_elevated",
        "cleanup_endpoints_elevated",
    ] {
        assert!(
            !backend.contains(forbidden),
            "VirtualPortBackend must keep privilege selection private: {forbidden}"
        );
    }

    let manager = read_source("virtual_port/manager.rs");
    for forbidden in ["cmd.exe", ".cmd", "ShellExecuteExW", "PortName=-"] {
        assert!(
            !manager.contains(forbidden),
            "VirtualPortManager must not rebuild unsafe legacy virtual-port mechanics: {forbidden}"
        );
    }
    assert!(
        manager.contains("endpoints_by_bus") && manager.contains("identity.matches(endpoint)"),
        "virtual-port deletion must remain gated by exact driver identity"
    );
    assert!(
        manager.contains("reconcile_owned_state_locked")
            && manager.contains("destroy_endpoint_privileged_locked"),
        "virtual-port privileged transactions must keep one mutation-lock owner and use locked helpers internally"
    );
    assert!(
        manager.contains("current protected ownership no longer authorizes this identity"),
        "virtual-port deletion must re-check protected ownership under the mutation lock"
    );

    #[cfg(windows)]
    {
        let elevated = read_source("virtual_port/elevated.rs");
        assert!(elevated.contains("--tauterm-vport-helper"));
        assert!(elevated.contains("GetNamedPipeClientProcessId"));
        assert!(elevated.contains("GetNamedPipeServerProcessId"));
        assert!(!elevated.contains("cmd.exe"));
        assert!(!elevated.contains("state_dir: PathBuf"));

        let state = read_source("virtual_port/windows_state.rs");
        assert!(
            state.contains("FOLDERID_ProgramData"),
            "virtual-port ownership must resolve ProgramData from the Windows known-folder API"
        );
        assert!(state.contains("Authenticated Users"));
        assert!(state.contains("PROTECTED_DACL_SECURITY_INFORMATION"));
        assert!(state.contains("OWNER_SECURITY_INFORMATION"));
    }
}
