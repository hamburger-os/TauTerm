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


def regex_once(text: str, pattern: str, replacement: str, label: str) -> str:
    text, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one regex match, found {count}")
    return text


# ---------------------------------------------------------------------------
# Generic non-owning per-plugin Session runtime index.
# ---------------------------------------------------------------------------
path = "src-tauri/src/kernel/plugin_runtime.rs"
text = read(path)
text = replace_once(
    text,
    "use std::sync::Arc;",
    "use std::sync::{Arc, Mutex, Weak};",
    "plugin runtime sync imports",
)
marker = "\nimpl Default for PluginRuntime {"
helper = r'''

/// Adapter-owned, non-owning Session runtime index.
///
/// SessionStore capability graph remains the strong lifecycle owner. A plugin Adapter keeps one
/// registry instance and clones the lightweight handle into SessionAttach hooks; entries are Weak
/// so lookup never extends a Session runtime lifetime or creates a second ownership graph.
pub struct SessionRuntimeRegistry<T> {
    entries: Arc<Mutex<HashMap<String, Weak<T>>>>,
}

impl<T> Clone for SessionRuntimeRegistry<T> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
        }
    }
}

impl<T> Default for SessionRuntimeRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SessionRuntimeRegistry<T> {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, session_id: &str) -> Option<Arc<T>> {
        let mut entries = self.entries.lock().ok()?;
        let runtime = entries.get(session_id).and_then(Weak::upgrade);
        if runtime.is_none() {
            entries.remove(session_id);
        }
        runtime
    }

    pub fn attach(&self, session_id: &str, runtime: &Arc<T>) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(session_id.to_string(), Arc::downgrade(runtime));
        }
    }

    pub fn detach(&self, session_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(session_id);
        }
    }
}
'''
if marker not in text:
    raise RuntimeError("SessionRuntimeRegistry insertion marker missing")
text = text.replace(marker, helper + marker, 1)
test_marker = "\n    #[test]\n    fn adapter_identity_must_match_manifest_identity()"
test = r'''

    #[test]
    fn session_runtime_registry_is_non_owning_and_cleans_stale_entries() {
        let registry = SessionRuntimeRegistry::<String>::new();
        let runtime = Arc::new("runtime".to_string());
        registry.attach("session", &runtime);
        assert_eq!(Arc::strong_count(&runtime), 1);
        assert_eq!(registry.get("session").as_deref().map(String::as_str), Some("runtime"));
        drop(runtime);
        assert!(registry.get("session").is_none());
    }
'''
if test_marker not in text:
    raise RuntimeError("SessionRuntimeRegistry test marker missing")
text = text.replace(test_marker, test + test_marker, 1)
write(path, text)


# ---------------------------------------------------------------------------
# SSH: verifier + runtime index are both owned by the registered SshAdapter.
# ---------------------------------------------------------------------------
path = "src-tauri/src/plugins/ssh/mod.rs"
text = read(path)
text = replace_once(text, "use std::sync::{Arc, Weak};", "use std::sync::Arc;", "ssh weak import")
text = replace_once(
    text,
    "use crate::session::SessionError;",
    "use crate::kernel::plugin_runtime::SessionRuntimeRegistry;\nuse crate::session::SessionError;",
    "ssh registry import",
)
text = regex_once(
    text,
    r'''fn runtime_registry\(\n\) -> &'static std::sync::Mutex<std::collections::HashMap<String, Weak<SshRuntime>>> \{.*?\n\}\n\npub fn runtime\(session_id: &str\) -> Option<Arc<SshRuntime>> \{.*?\n\}\n\nstruct RuntimeAttach \{\n    runtime: Arc<SshRuntime>,\n\}\nimpl SessionAttach for RuntimeAttach \{\n    fn on_attached\(&self, session_id: &str\) \{.*?\n    \}\n    fn on_detached\(&self, session_id: &str\) \{\n        // SSH-owned background operations are keyed by the final Session id, so their\n        // lifecycle cleanup belongs in the SSH attachment hook rather than SessionStore\.\n        journald::stop_journald_stream\(session_id\);\n        journald::stop_journald_export\(session_id\);\n.*?\n    \}\n\}\n''',
    '''struct RuntimeAttach {
    runtime: Arc<SshRuntime>,
    runtimes: SessionRuntimeRegistry<SshRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        // SSH-owned background operations are keyed by the final Session id, so their
        // lifecycle cleanup belongs in the SSH attachment hook rather than SessionStore.
        journald::stop_journald_stream(session_id);
        journald::stop_journald_export(session_id);
        self.runtimes.detach(session_id);
    }
}
''',
    "ssh static runtime registry",
)
text = replace_once(
    text,
    '''pub struct SshAdapter {
    host_key_verifier: HostKeyVerifier,
}''',
    '''pub struct SshAdapter {
    host_key_verifier: HostKeyVerifier,
    runtimes: SessionRuntimeRegistry<SshRuntime>,
}''',
    "ssh adapter state",
)
text = replace_once(
    text,
    '''        Self {
            host_key_verifier: HostKeyVerifier::new(),
        }''',
    '''        Self {
            host_key_verifier: HostKeyVerifier::new(),
            runtimes: SessionRuntimeRegistry::new(),
        }''',
    "ssh adapter constructor",
)
text = replace_once(
    text,
    '''    pub fn runtime(&self, session_id: &str) -> Option<Arc<SshRuntime>> {
        runtime(session_id)
    }''',
    '''    pub fn runtime(&self, session_id: &str) -> Option<Arc<SshRuntime>> {
        self.runtimes.get(session_id)
    }''',
    "ssh adapter runtime lookup",
)
text = replace_once(
    text,
    '''            on_attached: Some(Arc::new(RuntimeAttach { runtime: shared })),''',
    '''            on_attached: Some(Arc::new(RuntimeAttach {
                runtime: shared,
                runtimes: self.runtimes.clone(),
            })),''',
    "ssh runtime attachment",
)
text = text.replace(
    "/// 无状态结构体——每次 `connect()` 调用建立全新的 TCP 连接和 SSH 会话。",
    "/// Adapter 持有 host-key verifier 与非持有型 Session runtime 索引；每次 `connect()` 建立全新的 TCP/SSH 会话。",
    1,
)
write(path, text)


# ---------------------------------------------------------------------------
# Network: Adapter owns the runtime index; SessionService remains strong owner.
# ---------------------------------------------------------------------------
path = "src-tauri/src/plugins/network/mod.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::kernel::session_store::PeerChannelRegistration;",
    "use crate::kernel::plugin_runtime::SessionRuntimeRegistry;\nuse crate::kernel::session_store::PeerChannelRegistration;",
    "network registry import",
)
text = regex_once(
    text,
    r'''fn runtime_registry\(\n\) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<NetworkRuntime>>> \{.*?\n\}\n\npub fn runtime\(session_id: &str\) -> Option<Arc<NetworkRuntime>> \{.*?\n\}\n\nstruct RuntimeAttach \{\n    runtime: Arc<NetworkRuntime>,\n\}\nimpl SessionAttach for RuntimeAttach \{.*?\n\}\n''',
    '''struct RuntimeAttach {
    runtime: Arc<NetworkRuntime>,
    runtimes: SessionRuntimeRegistry<NetworkRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}
''',
    "network static runtime registry",
)
text = replace_once(
    text,
    '''pub struct NetworkAdapter;

impl NetworkAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<NetworkRuntime>> {
        runtime(session_id)
    }
}''',
    '''pub struct NetworkAdapter {
    runtimes: SessionRuntimeRegistry<NetworkRuntime>,
}

impl NetworkAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<NetworkRuntime>> {
        self.runtimes.get(session_id)
    }
}''',
    "network adapter state",
)
text = replace_once(
    text,
    '''            on_attached: Some(Arc::new(RuntimeAttach { runtime: side })),''',
    '''            on_attached: Some(Arc::new(RuntimeAttach {
                runtime: side,
                runtimes: self.runtimes.clone(),
            })),''',
    "network runtime attachment",
)
write(path, text)


# ---------------------------------------------------------------------------
# TFTP: Adapter owns weak runtime index.
# ---------------------------------------------------------------------------
path = "src-tauri/src/plugins/tftp/mod.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::session::SessionError;",
    "use crate::kernel::plugin_runtime::SessionRuntimeRegistry;\nuse crate::session::SessionError;",
    "tftp registry import",
)
text = regex_once(
    text,
    r'''fn runtime_registry\(\n\) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<TftpRuntime>>> \{.*?\n\}\n\npub fn runtime\(session_id: &str\) -> Option<Arc<TftpRuntime>> \{.*?\n\}\n\nstruct RuntimeAttach \{\n    runtime: Arc<TftpRuntime>,\n\}\nimpl SessionAttach for RuntimeAttach \{.*?\n\}\n''',
    '''struct RuntimeAttach {
    runtime: Arc<TftpRuntime>,
    runtimes: SessionRuntimeRegistry<TftpRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}
''',
    "tftp static runtime registry",
)
text = replace_once(
    text,
    '''pub struct TftpAdapter;

impl TftpAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<TftpRuntime>> {
        runtime(session_id)
    }
}''',
    '''pub struct TftpAdapter {
    runtimes: SessionRuntimeRegistry<TftpRuntime>,
}

impl TftpAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<TftpRuntime>> {
        self.runtimes.get(session_id)
    }
}''',
    "tftp adapter state",
)
text = replace_once(
    text,
    '''            on_attached: Some(Arc::new(RuntimeAttach { runtime })),''',
    '''            on_attached: Some(Arc::new(RuntimeAttach {
                runtime,
                runtimes: self.runtimes.clone(),
            })),''',
    "tftp runtime attachment",
)
write(path, text)


# ---------------------------------------------------------------------------
# iperf: Adapter owns weak runtime index.
# ---------------------------------------------------------------------------
path = "src-tauri/src/plugins/iperf/mod.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::session::SessionError;",
    "use crate::kernel::plugin_runtime::SessionRuntimeRegistry;\nuse crate::session::SessionError;",
    "iperf registry import",
)
text = regex_once(
    text,
    r'''fn runtime_registry\(\n\) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<IperfRuntime>>> \{.*?\n\}\n\npub fn runtime\(session_id: &str\) -> Option<Arc<IperfRuntime>> \{.*?\n\}\n\nstruct RuntimeAttach \{\n    runtime: Arc<IperfRuntime>,\n\}\nimpl SessionAttach for RuntimeAttach \{.*?\n\}\n''',
    '''struct RuntimeAttach {
    runtime: Arc<IperfRuntime>,
    runtimes: SessionRuntimeRegistry<IperfRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}
''',
    "iperf static runtime registry",
)
text = replace_once(
    text,
    '''/// 无状态结构体——每次 `connect()` 创建侧通道。''',
    '''/// Adapter 持有非持有型 Session runtime 索引；每次 `connect()` 创建独立侧通道。''',
    "iperf adapter docs",
)
text = replace_once(
    text,
    '''pub struct IperfAdapter;

impl IperfAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<IperfRuntime>> {
        runtime(session_id)
    }
}''',
    '''pub struct IperfAdapter {
    runtimes: SessionRuntimeRegistry<IperfRuntime>,
}

impl IperfAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<IperfRuntime>> {
        self.runtimes.get(session_id)
    }
}''',
    "iperf adapter state",
)
text = replace_once(
    text,
    '''            on_attached: Some(Arc::new(RuntimeAttach { runtime })),''',
    '''            on_attached: Some(Arc::new(RuntimeAttach {
                runtime,
                runtimes: self.runtimes.clone(),
            })),''',
    "iperf runtime attachment",
)
write(path, text)


# ---------------------------------------------------------------------------
# Modbus: Adapter owns weak runtime index; commands resolve through PluginRuntime contribution.
# ---------------------------------------------------------------------------
path = "src-tauri/src/plugins/modbus/mod.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::kernel::session_store::{ContainerSessionCreateOptions, SessionStore};",
    "use crate::kernel::plugin_runtime::SessionRuntimeRegistry;\nuse crate::kernel::session_store::{ContainerSessionCreateOptions, SessionStore};",
    "modbus registry import",
)
text = regex_once(
    text,
    r'''fn runtime_registry\(\n\) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<ModbusRuntime>>> \{.*?\n\}\n\npub fn runtime\(session_id: &str\) -> Option<Arc<ModbusRuntime>> \{.*?\n\}\n\nstruct RuntimeAttach \{\n    runtime: Arc<ModbusRuntime>,\n\}\n\nimpl SessionAttach for RuntimeAttach \{.*?\n\}\n''',
    '''struct RuntimeAttach {
    runtime: Arc<ModbusRuntime>,
    runtimes: SessionRuntimeRegistry<ModbusRuntime>,
}

impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }

    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}
''',
    "modbus static runtime registry",
)
text = replace_once(
    text,
    '''pub struct ModbusAdapter;

impl ModbusAdapter {
    pub fn new() -> Self {
        Self
    }
}''',
    '''pub struct ModbusAdapter {
    runtimes: SessionRuntimeRegistry<ModbusRuntime>,
}

impl ModbusAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<ModbusRuntime>> {
        self.runtimes.get(session_id)
    }
}''',
    "modbus adapter state",
)
text = replace_once(
    text,
    '''            on_attached: Some(Arc::new(RuntimeAttach { runtime })),''',
    '''            on_attached: Some(Arc::new(RuntimeAttach {
                runtime,
                runtimes: self.runtimes.clone(),
            })),''',
    "modbus runtime attachment",
)
text = replace_once(
    text,
    '''fn with_modbus<T>(
    _state: &State<'_, AppState>,
    session_id: &str,
    function: impl FnOnce(&ModbusRuntime) -> Result<T, String>,
) -> Result<T, String> {
    let modbus = runtime(session_id).ok_or_else(|| format!("Modbus 会话 {session_id} 未连接"))?;
    function(&modbus)
}''',
    '''fn with_modbus<T>(
    state: &State<'_, AppState>,
    session_id: &str,
    function: impl FnOnce(&ModbusRuntime) -> Result<T, String>,
) -> Result<T, String> {
    let modbus = state
        .plugin::<ModbusAdapter>(PLUGIN_ID)
        .runtime(session_id)
        .ok_or_else(|| format!("Modbus 会话 {session_id} 未连接"))?;
    function(&modbus)
}''',
    "modbus runtime command lookup",
)
write(path, text)


# ---------------------------------------------------------------------------
# Architecture regression: no plugin may recreate a process-global runtime registry.
# ---------------------------------------------------------------------------
path = "src-tauri/src/architecture_contract.rs"
text = read(path)
marker = "\n#[test]\nfn kernel_transfer_protocol_id_is_provider_agnostic()"
test = r'''

#[test]
fn plugin_session_runtime_indices_are_adapter_owned() {
    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugins");
    for entry in std::fs::read_dir(&plugins_dir).expect("plugins directory") {
        let entry = entry.expect("plugin entry");
        let module = entry.path().join("mod.rs");
        if !module.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&module).expect("plugin module source");
        assert!(
            !source.contains("fn runtime_registry("),
            "plugin module {} must keep Session runtime indices on its Adapter instance, not in process-global static state",
            module.display()
        );
    }
}
'''
if marker not in text:
    raise RuntimeError("architecture test insertion marker missing")
text = text.replace(marker, test + marker, 1)
write(path, text)


# ---------------------------------------------------------------------------
# Maintainer docs: describe the actual ownership model.
# ---------------------------------------------------------------------------
path = "docs/modules/CORE.md"
text = read(path)
needle = "- 插件专属运行态必须由插件对象或 Session capability 持有，不把 SSH known-host verifier、协议 runtime registry 等字段泄漏到 `AppState`。"
replacement = needle + " Adapter 需要按 `session_id` 查找专属 runtime 时使用 `SessionRuntimeRegistry<T>`：索引实例由 Adapter 持有且只保存 `Weak<T>`，SessionStore capability graph 仍是唯一强生命周期 owner；禁止模块级 `OnceLock`/静态 runtime registry。"
text = replace_once(text, needle, replacement, "CORE runtime ownership docs")
write(path, text)

path = "docs/modules/SSH.md"
text = read(path)
old = "`SshRuntime` 的强引用只由 SessionStore 持有的 service / file-transfer / channel-factory capability graph 管理。SSH 插件为了按 `session_id` 提供类型化运行时查找，只维护 `Weak<SshRuntime>` 索引；该索引不拥有连接、不能延长连接生命周期，失效 weak entry 会被视为运行时不可用并清理。这样 SessionStore 仍是运行时资源生命周期的单一强 ownership source。"
new = "`SshRuntime` 的强引用只由 SessionStore 持有的 service / file-transfer / channel-factory capability graph 管理。注册到 `PluginRuntime` 的 `SshAdapter` 自己持有 `SessionRuntimeRegistry<SshRuntime>` 弱索引；该索引不拥有连接、不能延长连接生命周期，失效 weak entry 会被视为运行时不可用并清理。SSH 不使用模块级静态 runtime registry，因此插件实例与其私有运行态索引具有明确 ownership，SessionStore 仍是运行时资源生命周期的单一强 ownership source。"
text = replace_once(text, old, new, "SSH runtime ownership docs")
write(path, text)

path = "docs/modules/NETWORK.md"
text = read(path)
needle = "TCP connect/listen 使用 `transport::tcp`，UDP bind/recv/send 使用 `transport::udp`。Network 协议层不再维护自己的通用 TCP channel 或第二套 I/O loop。"
replacement = needle + " Network、TFTP、iperf 各自注册到 `PluginRuntime` 的 Adapter 持有自己的 `SessionRuntimeRegistry<T>` 弱索引；SessionStore capability graph 才持有 runtime 强引用，模块级静态 registry 不参与会话生命周期。"
text = replace_once(text, needle, replacement, "NETWORK runtime ownership docs")
write(path, text)

path = "docs/modules/MODBUS.md"
text = read(path)
needle = "Session JSON 中的 `ModbusConfig` 只是边界 DTO。连接开始后立即通过 `validated()` 转成 tagged runtime domain：endpoint 明确为 Serial 或 TCP，role 明确为 Client 或 Server。Client timeout/retry 与 Server max-clients/fault 分别只存在于对应角色语义中。"
replacement = needle + " `ModbusAdapter` 作为 `PluginRuntime` 中唯一注册的插件实例持有 `SessionRuntimeRegistry<ModbusRuntime>` 弱索引，命令层通过该 Adapter 查找已连接 runtime；强生命周期仍由 SessionStore 的 SessionService capability 管理，不使用模块级静态 runtime registry。"
text = replace_once(text, needle, replacement, "MODBUS runtime ownership docs")
write(path, text)
