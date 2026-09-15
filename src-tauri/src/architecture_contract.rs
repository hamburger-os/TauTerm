//! Architecture regression tests for dependency direction and plugin composition.

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
