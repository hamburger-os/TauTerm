from pathlib import Path
import shutil


def replace_if_present(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old in text:
        p.write_text(text.replace(old, new, 1))

# Keep NetworkCore shared by the side-channel and aggregate driver.
replace_if_present(
    "src-tauri/src/plugins/network/mod.rs",
    """        let runtime = DataPlaneRuntime::spawn(Box::new(NetworkMuxDriver::new(\n            aggregate_rx,\n            core,\n            side.running.clone(),\n        )));\n""",
    """        let runtime = DataPlaneRuntime::spawn(Box::new(NetworkMuxDriver::new(\n            aggregate_rx,\n            core.clone(),\n            side.running.clone(),\n        )));\n""",
)

# Canonical lifecycle contract: release ExclusiveIo and restore Session state before terminal event.
replace_if_present(
    "scripts/test-file-transfer-lifecycle.mjs",
    '''assert.match(\n  orchestrator,\n  /return_port[\\s\\S]*emit_transfer_finished/,\n  "Inline resources must be returned before terminal event is emitted",\n);\n''',
    '''assert.match(\n  orchestrator,\n  /drop\\(transfer\\);[\\s\\S]*restore_session_state\\(&task_app, &task_sid, &task_transfer_id\\);[\\s\\S]*emit_transfer_finished/,\n  "Inline ExclusiveIo must be released and Session state restored before terminal event is emitted",\n);\nfor (const legacyToken of ["HandoffPort", "channel_return_tx", "try_handoff", "return_port("]) {\n  if (orchestrator.includes(legacyToken)) {\n    throw new Error(`legacy inline-transfer handoff token must be removed: ${legacyToken}`);\n  }\n}\n''',
)
contract = Path("scripts/test-file-transfer-lifecycle.mjs").read_text()
if "/return_port[\\s\\S]*emit_transfer_finished/" in contract:
    raise SystemExit("legacy transfer contract still present")

# Remove stale architectural language from comments so the source documents the current runtime.
comment_replacements = {
    "src-tauri/src/commands.rs": [
        ("// 数据推送至脚本引擎由 CommHandle::notify_receive() 统一扇出", "// 数据推送至脚本引擎由 SessionDataPlane subscription 统一扇出"),
        ("/// `CommHandle::send_text` 按会话编码转码后写设备；false 时（HEX 发送 /", "/// `SessionIo::send_text` 按会话编码转码后写设备；false 时（HEX 发送 /"),
        ("/// 脚本原始字节路径）原样透传。转码策略只存在于 CommHandle（单一知识源），", "/// 脚本原始字节路径）原样透传。转码策略只存在于 SessionIo（单一知识源），"),
        ("// 对端（网络调试）拥有各自 CommHandle，文本路径按对端编码转码。", "// 对端（网络调试）拥有各自 SessionIo，文本路径按对端编码转码。"),
        ("// 文本路径：UTF-8 → 会话编码转码（与 write_data 的 CommHandle::send_text 一致）；", "// 文本路径：UTF-8 → 会话编码转码（与 write_data 的 SessionIo::send_text 一致）；"),
        ("/// 前端终端 resize 时调用，通过 IoLoopCmd::ResizePty 转发到 I/O 循环线程，", "/// 前端终端 resize 时调用，通过 SessionIo 的 terminal-control capability 转发到 DataPlane，"),
    ],
    "src-tauri/src/plugins/ssh/mod.rs": [
        ("/// - `comm_handle`: None（由 SessionStore 统一使用默认 CommHandle 包装 write_tx）", "/// - 会话 I/O：由 SessionStore 统一绑定返回的 DataPlaneRuntime 与 SessionIo"),
    ],
    "src-tauri/src/kernel/plugin_adapter.rs": [
        ("//! 不再区分同步/异步 Channel，也不再创建第二套 CommHandle。", "//! 协议统一返回 DataPlaneRuntime，不再区分同步/异步上层 I/O 模型。"),
    ],
    "src-tauri/src/kernel/mod.rs": [
        ("//! - `plugin_adapter`  — ProtocolAdapter trait + ContentType/IoStrategy 定义", "//! - `plugin_adapter`  — ProtocolAdapter trait + protocol/session capability definitions"),
    ],
    "src-tauri/src/kernel/session_store.rs": [
        ("/// 统计采集、CommHandle（自动应答/脚本按对端生效）与日志路由。", "/// 统计采集、SessionIo（自动应答/脚本按对端生效）与日志路由。"),
        ("/// 先尝试匹配会话；否则搜索对端（网络调试）。对端拥有各自的 CommHandle，", "/// 先尝试匹配会话；否则搜索对端（网络调试）。对端拥有各自的 SessionIo，"),
    ],
}
for path, replacements in comment_replacements.items():
    for old, new in replacements:
        replace_if_present(path, old, new)

root = Path("src-tauri/src")
excluded = {
    Path("src-tauri/src/kernel/comm_handle.rs"),
    Path("src-tauri/src/plugins/network/comm.rs"),
    Path("src-tauri/src/plugins/network/tcp_channel.rs"),
}
needles = [
    "crate::channel",
    "kernel::comm_handle",
    "comm_handle::",
    "CommHandle",
    "IoLoopCmd",
    "HandoffPort",
    "StubChannel",
    "try_handoff",
    "channel_return",
    "spawn_sync_io_loop",
    "spawn_async_io_loop",
    "IoStrategy",
    "ChannelKind",
]
violations = []
for path in root.rglob("*.rs"):
    if "src-tauri/src/channel" in path.as_posix() or path in excluded:
        continue
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        if any(token in line for token in needles):
            violations.append(f"{path}:{lineno}: {line.strip()}")
if violations:
    raise SystemExit("legacy I/O references remain outside removable modules:\n" + "\n".join(violations))

replace_if_present("src-tauri/src/lib.rs", "mod channel;\n", "")
replace_if_present("src-tauri/src/kernel/mod.rs", "pub mod comm_handle;\n", "")
if Path("src-tauri/src/channel").exists():
    shutil.rmtree("src-tauri/src/channel")
for path in excluded:
    if path.exists():
        path.unlink()
