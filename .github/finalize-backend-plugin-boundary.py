from pathlib import Path
import re

ROOT = Path('.')
COMMANDS = ROOT / 'src-tauri/src/commands.rs'
PLUGIN_APP = ROOT / 'src-tauri/src/plugin_application.rs'
LIB = ROOT / 'src-tauri/src/lib.rs'
ARCH = ROOT / 'src-tauri/src/architecture_contract.rs'
CORE_DOC = ROOT / 'docs/modules/CORE.md'


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 match, got {count}')
    return text.replace(old, new, 1)


def include_prefix(text: str, start: int) -> int:
    cur = text.rfind('\n', 0, start) + 1
    while cur > 0:
        prev_end = cur - 1
        prev_start = text.rfind('\n', 0, prev_end) + 1
        line = text[prev_start:prev_end].strip()
        if line == '' or line.startswith('///') or line.startswith('#['):
            cur = prev_start
            continue
        break
    return cur


def item_span(text: str, pattern: str, label: str):
    match = re.search(pattern, text, flags=re.M)
    if not match:
        raise SystemExit(f'{label}: item not found')
    start = include_prefix(text, match.start())
    brace = text.find('{', match.end())
    if brace < 0:
        raise SystemExit(f'{label}: opening brace not found')
    depth = 0
    end = None
    for index in range(brace, len(text)):
        char = text[index]
        if char == '{':
            depth += 1
        elif char == '}':
            depth -= 1
            if depth == 0:
                end = index + 1
                break
    if end is None:
        raise SystemExit(f'{label}: closing brace not found')
    while end < len(text) and text[end] == '\n':
        end += 1
    return start, end


def extract_item(text: str, pattern: str, label: str):
    start, end = item_span(text, pattern, label)
    return text[start:end], text[:start] + text[end:]


def extract_between(text: str, start_marker: str, end_marker: str, label: str):
    start = text.find(start_marker)
    if start < 0:
        raise SystemExit(f'{label}: start marker not found')
    end = text.find(end_marker, start + len(start_marker))
    if end < 0:
        raise SystemExit(f'{label}: end marker not found')
    return text[start:end], text[:start] + text[end:]


def add_commands_module(path: str):
    p = ROOT / path
    text = p.read_text(encoding='utf-8')
    if 'mod commands;' in text:
        return
    marker = 'pub const PLUGIN_ID: &str = '
    pos = text.find(marker)
    if pos < 0:
        raise SystemExit(f'{path}: PLUGIN_ID marker not found')
    end = text.find('\n', pos)
    text = text[:end + 1] + '\npub(crate) mod commands;\n' + text[end + 1:]
    p.write_text(text, encoding='utf-8')


commands = COMMANDS.read_text(encoding='utf-8')

# --- Move the generic connector contract out of commands.rs. ---
connect_request, commands = extract_item(
    commands,
    r'^pub struct ConnectSessionRequest\s*\{',
    'ConnectSessionRequest',
)

alias_match = re.search(
    r'(?ms)^pub\(crate\) type SessionConnectFuture\s*=.*?;\n'
    r'pub\(crate\) type SessionConnectHandler\s*=.*?;\n',
    commands,
)
if not alias_match:
    raise SystemExit('Session connector aliases not found')
connector_aliases = commands[alias_match.start():alias_match.end()]
commands = commands[:alias_match.start()] + commands[alias_match.end():]

save_marker = '#[derive(Debug, Clone, Deserialize)]\n#[serde(rename_all = "camelCase")]\npub struct SaveSessionConfigRequest'
macro_start = commands.find('macro_rules! define_session_connector')
macro_end = commands.find(save_marker)
if macro_start < 0 or macro_end < 0 or macro_end <= macro_start:
    raise SystemExit('connector wrapper macro block not found')
commands = commands[:macro_start] + commands[macro_end:]

# --- Move reusable application/session orchestration helpers. ---
create_on_data, commands = extract_item(
    commands,
    r'^fn create_on_data_callback\s*\(',
    'create_on_data_callback',
)
connect_simple, commands = extract_item(
    commands,
    r'^fn connect_simple_terminal_session\s*\(',
    'connect_simple_terminal_session',
)
create_child, commands = extract_item(
    commands,
    r'^async fn create_terminal_sub_channel\s*\(',
    'create_terminal_sub_channel',
)
child_payload, commands = extract_item(
    commands,
    r'^fn terminal_sub_channel_connected_payload\s*\(',
    'terminal_sub_channel_connected_payload',
)

for original, public in [
    ('fn create_on_data_callback(', 'pub(crate) fn create_on_data_callback('),
    ('fn connect_simple_terminal_session(', 'pub(crate) fn connect_simple_terminal_session('),
    ('async fn create_terminal_sub_channel(', 'pub(crate) async fn create_terminal_sub_channel('),
    ('fn terminal_sub_channel_connected_payload(', 'pub(crate) fn terminal_sub_channel_connected_payload('),
]:
    if original not in locals().get('create_on_data', '') + locals().get('connect_simple', '') + locals().get('create_child', '') + locals().get('child_payload', ''):
        raise SystemExit(f'shared helper visibility marker missing: {original}')
    if original in create_on_data:
        create_on_data = create_on_data.replace(original, public, 1)
    elif original in connect_simple:
        connect_simple = connect_simple.replace(original, public, 1)
    elif original in create_child:
        create_child = create_child.replace(original, public, 1)
    else:
        child_payload = child_payload.replace(original, public, 1)

# Shared child-session events carry opaque params; protocol-private convenience fields are forbidden.
create_child = replace_once(
    create_child,
    '''        send_bar_enabled_val,\n        file_service_enabled,\n        journald_enabled,\n        file_service_protocol,\n''',
    '''        send_bar_enabled_val,\n''',
    'generic child tuple declaration',
)
create_child = replace_once(
    create_child,
    '''            handle.send_bar_enabled,\n            handle\n                .params\n                .get("file_service_enabled")\n                .and_then(Value::as_bool)\n                .unwrap_or(false),\n            handle\n                .params\n                .get("journald_enabled")\n                .and_then(Value::as_bool)\n                .unwrap_or(false),\n            handle\n                .params\n                .get("file_service_protocol")\n                .and_then(Value::as_str)\n                .unwrap_or("sftp")\n                .to_string(),\n''',
    '''            handle.send_bar_enabled,\n''',
    'generic child tuple values',
)
create_child = replace_once(
    create_child,
    '''                "elevated": elevated,\n                "file_service_enabled": file_service_enabled,\n                "file_service_protocol": file_service_protocol,\n                "journald_enabled": journald_enabled,\n''',
    '''                "elevated": elevated,\n''',
    'generic child event payload',
)
child_payload = replace_once(
    child_payload,
    '''        send_bar_enabled,\n        file_service_enabled,\n        file_service_protocol,\n        journald_enabled,\n        channel_name,\n''',
    '''        send_bar_enabled,\n        channel_name,\n''',
    'generic connected payload tuple declaration',
)
child_payload = replace_once(
    child_payload,
    '''            parent.send_bar_enabled,\n            parent\n                .params\n                .get("file_service_enabled")\n                .and_then(Value::as_bool)\n                .unwrap_or(false),\n            parent\n                .params\n                .get("file_service_protocol")\n                .and_then(Value::as_str)\n                .unwrap_or("sftp")\n                .to_string(),\n            parent\n                .params\n                .get("journald_enabled")\n                .and_then(Value::as_bool)\n                .unwrap_or(false),\n''',
    '''            parent.send_bar_enabled,\n''',
    'generic connected payload tuple values',
)
child_payload = replace_once(
    child_payload,
    '''        "elevated": elevated,\n        "file_service_enabled": file_service_enabled,\n        "file_service_protocol": file_service_protocol,\n        "journald_enabled": journald_enabled,\n''',
    '''        "elevated": elevated,\n''',
    'generic connected payload fields',
)

# --- Move protocol-specific connectors. ---
serial_connect, commands = extract_item(commands, r'^async fn connect_session_serial\s*\(', 'serial connector')
telnet_connect, commands = extract_item(commands, r'^async fn connect_session_telnet\s*\(', 'telnet connector')
local_connect, commands = extract_item(commands, r'^async fn connect_session_local_shell\s*\(', 'local shell connector')
ssh_connect, commands = extract_item(commands, r'^async fn connect_session_ssh\s*\(', 'ssh connector')

serial_connect = serial_connect.replace('async fn connect_session_serial(', 'async fn connect_session(', 1)
telnet_connect = telnet_connect.replace('async fn connect_session_telnet(', 'async fn connect_session(', 1)
local_connect = local_connect.replace('async fn connect_session_local_shell(', 'async fn connect_session(', 1)
ssh_connect = ssh_connect.replace('async fn connect_session_ssh(', 'async fn connect_session(', 1)

# --- Move SSH host-key, SFTP and journald IPC + DTOs. ---
host_key_block, commands = extract_between(
    commands,
    '// ── 命令：SSH 主机密钥确认',
    '// ── 命令：会话断开',
    'SSH host-key commands',
)
journald_query, commands = extract_item(commands, r'^pub struct JournaldQueryRequest\s*\{', 'JournaldQueryRequest')
journald_export, commands = extract_item(commands, r'^pub struct JournaldExportRequest\s*\{', 'JournaldExportRequest')
ssh_side_block, commands = extract_between(
    commands,
    '// ── 命令：SSH 文件服务（SFTP）',
    '// ── 统一文件传输命令',
    'SSH side-channel commands',
)

# --- Move Network, TFTP and iperf application commands. ---
network_block, commands = extract_between(
    commands,
    '// ── 网络调试会话命令',
    '// ── 会话持久化命令',
    'Network commands',
)
network_block = network_block.replace('#[tauri::command]\npub async fn connect_session_network(', 'async fn connect_session(', 1)

tftp_request, commands = extract_item(commands, r'^pub struct TftpClientRequest\s*\{', 'TftpClientRequest')
tftp_marker = '// ═══════════════════════════════════════════════════════════════\n// TFTP 协议命令'
iperf_marker = '// ═══════════════════════════════════════════════════════════════\n// iperf 协议命令（iperf2 + iperf3）'
tests_marker = '#[cfg(test)]\nmod command_security_tests'
tftp_block, commands = extract_between(commands, tftp_marker, iperf_marker, 'TFTP commands')
iperf_block, commands = extract_between(commands, iperf_marker, tests_marker, 'iperf commands')
tftp_block = tftp_block.replace('async fn connect_session_tftp(', 'async fn connect_session(', 1)
iperf_block = iperf_block.replace('async fn connect_session_iperf(', 'async fn connect_session(', 1)

# --- Make common disconnect lifecycle contribution-driven. ---
disconnect_pattern = re.compile(
    r'''    log::info!\("会话已断开: \{\} \(\{\}\)", session_name, plugin_id\);\n'''
    r'''    if plugin_id == crate::plugins::tftp::PLUGIN_ID \{.*?\n    \}\n'''
    r'''    if plugin_id == crate::plugins::iperf::PLUGIN_ID \{.*?\n    \}\n''',
    re.S,
)
replacement = '''    log::info!("会话已断开: {} ({})", session_name, plugin_id);\n    if let Ok(plugin_id) = PluginId::parse(plugin_id) {\n        if let Some(hook) = state\n            .plugins\n            .contribution::<SessionDisconnectedHook>(&plugin_id)\n        {\n            (hook.as_ref())(&app, &session_id);\n        }\n    }\n'''
commands, count = disconnect_pattern.subn(replacement, commands, count=1)
if count != 1:
    raise SystemExit(f'disconnect contribution migration expected 1 match, got {count}')

# Clean obsolete section labels and make the remaining multi-terminal command generic.
commands = commands.replace('// ── SSH 子通道创建（共享逻辑）───────────────────────\n\n', '')
commands = commands.replace('// ── SSH 多连接命令 ─────────────────────────────────', '// ── 多终端通道命令 ─────────────────────────────────')
commands = commands.replace('/// 在已有 SSH 会话上打开新的 PTY channel（不重复 TCP/握手/认证）。', '/// 在支持子终端工厂的父会话上打开新的终端 channel。')
commands = commands.replace('create_terminal_sub_channel(', 'crate::plugin_application::create_terminal_sub_channel(', 1)

# Replace imports that were only needed by protocol implementations.
commands = commands.replace('use crate::kernel::charset::transcode_utf8_to_encoding;\n', '')
commands = replace_once(
    commands,
    '''use crate::kernel::log_engine::{\n    try_send_session_log, try_send_system_event, DataDirection, DataLogEntry, LogConfigResponse,\n    LogConfigUpdate, LogEntry, LogHealth, LogStatus,\n};\n''',
    '''use crate::kernel::log_engine::{\n    try_send_system_event, LogConfigResponse, LogConfigUpdate, LogEntry, LogHealth, LogStatus,\n};\n''',
    'common log imports',
)
commands = replace_once(
    commands,
    '''use crate::kernel::plugin_adapter::{\n    ChannelOpenMode, PluginId, ProtocolAdapter, TransferProtocolType,\n};\n''',
    '''use crate::kernel::plugin_adapter::{ChannelOpenMode, PluginId, TransferProtocolType};\n''',
    'common plugin adapter imports',
)
commands = replace_once(
    commands,
    '''use crate::kernel::session_store::{\n    ContainerSessionCreateOptions, ContainerSessionRuntime, SessionCreateOptions, SessionState,\n    SessionStore,\n};\n''',
    '''use crate::kernel::session_store::{SessionState, SessionStore};\n''',
    'common session store imports',
)
commands = replace_once(
    commands,
    '''use crate::plugin_application::{SessionConfigHandler, SessionConfigServices};\n''',
    '''use crate::plugin_application::{\n    ConnectSessionRequest, SessionConfigHandler, SessionConfigServices, SessionConnectHandler,\n    SessionDisconnectedHook,\n};\n''',
    'common plugin application imports',
)
commands = commands.replace('use crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};\n', 'use crate::session::DisconnectInfo;\n')
commands = commands.replace('use crate::transport::DataPlaneRuntime;\n', '')
commands = commands.replace('use std::collections::HashMap;\n', '')
commands = commands.replace('use std::future::Future;\n', '')
commands = commands.replace('use std::pin::Pin;\n', '')
commands = commands.replace('use std::sync::atomic::{AtomicBool, Ordering};\n', '')
commands = commands.replace('use std::sync::{Arc, LazyLock, Mutex};\n', '')

COMMANDS.write_text(commands, encoding='utf-8')

# --- Expand application-layer plugin contracts with connector/session orchestration. ---
plugin_app = PLUGIN_APP.read_text(encoding='utf-8')
plugin_app = replace_once(plugin_app, 'use serde_json::Value;\nuse std::sync::Mutex;\n', '''use chrono::Local;\nuse serde::Deserialize;\nuse serde_json::Value;\nuse std::future::Future;\nuse std::pin::Pin;\nuse std::sync::atomic::{AtomicBool, Ordering};\nuse std::sync::{Arc, Mutex};\nuse tauri::{AppHandle, Emitter, Manager, State};\n''', 'plugin application std imports')
plugin_app = replace_once(
    plugin_app,
    'use crate::kernel::session_store::{SavedSession, SessionStore};\nuse crate::security::credential_store::CredentialStore;\n',
    '''use crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry, LogEntry};\nuse crate::kernel::plugin_adapter::ProtocolConnection;\nuse crate::kernel::session_store::{SavedSession, SessionCreateOptions, SessionState, SessionStore};\nuse crate::security::credential_store::CredentialStore;\nuse crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};\nuse crate::transport::DataPlaneRuntime;\nuse crate::AppState;\n''',
    'plugin application crate imports',
)
connect_request = connect_request.replace('pub struct ConnectSessionRequest', 'pub(crate) struct ConnectSessionRequest', 1)
connector_aliases = connector_aliases.replace('pub(crate) type', 'pub(crate) type')
plugin_app += '''\n// ── Session connection application contract ─────────────────────\n\n''' + connect_request + '\n' + connector_aliases + '''\npub(crate) type SessionDisconnectedHook = fn(&AppHandle, &str);\n\n''' + create_on_data + '\n' + connect_simple + '\n' + create_child + '\n' + child_payload
PLUGIN_APP.write_text(plugin_app, encoding='utf-8')

# --- Per-plugin command modules. ---
def connector_wrapper() -> str:
    return '''pub(crate) fn session_connector(\n    app: AppHandle,\n    request: ConnectSessionRequest,\n) -> SessionConnectFuture {\n    Box::pin(async move {\n        let state: State<'_, AppState> = app.state();\n        connect_session(app.clone(), state, request).await\n    })\n}\n\n'''

serial_src = '''//! Serial application-layer Session connector.\n\nuse serde_json::Value;\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::plugin_adapter::ProtocolAdapter;\nuse crate::kernel::session_store::SessionCreateOptions;\nuse crate::plugin_application::{\n    create_on_data_callback, ConnectSessionRequest, SessionConnectFuture,\n};\nuse crate::session::DisconnectInfo;\nuse crate::AppState;\n\n''' + connector_wrapper() + serial_connect
(ROOT / 'src-tauri/src/plugins/serial/commands.rs').write_text(serial_src, encoding='utf-8')

telnet_src = '''//! Telnet application-layer Session connector.\n\nuse tauri::{AppHandle, Manager, State};\n\nuse crate::kernel::plugin_adapter::ProtocolAdapter;\nuse crate::plugin_application::{\n    connect_simple_terminal_session, ConnectSessionRequest, SessionConnectFuture,\n};\nuse crate::AppState;\n\n''' + connector_wrapper() + telnet_connect
(ROOT / 'src-tauri/src/plugins/telnet/commands.rs').write_text(telnet_src, encoding='utf-8')

local_src = '''//! Local Shell application-layer Session connector.\n\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::plugin_adapter::ChannelOpenMode;\nuse crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};\nuse crate::plugin_application::{\n    create_terminal_sub_channel, ConnectSessionRequest, SessionConnectFuture,\n};\nuse crate::AppState;\n\n''' + connector_wrapper() + local_connect
(ROOT / 'src-tauri/src/plugins/local_shell/commands.rs').write_text(local_src, encoding='utf-8')

ssh_src = '''//! SSH application-layer connector and SSH-only IPC commands.\n\nuse serde::Deserialize;\nuse serde_json::Value;\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::session_store::{\n    ContainerSessionCreateOptions, ContainerSessionRuntime, SessionState,\n};\nuse crate::plugin_application::{\n    create_terminal_sub_channel, terminal_sub_channel_connected_payload, ConnectSessionRequest,\n    SessionConnectFuture,\n};\nuse crate::AppState;\n\n''' + connector_wrapper() + ssh_connect + '\n' + host_key_block + '\n' + journald_query + journald_export + '\n' + ssh_side_block
(ROOT / 'src-tauri/src/plugins/ssh/commands.rs').write_text(ssh_src, encoding='utf-8')

network_src = '''//! Network Debug application-layer connector and IPC commands.\n\nuse chrono::Local;\nuse serde_json::Value;\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::charset::transcode_utf8_to_encoding;\nuse crate::kernel::log_engine::{try_send_session_log, DataDirection, DataLogEntry};\nuse crate::kernel::plugin_adapter::ProtocolAdapter;\nuse crate::kernel::session_store::SessionCreateOptions;\nuse crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};\nuse crate::AppState;\n\n''' + connector_wrapper() + network_block
(ROOT / 'src-tauri/src/plugins/network/commands.rs').write_text(network_src, encoding='utf-8')

tftp_src = '''//! TFTP application-layer connector and IPC commands.\n\nuse serde::Deserialize;\nuse serde_json::Value;\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::plugin_adapter::ProtocolAdapter;\nuse crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};\nuse crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};\nuse crate::AppState;\n\n''' + tftp_request + '\n' + connector_wrapper() + tftp_block + '''\npub(crate) fn session_disconnected(app: &AppHandle, session_id: &str) {\n    let _ = app.emit(\n        "tftp-server-status",\n        serde_json::json!({ "session_id": session_id, "running": false }),\n    );\n}\n'''
(ROOT / 'src-tauri/src/plugins/tftp/commands.rs').write_text(tftp_src, encoding='utf-8')

iperf_src = '''//! iperf application-layer connector and IPC commands.\n\nuse std::collections::HashMap;\nuse std::sync::atomic::{AtomicBool, Ordering};\nuse std::sync::{Arc, LazyLock, Mutex};\n\nuse serde_json::Value;\nuse tauri::{AppHandle, Emitter, Manager, State};\n\nuse crate::kernel::plugin_adapter::ProtocolAdapter;\nuse crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};\nuse crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};\nuse crate::AppState;\n\n''' + connector_wrapper() + iperf_block + '''\npub(crate) fn session_disconnected(app: &AppHandle, session_id: &str) {\n    let _ = app.emit(\n        "iperf-server-status",\n        serde_json::json!({ "session_id": session_id, "running": false }),\n    );\n}\n'''
(ROOT / 'src-tauri/src/plugins/iperf/commands.rs').write_text(iperf_src, encoding='utf-8')

for module_path in [
    'src-tauri/src/plugins/serial/mod.rs',
    'src-tauri/src/plugins/telnet/mod.rs',
    'src-tauri/src/plugins/local_shell/mod.rs',
    'src-tauri/src/plugins/ssh/mod.rs',
    'src-tauri/src/plugins/network/mod.rs',
    'src-tauri/src/plugins/tftp/mod.rs',
    'src-tauri/src/plugins/iperf/mod.rs',
]:
    add_commands_module(module_path)

# --- Modbus and TRDP use the same connector contract without depending on commands.rs. ---
modbus_path = ROOT / 'src-tauri/src/plugins/modbus/mod.rs'
modbus = modbus_path.read_text(encoding='utf-8')
modbus = replace_once(modbus, 'use crate::commands::ConnectSessionRequest;\n', 'use crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};\n', 'Modbus connector import')
modbus = replace_once(modbus, 'use tauri::{AppHandle, Emitter, State};\n', 'use tauri::{AppHandle, Emitter, Manager, State};\n', 'Modbus Manager import')
modbus = modbus.replace('pub async fn connect_session(', 'async fn connect_session(', 1)
modbus_insert = '''\npub(crate) fn session_connector(\n    app: AppHandle,\n    request: ConnectSessionRequest,\n) -> SessionConnectFuture {\n    Box::pin(async move {\n        let state: State<'_, AppState> = app.state();\n        connect_session(app.clone(), state, request).await\n    })\n}\n\n'''
marker = 'fn parse_watch_rows('
pos = modbus.find(marker)
if pos < 0:
    raise SystemExit('Modbus wrapper insertion marker missing')
modbus = modbus[:pos] + modbus_insert + modbus[pos:]
modbus_path.write_text(modbus, encoding='utf-8')

trdp_path = ROOT / 'src-tauri/src/plugins/trdp.rs'
trdp = trdp_path.read_text(encoding='utf-8')
trdp = replace_once(trdp, 'use crate::commands::ConnectSessionRequest;\n', 'use crate::plugin_application::{ConnectSessionRequest, SessionConnectFuture};\n', 'TRDP connector import')
trdp = trdp.replace('pub async fn connect_session(', 'async fn connect_session(', 1)
trdp_wrapper = '''\npub(crate) fn session_connector(\n    app: AppHandle,\n    request: ConnectSessionRequest,\n) -> SessionConnectFuture {\n    Box::pin(async move {\n        let state: State<'_, AppState> = app.state();\n        connect_session(app.clone(), state, request).await\n    })\n}\n\n'''
connect_pos = trdp.find('async fn connect_session(')
if connect_pos < 0:
    raise SystemExit('TRDP connect_session marker missing')
trdp = trdp[:connect_pos] + trdp_wrapper + trdp[connect_pos:]
trdp_path.write_text(trdp, encoding='utf-8')

# --- Composition root now registers plugin-owned application connectors and lifecycle hooks. ---
lib = LIB.read_text(encoding='utf-8')
lib = lib.replace('connector: commands::SessionConnectHandler,', 'connector: plugin_application::SessionConnectHandler,')
for old, new in [
    ('commands::serial_session_connector', 'plugins::serial::commands::session_connector'),
    ('commands::ssh_session_connector', 'plugins::ssh::commands::session_connector'),
    ('commands::telnet_session_connector', 'plugins::telnet::commands::session_connector'),
    ('commands::local_shell_session_connector', 'plugins::local_shell::commands::session_connector'),
    ('commands::tftp_session_connector', 'plugins::tftp::commands::session_connector'),
    ('commands::iperf_session_connector', 'plugins::iperf::commands::session_connector'),
    ('commands::network_session_connector', 'plugins::network::commands::session_connector'),
    ('commands::modbus_session_connector', 'plugins::modbus::session_connector'),
]:
    lib = replace_once(lib, old, new, f'connector wiring {old}')
lib = replace_once(
    lib,
    '''            commands::trdp_session_connector as commands::SessionConnectHandler,\n''',
    '''            plugins::trdp::session_connector as plugin_application::SessionConnectHandler,\n''',
    'TRDP connector wiring',
)

# Register plugin-owned disconnect side effects through the same runtime directory.
hook_anchor = '''    for (plugin_id, handler) in [\n'''
hook_code = '''    for (plugin_id, hook) in [\n        (\n            plugins::tftp::PLUGIN_ID,\n            plugins::tftp::commands::session_disconnected\n                as plugin_application::SessionDisconnectedHook,\n        ),\n        (\n            plugins::iperf::PLUGIN_ID,\n            plugins::iperf::commands::session_disconnected\n                as plugin_application::SessionDisconnectedHook,\n        ),\n    ] {\n        let plugin_id =\n            kernel::plugin_adapter::PluginId::parse(plugin_id).expect("built-in plugin id");\n        runtime\n            .register_contribution(&plugin_id, hook)\n            .unwrap_or_else(|error| panic!("注册 Session 断开 contribution 失败: {error}"));\n    }\n\n'''
pos = lib.find(hook_anchor)
if pos < 0:
    raise SystemExit('lib disconnect hook insertion marker missing')
lib = lib[:pos] + hook_code + lib[pos:]

# Invoke handler paths keep stable command names while ownership moves to plugin modules.
invoke_replacements = {
    '            commands::connect_session_network,\n': '',
    'commands::list_network_peers': 'plugins::network::commands::list_network_peers',
    'commands::close_network_peer': 'plugins::network::commands::close_network_peer',
    'commands::network_udp_send_to': 'plugins::network::commands::network_udp_send_to',
    'commands::network_udp_send': 'plugins::network::commands::network_udp_send',
    'commands::set_network_send_target': 'plugins::network::commands::set_network_send_target',
    'commands::sftp_list_dir_cmd': 'plugins::ssh::commands::sftp_list_dir_cmd',
    'commands::sftp_stat_cmd': 'plugins::ssh::commands::sftp_stat_cmd',
    'commands::sftp_read_head_cmd': 'plugins::ssh::commands::sftp_read_head_cmd',
    'commands::sftp_chmod_cmd': 'plugins::ssh::commands::sftp_chmod_cmd',
    'commands::sftp_delete_cmd': 'plugins::ssh::commands::sftp_delete_cmd',
    'commands::sftp_rename_cmd': 'plugins::ssh::commands::sftp_rename_cmd',
    'commands::sftp_mkdir_cmd': 'plugins::ssh::commands::sftp_mkdir_cmd',
    'commands::sftp_new_file_cmd': 'plugins::ssh::commands::sftp_new_file_cmd',
    'commands::sftp_delete_batch_cmd': 'plugins::ssh::commands::sftp_delete_batch_cmd',
    'commands::sftp_delete_recursive_cmd': 'plugins::ssh::commands::sftp_delete_recursive_cmd',
    'commands::start_journald_stream': 'plugins::ssh::commands::start_journald_stream',
    'commands::stop_journald_stream': 'plugins::ssh::commands::stop_journald_stream',
    'commands::journald_query_cmd': 'plugins::ssh::commands::journald_query_cmd',
    'commands::start_journald_export': 'plugins::ssh::commands::start_journald_export',
    'commands::stop_journald_export': 'plugins::ssh::commands::stop_journald_export',
    'commands::get_ssh_home_dir': 'plugins::ssh::commands::get_ssh_home_dir',
    'commands::confirm_host_key': 'plugins::ssh::commands::confirm_host_key',
    'commands::tftp_server_start': 'plugins::tftp::commands::tftp_server_start',
    'commands::tftp_server_stop': 'plugins::tftp::commands::tftp_server_stop',
    'commands::tftp_client_get': 'plugins::tftp::commands::tftp_client_get',
    'commands::tftp_client_put': 'plugins::tftp::commands::tftp_client_put',
    'commands::tftp_update_params': 'plugins::tftp::commands::tftp_update_params',
    'commands::tftp_get_status': 'plugins::tftp::commands::tftp_get_status',
    'commands::iperf_server_start': 'plugins::iperf::commands::iperf_server_start',
    'commands::iperf_server_stop': 'plugins::iperf::commands::iperf_server_stop',
    'commands::iperf_client_run': 'plugins::iperf::commands::iperf_client_run',
    'commands::iperf_client_stop': 'plugins::iperf::commands::iperf_client_stop',
    'commands::iperf_update_params': 'plugins::iperf::commands::iperf_update_params',
    'commands::iperf_get_status': 'plugins::iperf::commands::iperf_get_status',
}
for old, new in invoke_replacements.items():
    if old not in lib:
        raise SystemExit(f'invoke wiring not found: {old!r}')
    lib = lib.replace(old, new)
LIB.write_text(lib, encoding='utf-8')

# --- Architecture contracts: common command layer may not own concrete protocol application logic. ---
arch = ARCH.read_text(encoding='utf-8')
old_router = '''    let start = source\n        .find("pub async fn connect_session(")\n        .expect("connect_session start");\n    let tail = &source[start..];\n    let end = tail\n        .find("/// 创建共享 on_data 回调")\n        .expect("connect_session end marker");\n    let router = &tail[..end];\n\n    assert!(router.contains("contribution::<SessionConnectHandler>"));\n    assert!(\n        !router.contains("match pid.as_str()"),\n'''
new_router = '''    let start = source\n        .find("pub async fn connect_session(")\n        .expect("connect_session start");\n    let router = &source[start..];\n\n    assert!(router.contains("contribution::<SessionConnectHandler>"));\n    assert!(\n        !router.contains("match pid.as_str()"),\n'''
arch = replace_once(arch, old_router, new_router, 'architecture connection router test')
arch += '''\n#[test]\nfn common_commands_do_not_own_protocol_application_surfaces() {\n    let source = read_source("commands.rs");\n    for forbidden in [\n        "crate::plugins::",\n        "connect_session_serial",\n        "connect_session_ssh",\n        "connect_session_telnet",\n        "connect_session_local_shell",\n        "connect_session_tftp",\n        "connect_session_iperf",\n        "connect_session_network",\n        "confirm_host_key",\n        "sftp_list_dir_cmd",\n        "journald_query_cmd",\n        "network_udp_send",\n        "tftp_server_start",\n        "iperf_server_start",\n    ] {\n        assert!(\n            !source.contains(forbidden),\n            "common commands.rs must stay protocol-agnostic: {forbidden}"\n        );\n    }\n}\n\n#[test]\nfn plugin_connectors_depend_on_application_contract_not_common_commands() {\n    for relative in ["plugins/modbus/mod.rs", "plugins/trdp.rs"] {\n        let source = read_source(relative);\n        assert!(\n            !source.contains("crate::commands::ConnectSessionRequest"),\n            "{relative} must consume plugin_application connector contracts"\n        );\n    }\n}\n\n#[test]\nfn generic_session_application_payload_has_no_protocol_private_fields() {\n    let source = read_source("plugin_application.rs");\n    for forbidden in ["file_service_enabled", "file_service_protocol", "journald_enabled"] {\n        assert!(\n            !source.contains(forbidden),\n            "generic Session application orchestration must not project SSH-private field {forbidden}"\n        );\n    }\n}\n'''
ARCH.write_text(arch, encoding='utf-8')

# --- Documentation ownership. ---
core = CORE_DOC.read_text(encoding='utf-8')
old = 'Tauri command 按阻塞风险分类：纯内存/短锁读取可以同步；文件系统、进程、凭据后端、驱动/平台探测、thread join 等潜在阻塞工作必须使用 async command，并在需要时进入 blocking worker。端点发现同样是配置辅助能力，不属于 Session 生命周期；前端进入配置页时按需请求，后端不得让硬件枚举阻塞 UI。'
new = old + ' 公共 `commands.rs` 只承载协议无关 IPC；协议连接编排、协议专属 DTO/Tauri command 与断开后的协议副作用由各插件 application/commands 模块持有，通过 `PluginRuntime` 中的 connector/lifecycle contribution 接入。共享 Session 编排辅助函数位于 application contribution 层，不反向解释 SSH、TFTP、Network 等字段。'
core = replace_once(core, old, new, 'CORE command ownership paragraph')
old_boundary = '- 协议连接入口由 `PluginRuntime` 中注册的类型化 Session connector contribution 分发；公共 `connect_session` 不按插件 ID `match`。新增内建插件只在 composition root 注册 manifest、Adapter/能力和 connector。`connect_session`、端点枚举和保存配置都要求显式 `plugin_id`，公共层不提供 Serial 等具体插件的兼容默认值。'
new_boundary = old_boundary + ' 插件专属 Tauri command、连接实现和断开通知同样不得堆回公共 `commands.rs`；composition root 只负责显式注册，不承载协议语义。'
core = replace_once(core, old_boundary, new_boundary, 'CORE connector boundary')
CORE_DOC.write_text(core, encoding='utf-8')

print('backend plugin application ownership migration applied')
