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
    let tail = &source[start..];
    let end = tail
        .find("/// 创建共享 on_data 回调")
        .expect("connect_session end marker");
    let router = &tail[..end];

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
    for plugin_id in ["ssh", "iperf", "trdp", "network", "serial", "modbus"] {
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
