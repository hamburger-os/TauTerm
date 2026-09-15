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
    text, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one regex match, found {count}")
    return text


# Kernel exposes only an opaque transfer protocol identifier. Concrete transfer engines and
# execution strategy belong to the transfer subsystem, not the plugin API/kernel.
path = "src-tauri/src/kernel/plugin_adapter.rs"
text = read(path)
start = text.index("/// 文件传输协议如何取得 I/O 所有权。")
end = text.index("impl std::str::FromStr for TransferProtocolType")
text = text[:start] + '''impl TransferProtocolType {
    /// 构造规范化的传输协议标识。
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().to_lowercase())
    }

    /// 返回规范化后的传输协议标识。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

''' + text[end:]
text = replace_once(
    text,
    '''
    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        Vec::new()
    }
''',
    "",
    "ProtocolAdapter transfer_protocols",
)
write(path, text)

# Plugin adapters no longer duplicate transfer metadata already present in the canonical manifest.
path = "src-tauri/src/plugins/serial/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''use crate::kernel::plugin_adapter::{
    ContentType, EndpointInfo, ProtocolAdapter, ProtocolConnection, TransferProtocolType,
};''',
    '''use crate::kernel::plugin_adapter::{ContentType, EndpointInfo, ProtocolAdapter, ProtocolConnection};''',
    "serial transfer import",
)
text = replace_once(
    text,
    '''
    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![
            TransferProtocolType::ymodem(),
            TransferProtocolType::xmodem(),
            TransferProtocolType::zmodem(),
        ]
    }
''',
    "",
    "serial transfer protocols",
)
write(path, text)

path = "src-tauri/src/plugins/ssh/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''    ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach,
    SessionChannelFactory, SessionService, TransferProtocolType,
''',
    '''    ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach,
    SessionChannelFactory, SessionService,
''',
    "ssh transfer import",
)
text = replace_once(
    text,
    '''
    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![TransferProtocolType::sftp()]
    }
''',
    "",
    "ssh transfer protocols",
)
write(path, text)

path = "src-tauri/src/plugins/telnet/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''use crate::kernel::plugin_adapter::{
    EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach, TransferProtocolType,
};''',
    '''use crate::kernel::plugin_adapter::{
    EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach,
};''',
    "telnet transfer import",
)
text = replace_once(
    text,
    '''
    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![]
    }
''',
    "",
    "telnet transfer protocols",
)
write(path, text)

# The transfer subsystem owns concrete execution strategies. Unsupported identifiers fail closed;
# no fake FTP descriptor is exposed for a backend that does not exist.
path = "src-tauri/src/transfer/orchestrator.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::kernel::plugin_adapter::{TransferExecutionMode, TransferProtocolType};",
    "use crate::kernel::plugin_adapter::TransferProtocolType;",
    "orchestrator kernel transfer import",
)
text = replace_regex_once(
    text,
    r'''\#\[async_trait\]\npub trait TransferOrchestrator: Send \+ Sync \{.*?\n\}\n\n/// 协议 descriptor 到执行策略的唯一解析入口。\npub fn create_orchestrator\(\n    protocol_type: &TransferProtocolType,\n\) -> Result<Box<dyn TransferOrchestrator>, String> \{.*?\n\}\n''',
    '''#[async_trait]
pub trait TransferOrchestrator: Send + Sync {
    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;
}

/// 根据传输子系统实际实现的 provider 创建执行编排器。
///
/// Kernel 只携带不透明协议 ID；具体协议与执行策略在这里收敛，新增 provider 时只修改
/// transfer 模块，不修改插件 API 或 Session Kernel。
pub fn create_orchestrator(
    protocol_type: &TransferProtocolType,
) -> Result<Box<dyn TransferOrchestrator>, String> {
    match protocol_type.as_str() {
        "xmodem" | "ymodem" | "zmodem" => Ok(Box::new(InlineTransferOrchestrator {
            pt: protocol_type.clone(),
        })),
        "sftp" => Ok(Box::new(AuxiliaryTransferOrchestrator {
            pt: protocol_type.clone(),
        })),
        _ => Err(format!("不支持的传输协议: '{}'", protocol_type)),
    }
}
''',
    "transfer orchestrator strategy boundary",
)
for old, label in [
    ('''    fn protocol(&self) -> &str {
        self.pt.as_str()
    }

''', "inline protocol helper"),
    ('''    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
''', "inline cancel helper"),
]:
    text = replace_once(text, old, "", label)
# The auxiliary implementation has the same now-unused protocol/cancel helpers.
text = replace_once(
    text,
    '''    fn protocol(&self) -> &str {
        self.pt.as_str()
    }

''',
    "",
    "auxiliary protocol helper",
)
text = replace_once(
    text,
    '''    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
''',
    "",
    "auxiliary cancel helper",
)
write(path, text)

# Common session IPC must be explicit about plugin identity. There is no Serial compatibility
# default in the generic command layer.
path = "src-tauri/src/commands.rs"
text = read(path)
count = text.count("    pub plugin_id: Option<String>,")
if count != 2:
    raise RuntimeError(f"commands plugin_id request fields: expected 2, found {count}")
text = text.replace("    pub plugin_id: Option<String>,", "    pub plugin_id: String,")
text = replace_once(
    text,
    '''pub async fn enumerate_endpoints(
    state: State<'_, AppState>,
    plugin_id: Option<String>,
) -> Result<Vec<EndpointItem>, String> {
    let raw_plugin_id = plugin_id.unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID.into());
    let plugin_id =
        PluginId::parse(raw_plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;''',
    '''pub async fn enumerate_endpoints(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<EndpointItem>, String> {
    let plugin_id =
        PluginId::parse(plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;''',
    "explicit endpoint plugin id",
)
text = replace_once(
    text,
    '''    let raw_plugin_id = request
        .plugin_id
        .clone()
        .unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID.into());
    let plugin_id =
        PluginId::parse(raw_plugin_id).map_err(|error| format!("无效插件 ID: {error}"))?;''',
    '''    let plugin_id = PluginId::parse(request.plugin_id.clone())
        .map_err(|error| format!("无效插件 ID: {error}"))?;''',
    "explicit connect plugin id",
)
text = replace_once(
    text,
    '''    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state
        .plugin::<crate::plugins::serial::SerialAdapter>(crate::plugins::serial::PLUGIN_ID)
        .content_type();
    let transfer_protocols = state
        .plugin::<crate::plugins::serial::SerialAdapter>(crate::plugins::serial::PLUGIN_ID)
        .transfer_protocols();
    log::info!(
        "串口连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
    );

''',
    "",
    "serial duplicate transfer metadata log",
)
text = replace_once(
    text,
    '''    // 查询插件能力（trait 方法调度，验证 ProtocolAdapter 全路径可用）
    let content_type = state
        .plugin::<crate::plugins::telnet::TelnetAdapter>(crate::plugins::telnet::PLUGIN_ID)
        .content_type();
    let transfer_protocols = state
        .plugin::<crate::plugins::telnet::TelnetAdapter>(crate::plugins::telnet::PLUGIN_ID)
        .transfer_protocols();
    log::info!(
        "Telnet 连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols
    );

''',
    "",
    "telnet duplicate transfer metadata log",
)
text = replace_once(
    text,
    '''    let content_type = ssh_adapter.content_type();
    let transfer_protocols_list = ssh_adapter.transfer_protocols();
    log::info!(
        "SSH 连接: content_type={:?}, transfer_protocols={:?}",
        content_type,
        transfer_protocols_list
    );

''',
    "",
    "ssh duplicate transfer metadata log",
)
text = replace_once(
    text,
    '''    let pid = plugin_id.unwrap_or_else(|| "serial".into());''',
    '''    let pid = plugin_id;''',
    "saved session serial fallback",
)
write(path, text)

# Common frontend session API likewise requires plugin identity and never injects a Serial transfer
# protocol. Plugin-specific defaults belong to plugin registration/configuration.
path = "src/context/SessionContext.tsx"
text = read(path)
text = replace_once(text, "  pluginId?: string;", "  pluginId: string;", "ConnectOptions pluginId")
text = replace_once(
    text,
    '''    const effectivePluginId = pluginId || "serial";''',
    '''    const effectivePluginId = pluginId;''',
    "frontend serial fallback",
)
text = text.replace("  /** 文件传输协议（ymodem / xmodem / zmodem） */", "  /** 文件传输协议 ID */")
count = text.count('transferProtocol: transferProtocol || "ymodem",')
if count != 4:
    raise RuntimeError(f"frontend ymodem defaults: expected 4, found {count}")
text = text.replace(
    'transferProtocol: transferProtocol || "ymodem",',
    "transferProtocol: transferProtocol ?? null,",
)
write(path, text)

# Documentation must describe what exists today, not a removed PluginHost or hypothetical dynamic ABI.
path = "src-tauri/src/plugins/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''//! 每个插件实现 `ProtocolAdapter` trait并注册到 Plugin Host。
//! 未来可支持动态加载第三方插件。''',
    '''//! 内建插件在应用 composition root 显式注册到 `PluginRuntime`。通用会话插件实现
//! `ProtocolAdapter`，专属能力通过类型化 contribution 注册；本模块不维护第二套插件目录。''',
    "plugins module docs",
)
write(path, text)

# Strengthen architecture contracts around the two review findings.
path = "src-tauri/src/architecture_contract.rs"
text = read(path)
insert = '''

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
        !commands.contains("plugin_id.unwrap_or_else(|| \"serial\""),
        "saved-session IPC must require plugin_id instead of defaulting to Serial"
    );

    let frontend = read_workspace_source("src/context/SessionContext.tsx");
    assert!(
        !frontend.contains("pluginId || \"serial\""),
        "frontend generic session API must require pluginId"
    );
    assert!(
        !frontend.contains("transferProtocol || \"ymodem\""),
        "frontend generic session API must not inject a Serial transfer protocol"
    );
}
'''
# Extend the helper set once, before the first existing test.
if "fn read_workspace_source" not in text:
    marker = "\n#[test]\nfn kernel_does_not_depend_on_concrete_plugins()"
    helper = '''
fn read_workspace_source(relative: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().expect("src-tauri must have workspace parent");
    std::fs::read_to_string(workspace_root.join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}
'''
    text = replace_once(text, marker, helper + marker, "workspace architecture helper")
text += insert
write(path, text)

# CORE documents the explicit identity contract and transfer-boundary ownership.
path = "docs/modules/CORE.md"
text = read(path)
needle = "- 协议连接入口由 `PluginRuntime` 中注册的类型化 Session connector contribution 分发；公共 `connect_session` 不按插件 ID `match`。新增内建插件只在 composition root 注册 manifest、Adapter/能力和 connector。"
replacement = needle + " `connect_session`、端点枚举和保存配置都要求显式 `plugin_id`，公共层不提供 Serial 等具体插件的兼容默认值。"
text = replace_once(text, needle, replacement, "CORE explicit plugin identity")
needle = "- PTY resize、targeted send、多 peer、SFTP 等属于独立 capability，不塞进万能 stream trait。"
replacement = needle + " 文件传输 provider 的具体执行策略只存在于 `transfer/`；Kernel 仅传递不透明传输协议 ID，不维护 X/Y/ZModem、SFTP 等 provider 名称或执行模式。"
text = replace_once(text, needle, replacement, "CORE transfer provider boundary")
write(path, text)

# Migration-time static guardrails: these are intentionally strict because no compatibility layer
# is retained.
kernel_adapter = read("src-tauri/src/kernel/plugin_adapter.rs")
for forbidden in ["xmodem", "ymodem", "zmodem", "sftp", "ftp", "TransferExecutionMode"]:
    if forbidden in kernel_adapter:
        raise RuntimeError(f"kernel transfer provider leak remains: {forbidden}")
commands = read("src-tauri/src/commands.rs")
for forbidden in [
    "unwrap_or_else(|| crate::plugins::serial::PLUGIN_ID",
    'plugin_id.unwrap_or_else(|| "serial"',
    ".transfer_protocols()",
]:
    if forbidden in commands:
        raise RuntimeError(f"generic command compatibility/provider leak remains: {forbidden}")
frontend = read("src/context/SessionContext.tsx")
for forbidden in ['pluginId || "serial"', 'transferProtocol || "ymodem"']:
    if forbidden in frontend:
        raise RuntimeError(f"frontend generic fallback remains: {forbidden}")

# Print IPC call sites into the validation log so the final review can verify every frontend call
# supplies plugin identity explicitly.
for source in (ROOT / "src").rglob("*.ts*"):
    content = source.read_text(encoding="utf-8")
    if "connect_session" in content or "enumerate_endpoints" in content or "save_session_config" in content:
        print(f"IPC_CALLSITE {source.relative_to(ROOT)}")

print("final plugin architecture review cleanup applied")
