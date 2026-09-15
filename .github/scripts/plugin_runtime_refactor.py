from __future__ import annotations

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def replace_regex_once(text: str, pattern: str, replacement: str, label: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return updated


PLUGIN_RUNTIME = r'''//! 插件运行时目录。
//!
//! `PluginRuntime` 是后端插件身份、canonical manifest、通用 `ProtocolAdapter` 与
//! 类型化 contribution 的唯一注册表。具体插件只在应用 composition root 注册；
//! Kernel 消费插件 API/capability，不维护协议名分支或平行 adapter 字段。

use crate::kernel::plugin_adapter::{PluginId, PluginManifest, ProtocolAdapter};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

struct PluginEntry {
    manifest: PluginManifest,
    adapter: Option<Arc<dyn ProtocolAdapter>>,
    contributions: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

pub struct PluginRuntime {
    plugins: HashMap<PluginId, PluginEntry>,
    order: Vec<PluginId>,
}

impl PluginRuntime {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn register_manifest(
        &mut self,
        manifest: PluginManifest,
    ) -> Result<PluginId, PluginRuntimeError> {
        self.insert_entry(manifest, None, HashMap::new())
    }

    pub fn register_adapter<T>(
        &mut self,
        manifest: PluginManifest,
        adapter: T,
    ) -> Result<PluginId, PluginRuntimeError>
    where
        T: ProtocolAdapter + Any + Send + Sync + 'static,
    {
        let declared = adapter
            .plugin_id()
            .ok_or_else(|| PluginRuntimeError::MissingAdapterId(std::any::type_name::<T>()))?;
        let declared = PluginId::parse(declared).map_err(PluginRuntimeError::InvalidPluginId)?;
        if manifest.id != declared {
            return Err(PluginRuntimeError::AdapterIdMismatch {
                manifest: manifest.id,
                adapter: declared,
            });
        }

        let adapter = Arc::new(adapter);
        let protocol_adapter: Arc<dyn ProtocolAdapter> = adapter.clone();
        let erased: Arc<dyn Any + Send + Sync> = adapter;
        let mut contributions = HashMap::new();
        contributions.insert(TypeId::of::<T>(), erased);
        self.insert_entry(manifest, Some(protocol_adapter), contributions)
    }

    pub fn register_contribution<T>(
        &mut self,
        plugin_id: &PluginId,
        contribution: T,
    ) -> Result<(), PluginRuntimeError>
    where
        T: Any + Send + Sync + 'static,
    {
        let entry = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginRuntimeError::NotRegistered(plugin_id.clone()))?;
        let type_id = TypeId::of::<T>();
        if entry.contributions.contains_key(&type_id) {
            return Err(PluginRuntimeError::ContributionAlreadyRegistered {
                plugin_id: plugin_id.clone(),
                contribution: std::any::type_name::<T>(),
            });
        }
        entry.contributions.insert(type_id, Arc::new(contribution));
        Ok(())
    }

    pub fn manifests(&self) -> Vec<&PluginManifest> {
        self.order
            .iter()
            .filter_map(|plugin_id| self.plugins.get(plugin_id).map(|entry| &entry.manifest))
            .collect()
    }

    pub fn manifest(&self, plugin_id: &PluginId) -> Option<&PluginManifest> {
        self.plugins.get(plugin_id).map(|entry| &entry.manifest)
    }

    pub fn manifest_by_str(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.plugins.get(plugin_id).map(|entry| &entry.manifest)
    }

    pub fn adapter(&self, plugin_id: &PluginId) -> Option<Arc<dyn ProtocolAdapter>> {
        self.plugins
            .get(plugin_id)
            .and_then(|entry| entry.adapter.clone())
    }

    pub fn adapter_by_str(&self, plugin_id: &str) -> Option<Arc<dyn ProtocolAdapter>> {
        self.plugins
            .get(plugin_id)
            .and_then(|entry| entry.adapter.clone())
    }

    pub fn contribution<T>(&self, plugin_id: &PluginId) -> Option<Arc<T>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.plugins
            .get(plugin_id)?
            .contributions
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast::<T>()
            .ok()
    }

    pub fn contribution_by_str<T>(&self, plugin_id: &str) -> Option<Arc<T>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.plugins
            .get(plugin_id)?
            .contributions
            .get(&TypeId::of::<T>())?
            .clone()
            .downcast::<T>()
            .ok()
    }

    pub fn has_capability(&self, plugin_id: &PluginId, capability: &str) -> bool {
        self.manifest(plugin_id)
            .is_some_and(|manifest| manifest.capabilities.iter().any(|item| item == capability))
    }

    fn insert_entry(
        &mut self,
        manifest: PluginManifest,
        adapter: Option<Arc<dyn ProtocolAdapter>>,
        contributions: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    ) -> Result<PluginId, PluginRuntimeError> {
        let plugin_id = manifest.id.clone();
        if self.plugins.contains_key(&plugin_id) {
            return Err(PluginRuntimeError::AlreadyRegistered(plugin_id));
        }
        self.order.push(plugin_id.clone());
        self.plugins.insert(
            plugin_id.clone(),
            PluginEntry {
                manifest,
                adapter,
                contributions,
            },
        );
        Ok(plugin_id)
    }
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginRuntimeError {
    #[error("插件 '{0}' 已注册")]
    AlreadyRegistered(PluginId),
    #[error("插件 '{0}' 未注册")]
    NotRegistered(PluginId),
    #[error("Adapter {0} 未声明 plugin_id")]
    MissingAdapterId(&'static str),
    #[error("无效插件 ID: {0}")]
    InvalidPluginId(String),
    #[error("插件 manifest ID '{manifest}' 与 Adapter ID '{adapter}' 不一致")]
    AdapterIdMismatch {
        manifest: PluginId,
        adapter: PluginId,
    },
    #[error("插件 '{plugin_id}' 已注册 contribution {contribution}")]
    ContributionAlreadyRegistered {
        plugin_id: PluginId,
        contribution: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::plugin_adapter::ProtocolConnection;
    use crate::session::SessionError;

    struct DummyAdapter;

    #[async_trait::async_trait]
    impl ProtocolAdapter for DummyAdapter {
        fn plugin_id(&self) -> Option<&'static str> {
            Some("dummy")
        }

        async fn connect(
            &self,
            _endpoint: &str,
            _params: &serde_json::Value,
        ) -> Result<ProtocolConnection, SessionError> {
            Err(SessionError::CapabilityDenied {
                capability: "dummy".into(),
            })
        }
    }

    fn manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(id).unwrap(),
            name: id.to_string(),
            version: "1".into(),
            category: "test".into(),
            description: String::new(),
            icon: "terminal".into(),
            content_type: "terminal".into(),
            send_bar: false,
            capabilities: vec!["session".into()],
            transfer_protocols: Vec::new(),
        }
    }

    #[test]
    fn adapter_manifest_and_typed_contribution_share_one_entry() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime
            .register_adapter(manifest("dummy"), DummyAdapter)
            .unwrap();

        assert_eq!(runtime.manifests().len(), 1);
        assert!(runtime.adapter(&plugin_id).is_some());
        assert!(runtime.contribution::<DummyAdapter>(&plugin_id).is_some());
        assert!(runtime.has_capability(&plugin_id, "session"));
    }

    #[test]
    fn manifest_only_plugin_does_not_fake_a_protocol_adapter() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime.register_manifest(manifest("custom")).unwrap();
        assert!(runtime.adapter(&plugin_id).is_none());
    }

    #[test]
    fn duplicate_plugin_and_duplicate_contribution_fail_closed() {
        let mut runtime = PluginRuntime::new();
        let plugin_id = runtime
            .register_adapter(manifest("dummy"), DummyAdapter)
            .unwrap();
        assert!(matches!(
            runtime.register_manifest(manifest("dummy")),
            Err(PluginRuntimeError::AlreadyRegistered(_))
        ));

        runtime.register_contribution(&plugin_id, 7_u32).unwrap();
        assert!(matches!(
            runtime.register_contribution(&plugin_id, 8_u32),
            Err(PluginRuntimeError::ContributionAlreadyRegistered { .. })
        ));
    }

    #[test]
    fn adapter_identity_must_match_manifest_identity() {
        let mut runtime = PluginRuntime::new();
        assert!(matches!(
            runtime.register_adapter(manifest("other"), DummyAdapter),
            Err(PluginRuntimeError::AdapterIdMismatch { .. })
        ));
    }
}
'''

ARCHITECTURE_CONTRACT = r'''//! Architecture regression tests for dependency direction and plugin composition.

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
    let start = source.find("pub struct AppState {").expect("AppState start");
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
'''

# ---------------------------------------------------------------------------
# plugin API + runtime
# ---------------------------------------------------------------------------
plugin_adapter_path = "src-tauri/src/kernel/plugin_adapter.rs"
plugin_adapter = read(plugin_adapter_path)
plugin_id_code = r'''
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || !value
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        {
            return Err(format!(
                "'{value}'（仅允许小写 a-z、0-9、-、_）"
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for PluginId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for PluginId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PluginId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

'''
plugin_adapter = replace_once(
    plugin_adapter,
    "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct PluginManifest {\n    pub id: String,",
    plugin_id_code + "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct PluginManifest {\n    pub id: PluginId,",
    "PluginManifest PluginId",
)
plugin_adapter = replace_once(
    plugin_adapter,
    "pub trait ProtocolAdapter: Send + Sync {\n    async fn connect(",
    "pub trait ProtocolAdapter: Send + Sync {\n    /// Adapter 自己声明稳定插件身份；PluginRuntime 在 bootstrap 时校验它与 manifest 一致。\n    fn plugin_id(&self) -> Option<&'static str> {\n        None\n    }\n\n    async fn connect(",
    "ProtocolAdapter plugin id",
)
write(plugin_adapter_path, plugin_adapter)
write("src-tauri/src/kernel/plugin_runtime.rs", PLUGIN_RUNTIME)

kernel_mod_path = "src-tauri/src/kernel/mod.rs"
kernel_mod = read(kernel_mod_path)
kernel_mod = kernel_mod.replace("Plugin Host", "Plugin Runtime")
kernel_mod = kernel_mod.replace("`plugin_host`     — canonical PluginManifest 的运行时注册与能力查询", "`plugin_runtime`  — canonical PluginManifest、ProtocolAdapter 与类型化 contribution 的唯一运行时目录")
kernel_mod = replace_once(kernel_mod, "pub mod plugin_host;", "pub mod plugin_runtime;", "kernel runtime module")
write(kernel_mod_path, kernel_mod)

old_host = ROOT / "src-tauri/src/kernel/plugin_host.rs"
if not old_host.exists():
    raise RuntimeError("legacy plugin_host.rs missing")
old_host.unlink()

# ---------------------------------------------------------------------------
# Built-in adapters own stable identity. SSH also owns host-key verification.
# ---------------------------------------------------------------------------
adapters = {
    "src-tauri/src/plugins/serial/mod.rs": ("SerialAdapter", "serial"),
    "src-tauri/src/plugins/ssh/mod.rs": ("SshAdapter", "ssh"),
    "src-tauri/src/plugins/telnet/mod.rs": ("TelnetAdapter", "telnet"),
    "src-tauri/src/plugins/local_shell/mod.rs": ("LocalShellAdapter", "local-shell"),
    "src-tauri/src/plugins/tftp/mod.rs": ("TftpAdapter", "tftp"),
    "src-tauri/src/plugins/iperf/mod.rs": ("IperfAdapter", "iperf"),
    "src-tauri/src/plugins/network/mod.rs": ("NetworkAdapter", "network"),
    "src-tauri/src/plugins/modbus/mod.rs": ("ModbusAdapter", "modbus"),
}

for path, (adapter_name, plugin_id) in adapters.items():
    text = read(path)
    if "pub const PLUGIN_ID:" in text:
        raise RuntimeError(f"{path}: PLUGIN_ID already exists")
    doc_match = re.match(r"(?:(?://!.*\n)|\n)*", text)
    insert_at = doc_match.end() if doc_match else 0
    text = text[:insert_at] + f'pub const PLUGIN_ID: &str = "{plugin_id}";\n\n' + text[insert_at:]
    impl_marker = f"impl ProtocolAdapter for {adapter_name} {{\n"
    if impl_marker not in text:
        raise RuntimeError(f"{path}: missing {impl_marker.strip()}")
    text = text.replace(
        impl_marker,
        impl_marker + "    fn plugin_id(&self) -> Option<&'static str> {\n        Some(PLUGIN_ID)\n    }\n\n",
        1,
    )
    write(path, text)

trdp_path = "src-tauri/src/plugins/trdp.rs"
trdp = read(trdp_path)
doc_match = re.match(r"(?:(?://!.*\n)|\n)*", trdp)
insert_at = doc_match.end() if doc_match else 0
trdp = trdp[:insert_at] + 'pub const PLUGIN_ID: &str = "trdp";\n\n' + trdp[insert_at:]
write(trdp_path, trdp)

ssh_path = "src-tauri/src/plugins/ssh/mod.rs"
ssh = read(ssh_path)
ssh = replace_once(
    ssh,
    "pub struct SshAdapter;\n\nimpl SshAdapter {\n    pub fn new() -> Self {\n        Self\n    }",
    "pub struct SshAdapter {\n    host_key_verifier: HostKeyVerifier,\n}\n\nimpl SshAdapter {\n    pub fn new() -> Self {\n        Self {\n            host_key_verifier: HostKeyVerifier::new(),\n        }\n    }\n\n    pub fn configure_known_hosts(&self, path: std::path::PathBuf) -> Result<(), String> {\n        self.host_key_verifier.configure_known_hosts(path)\n    }\n\n    pub async fn respond_to_host_key_verification(\n        &self,\n        request_id: &str,\n        accepted: bool,\n    ) -> Result<bool, String> {\n        self.host_key_verifier.respond(request_id, accepted).await\n    }",
    "SSH adapter owns verifier",
)
ssh = replace_once(
    ssh,
    "        app_handle: tauri::AppHandle,\n        verifier: &HostKeyVerifier,\n    ) -> Result<ProtocolConnection, SessionError> {\n        config.validate()?;\n        let result = build_connection_with_config(config, app_handle, verifier).await?;",
    "        app_handle: tauri::AppHandle,\n    ) -> Result<ProtocolConnection, SessionError> {\n        config.validate()?;\n        let result = build_connection_with_config(config, app_handle, &self.host_key_verifier).await?;",
    "SSH connect verifier ownership",
)
ssh = replace_once(ssh, "pub struct HostKeyVerifier {", "struct HostKeyVerifier {", "HostKeyVerifier privacy")
ssh = ssh.replace("/// 由 AppState 持有，供 `build_connection_with_config`（写入待确认项）", "/// 由 `SshAdapter` 持有，供连接流程（写入待确认项）")
write(ssh_path, ssh)

# ---------------------------------------------------------------------------
# App composition root: one PluginRuntime, no concrete adapter fields.
# ---------------------------------------------------------------------------
lib_path = "src-tauri/src/lib.rs"
lib = read(lib_path)
lib = lib.replace("- **Plugin Host**: 插件注册与发现（`kernel/plugin_host`）", "- **Plugin Runtime**: canonical manifest、Adapter 与类型化 contribution 的唯一注册目录（`kernel/plugin_runtime`）")
lib = replace_once(lib, "use kernel::plugin_host::PluginHost;", "use kernel::plugin_runtime::PluginRuntime;", "lib runtime import")
lib = lib.replace("use plugins::ssh::HostKeyVerifier;\n", "")
lib = replace_once(lib, "use std::sync::Mutex;", "use std::any::Any;\nuse std::sync::{Arc, Mutex};", "lib Any imports")

old_state = '''pub struct AppState {
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
'''
new_state = '''pub struct AppState {
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
            .unwrap_or_else(|| panic!("built-in plugin contribution '{plugin_id}' is not registered"))
    }
}
'''
lib = replace_once(lib, old_state, new_state, "AppState convergence")

lib = replace_regex_once(
    lib,
    r"fn built_in_plugin_manifests\(\) -> Vec<PluginManifest> \{.*?\n\}\n\n#\[cfg_attr\(mobile, tauri::mobile_entry_point\)\]",
    '''fn parse_builtin_manifest(raw: &'static str) -> PluginManifest {
    serde_json::from_str::<PluginManifest>(raw).expect("canonical plugin manifest")
}

fn register_builtin_adapter<T>(
    runtime: &mut PluginRuntime,
    raw_manifest: &'static str,
    adapter: T,
    connector: commands::SessionConnectHandler,
) where
    T: kernel::plugin_adapter::ProtocolAdapter + Any + Send + Sync + 'static,
{
    let plugin_id = runtime
        .register_adapter(parse_builtin_manifest(raw_manifest), adapter)
        .unwrap_or_else(|error| panic!("注册内建协议插件失败: {error}"));
    runtime
        .register_contribution(&plugin_id, connector)
        .unwrap_or_else(|error| panic!("注册内建插件连接 contribution 失败: {error}"));
}

fn build_plugin_runtime() -> PluginRuntime {
    let mut runtime = PluginRuntime::new();
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/serial.json"),
        SerialAdapter::new(),
        commands::serial_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/ssh.json"),
        SshAdapter::new(),
        commands::ssh_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/telnet.json"),
        TelnetAdapter::new(),
        commands::telnet_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/local-shell.json"),
        LocalShellAdapter::new(),
        commands::local_shell_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/tftp.json"),
        TftpAdapter::new(),
        commands::tftp_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/iperf.json"),
        IperfAdapter::new(),
        commands::iperf_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/network.json"),
        NetworkAdapter::new(),
        commands::network_session_connector,
    );
    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/modbus.json"),
        ModbusAdapter::new(),
        commands::modbus_session_connector,
    );

    let trdp_id = runtime
        .register_manifest(parse_builtin_manifest(include_str!(
            "../../src/plugin-manifests/trdp.json"
        )))
        .unwrap_or_else(|error| panic!("注册 TRDP 插件失败: {error}"));
    runtime
        .register_contribution(&trdp_id, commands::trdp_session_connector as commands::SessionConnectHandler)
        .unwrap_or_else(|error| panic!("注册 TRDP 连接 contribution 失败: {error}"));

    runtime
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]''',
    "build PluginRuntime",
)

lib = replace_regex_once(
    lib,
    r"    let mut plugin_host = PluginHost::new\(\);\n    for manifest in built_in_plugin_manifests\(\) \{.*?\n    \}\n\n    tauri::Builder::default\(\)",
    "    let plugin_runtime = build_plugin_runtime();\n\n    tauri::Builder::default()",
    "runtime bootstrap",
)
lib = replace_once(
    lib,
    '''                        if let Err(error) = state
                            .host_key_verifier
                            .configure_known_hosts(config_dir.join("known_hosts.json"))
                        {
                            log::warn!("SSH known-host 存储初始化失败: {}", error);
                        }''',
    '''                        if let Err(error) = state
                            .plugin::<SshAdapter>(plugins::ssh::PLUGIN_ID)
                            .configure_known_hosts(config_dir.join("known_hosts.json"))
                        {
                            log::warn!("SSH known-host 存储初始化失败: {}", error);
                        }''',
    "SSH known hosts setup",
)
lib = replace_once(
    lib,
    "                state.telnet_adapter.inject_app_handle(app.handle().clone());",
    "                state\n                    .plugin::<TelnetAdapter>(plugins::telnet::PLUGIN_ID)\n                    .inject_app_handle(app.handle().clone());",
    "Telnet setup contribution",
)
old_manage = '''        .manage(AppState {
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
            theme_engine: ThemeEngine::new(),'''
new_manage = '''        .manage(AppState {
            session_store: Mutex::new(SessionStore::new()),
            plugins: plugin_runtime,
            config_store: ConfigStore::new(),
            theme_engine: ThemeEngine::new(),'''
lib = replace_once(lib, old_manage, new_manage, "AppState initialization")
lib = replace_once(lib, "#[cfg(test)]\nmod performance_contract;", "#[cfg(test)]\nmod architecture_contract;\n#[cfg(test)]\nmod performance_contract;", "architecture test module")
write(lib_path, lib)
write("src-tauri/src/architecture_contract.rs", ARCHITECTURE_CONTRACT)

# ---------------------------------------------------------------------------
# Common commands resolve capabilities/contributions from PluginRuntime.
# ---------------------------------------------------------------------------
commands_path = "src-tauri/src/commands.rs"
commands = read(commands_path)
commands = replace_once(
    commands,
    "use crate::kernel::plugin_adapter::{ChannelOpenMode, ProtocolAdapter, TransferProtocolType};",
    "use crate::kernel::plugin_adapter::{\n    ChannelOpenMode, PluginId, ProtocolAdapter, TransferProtocolType,\n};",
    "commands PluginId import",
)
commands = replace_once(
    commands,
    "use std::collections::HashMap;\nuse std::sync::atomic::{AtomicBool, Ordering};",
    "use std::collections::HashMap;\nuse std::future::Future;\nuse std::pin::Pin;\nuse std::sync::atomic::{AtomicBool, Ordering};",
    "commands future imports",
)

request_end = '''pub struct ConnectSessionRequest {
    pub endpoint: String,
    pub params: Value,
    pub name: Option<String>,
    pub plugin_id: Option<String>,
    pub transfer_enabled: Option<bool>,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: Option<bool>,
    pub journald_enabled: Option<bool>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub initial_elevated: bool,
}
'''
handler_code = request_end + r'''
pub(crate) type SessionConnectFuture =
    Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'static>>;
pub(crate) type SessionConnectHandler =
    fn(AppHandle, ConnectSessionRequest) -> SessionConnectFuture;

macro_rules! define_session_connector {
    ($name:ident, $target:path) => {
        pub(crate) fn $name(
            app: AppHandle,
            request: ConnectSessionRequest,
        ) -> SessionConnectFuture {
            Box::pin(async move {
                let state: State<'_, AppState> = app.state();
                $target(app.clone(), state, request).await
            })
        }
    };
}

define_session_connector!(serial_session_connector, connect_session_serial);
define_session_connector!(ssh_session_connector, connect_session_ssh);
define_session_connector!(tftp_session_connector, connect_session_tftp);
define_session_connector!(iperf_session_connector, connect_session_iperf);
define_session_connector!(telnet_session_connector, connect_session_telnet);
define_session_connector!(local_shell_session_connector, connect_session_local_shell);
define_session_connector!(network_session_connector, connect_session_network);
define_session_connector!(trdp_session_connector, crate::plugins::trdp::connect_session);
define_session_connector!(modbus_session_connector, crate::plugins::modbus::connect_session);
'''
commands = replace_once(commands, request_end, handler_code, "session connector contributions")

commands = replace_regex_once(
    commands,
    r'''#\[tauri::command\]\npub fn get_connection_types\(state: State<'_, AppState>\) -> Vec<ConnectionTypeInfo> \{.*?\n\}\n\n// ── 命令：端点枚举''',
    '''#[tauri::command]
pub fn get_connection_types(state: State<'_, AppState>) -> Vec<ConnectionTypeInfo> {
    state
        .plugins
        .manifests()
        .into_iter()
        .map(|plugin| ConnectionTypeInfo {
            id: plugin.id.to_string(),
            label: plugin.name.clone(),
            available: true,
            description: format!("{} v{}", plugin.name, plugin.version),
            icon: plugin.category.clone(),
            content_type: plugin.content_type.clone(),
        })
        .collect()
}

// ── 命令：端点枚举''',
    "generic connection type listing",
)

commands = replace_regex_once(
    commands,
    r'''#\[tauri::command\]\npub async fn enumerate_endpoints\(.*?\n\}\n\n// ── 命令：会话连接''',
    '''#[tauri::command]
pub async fn enumerate_endpoints(
    state: State<'_, AppState>,
    plugin_id: Option<String>,
) -> Result<Vec<EndpointItem>, String> {
    let raw_plugin_id = plugin_id.unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID.into());
    let plugin_id = PluginId::parse(raw_plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;
    let adapter = state
        .plugins
        .adapter(&plugin_id)
        .ok_or_else(|| format!("插件 '{plugin_id}' 不提供 ProtocolAdapter"))?;

    // 端点发现可能触发驱动枚举、平台命令或未来的网络发现；统一放入 blocking worker，
    // 公共命令不再猜测哪些具体协议会阻塞。
    let endpoints = tauri::async_runtime::spawn_blocking(move || adapter.discover_endpoints())
        .await
        .map_err(|error| format!("插件端点发现任务失败: {error}"))?
        .map_err(|error| error.to_string())?;

    Ok(endpoints
        .into_iter()
        .map(|endpoint| EndpointItem {
            name: endpoint.name,
            description: endpoint.description,
            connection_type: plugin_id.to_string(),
            params: endpoint.params,
        })
        .collect())
}

// ── 命令：会话连接''',
    "generic endpoint discovery",
)

commands = replace_regex_once(
    commands,
    r'''pub async fn connect_session\(\n    app: AppHandle,\n    state: State<'_, AppState>,\n    request: ConnectSessionRequest,\n\) -> Result<String, String> \{.*?\n\}\n\n/// 创建共享 on_data 回调''',
    '''pub async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let raw_plugin_id = request
        .plugin_id
        .clone()
        .unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID.into());
    let plugin_id = PluginId::parse(raw_plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;
    let handler = state
        .plugins
        .contribution::<SessionConnectHandler>(&plugin_id)
        .ok_or_else(|| format!("插件 '{plugin_id}' 未注册 Session 连接 contribution"))?;

    (handler.as_ref())(app, request).await
}

/// 创建共享 on_data 回调''',
    "registry-driven connection router",
)

# SSH verifier is plugin-private now.
commands = replace_regex_once(
    commands,
    r'''    // 通过 SshAdapter::connect_with_config 获取 ProtocolConnection，\n    // 复用已解析的 SshConfig 实例，避免 connect\(\) 内部二次 JSON 反序列化。\n    // 传入 AppHandle 和 HostKeyVerifier 以启用用户确认主机密钥流程。\n    let conn = state\n        \.ssh_adapter\n        \.connect_with_config\(ssh_config\.clone\(\), app\.clone\(\), &state\.host_key_verifier\)\n        \.await\n        \.map_err\(\|e\| e\.to_string\(\)\)\?;\n\n    let content_type = state\.ssh_adapter\.content_type\(\);\n    let transfer_protocols_list = state\.ssh_adapter\.transfer_protocols\(\);''',
    '''    // SSH 插件自己持有 known-host 验证状态；AppState 只通过 PluginRuntime 取得 contribution。
    let ssh_adapter = state.plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID);
    let conn = ssh_adapter
        .connect_with_config(ssh_config.clone(), app.clone())
        .await
        .map_err(|e| e.to_string())?;

    let content_type = ssh_adapter.content_type();
    let transfer_protocols_list = ssh_adapter.transfer_protocols();''',
    "SSH connect uses plugin-owned verifier",
)
commands = replace_regex_once(
    commands,
    r'''    let ok = state\n        \.host_key_verifier\n        \.respond\(&request_id, accepted\)\n        \.await\?;''',
    '''    let ok = state
        .plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID)
        .respond_to_host_key_verification(&request_id, accepted)
        .await?;''',
    "SSH host key response",
)

# Every remaining direct AppState adapter field becomes a typed PluginRuntime contribution lookup.
state_adapters = {
    "serial_adapter": ("crate::plugins::serial::SerialAdapter", "crate::plugins::serial::PLUGIN_ID"),
    "ssh_adapter": ("crate::plugins::ssh::SshAdapter", "crate::plugins::ssh::PLUGIN_ID"),
    "tftp_adapter": ("crate::plugins::tftp::TftpAdapter", "crate::plugins::tftp::PLUGIN_ID"),
    "telnet_adapter": ("crate::plugins::telnet::TelnetAdapter", "crate::plugins::telnet::PLUGIN_ID"),
    "local_shell_adapter": ("crate::plugins::local_shell::LocalShellAdapter", "crate::plugins::local_shell::PLUGIN_ID"),
    "iperf_adapter": ("crate::plugins::iperf::IperfAdapter", "crate::plugins::iperf::PLUGIN_ID"),
    "network_adapter": ("crate::plugins::network::NetworkAdapter", "crate::plugins::network::PLUGIN_ID"),
    "modbus_adapter": ("crate::plugins::modbus::ModbusAdapter", "crate::plugins::modbus::PLUGIN_ID"),
}
for field, (ty, plugin_id) in state_adapters.items():
    pattern = rf"\bstate\s*\.\s*{field}\b"
    commands = re.sub(pattern, f"state.plugin::<{ty}>({plugin_id})", commands)

if re.search(r"\bstate\s*\.\s*(?:serial|ssh|tftp|telnet|local_shell|iperf|network|modbus)_adapter\b", commands):
    raise RuntimeError("commands.rs still contains direct adapter state access")
write(commands_path, commands)

# ---------------------------------------------------------------------------
# Frontend registry: duplicate IDs are programmer errors, never silent overrides.
# ---------------------------------------------------------------------------
registry_path = "src/core/plugin-registry.ts"
registry = read(registry_path)
registry = replace_once(
    registry,
    '''    if (this.plugins.has(id)) {
      console.warn(`[PluginRegistry] 插件 "${id}" 已注册，将被覆盖`);
    }
    this.plugins.set(id, registration);''',
    '''    if (this.plugins.has(id)) {
      throw new Error(`[PluginRegistry] 插件 "${id}" 重复注册`);
    }
    this.plugins.set(id, registration);''',
    "frontend duplicate plugin registration",
)
write(registry_path, registry)

# ---------------------------------------------------------------------------
# Architecture documentation follows the same commit.
# ---------------------------------------------------------------------------
doc_path = "docs/modules/CORE.md"
doc = read(doc_path)
doc = replace_once(
    doc,
    "后端以 Rust `SessionStore` 作为用户可见 Session 生命周期的权威所有者。协议 Adapter 负责建立协议资源，并以 `ProtocolConnection` 返回 `DataPlaneRuntime`、可选 `SessionService`、显式 `FileTransfer` capability、可选子终端工厂和 attach hook 等明确能力；核心把 DataPlane 绑定为 `SessionDataPlane + SessionIo`，统一承担收发、订阅、统计、终端 resize 和独占 I/O lease。",
    "后端以 Rust `SessionStore` 作为用户可见 Session 生命周期的权威所有者。`PluginRuntime` 是插件身份、canonical manifest、通用 `ProtocolAdapter` 与类型化 contribution 的唯一后端目录；只有应用 composition root 知道有哪些具体内建插件，`AppState` 不再为 Serial、SSH、Telnet 等维护平行 adapter 字段。协议 Adapter 负责建立协议资源，并以 `ProtocolConnection` 返回 `DataPlaneRuntime`、可选 `SessionService`、显式 `FileTransfer` capability、可选子终端工厂和 attach hook 等明确能力；核心把 DataPlane 绑定为 `SessionDataPlane + SessionIo`，统一承担收发、订阅、统计、终端 resize 和独占 I/O lease。",
    "CORE runtime paragraph",
)
doc = replace_once(
    doc,
    "- 核心只拥有可复用机制，不加入 TRDP、SSH、Modbus、串口等协议专属判断。\n- 协议连接入口最终由统一内核路由分发，避免前端入口各自实现连接生命周期。",
    "- 核心只拥有可复用机制，不加入 TRDP、SSH、Modbus、串口等协议专属判断；`kernel/` 不允许依赖 `crate::plugins::*`。\n- 协议连接入口由 `PluginRuntime` 中注册的类型化 Session connector contribution 分发；公共 `connect_session` 不按插件 ID `match`。新增内建插件只在 composition root 注册 manifest、Adapter/能力和 connector。\n- 插件专属运行态必须由插件对象或 Session capability 持有，不把 SSH known-host verifier、协议 runtime registry 等字段泄漏到 `AppState`。",
    "CORE boundaries",
)
doc = replace_once(doc, "- `src-tauri/src/kernel/plugin_host.rs`", "- `src-tauri/src/kernel/plugin_runtime.rs`", "CORE code anchor")
doc = replace_once(
    doc,
    "内建插件元数据位于 `src/plugin-manifests/*.json`，TypeScript PluginRegistry 与 Rust PluginHost 都消费这组 canonical manifest。PluginHost 不维护平行生命周期 descriptor；Session 运行时生命周期由 SessionStore 负责。",
    "内建插件元数据位于 `src/plugin-manifests/*.json`，TypeScript `PluginRegistry` 与 Rust `PluginRuntime` 都消费这组 canonical manifest。前端 registry 只叠加 React/UI contribution，重复插件 ID 直接失败；后端 runtime 将 manifest、可选 `ProtocolAdapter` 与类型化 contribution 收敛为单一注册记录，并在启动时校验 Adapter 自声明 ID 与 manifest ID 一致。Session 运行时生命周期仍只由 `SessionStore` 负责。",
    "CORE single source",
)
write(doc_path, doc)

print("plugin runtime refactor applied")
