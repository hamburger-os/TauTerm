from pathlib import Path
import shutil


def replace(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing: {path}: {old[:100]!r}")
    p.write_text(text.replace(old, new, count))

# Avoid moving the shared NetworkCore out of a scope that also borrows its UDP fields.
replace(
    "src-tauri/src/plugins/network/mod.rs",
    """        let runtime = DataPlaneRuntime::spawn(Box::new(NetworkMuxDriver::new(\n            aggregate_rx,\n            core,\n            side.running.clone(),\n        )));\n""",
    """        let runtime = DataPlaneRuntime::spawn(Box::new(NetworkMuxDriver::new(\n            aggregate_rx,\n            core.clone(),\n            side.running.clone(),\n        )));\n""",
)

# Update the file-transfer lifecycle contract to the canonical ExclusiveIo RAII design.
replace(
    "scripts/test-file-transfer-lifecycle.mjs",
    """const terminalBlock = mustMatch(\n  orchestratorSource,\n  /let final_result = transfer_result\\.map_err\\(.*?worker\\.return_port\\(&task_app, &task_sid, &task_transfer_id, port\\).*?emit_transfer_finished\\(/s,\n  \"transfer terminal state must be emitted after I/O restoration\",\n);\nassertIncludes(\n  terminalBlock,\n  \"worker.return_port(\",\n  \"transfer terminal path must restore the real port before emitting terminal state\",\n);\nif (\n  terminalBlock.indexOf(\"worker.return_port(\") >\n  terminalBlock.indexOf(\"emit_transfer_finished(\")\n) {\n  throw new Error(\"transfer terminal state must not be emitted before the port is restored\");\n}\n""",
    """const terminalBlock = mustMatch(\n  orchestratorSource,\n  /let final_result = transfer_result\\.map_err\\(.*?drop\\(transfer\\);.*?restore_session_state\\(&task_app, &task_sid, &task_transfer_id\\);.*?emit_transfer_finished\\(/s,\n  \"transfer terminal state must be emitted after ExclusiveIo release and session restoration\",\n);\nassertIncludes(\n  terminalBlock,\n  \"drop(transfer);\",\n  \"transfer terminal path must release the ExclusiveIo lease before emitting terminal state\",\n);\nassertIncludes(\n  terminalBlock,\n  \"restore_session_state(&task_app, &task_sid, &task_transfer_id);\",\n  \"transfer terminal path must restore the session state before emitting terminal state\",\n);\nif (\n  terminalBlock.indexOf(\"drop(transfer);\") >\n    terminalBlock.indexOf(\"emit_transfer_finished(\") ||\n  terminalBlock.indexOf(\"restore_session_state(&task_app, &task_sid, &task_transfer_id);\") >\n    terminalBlock.indexOf(\"emit_transfer_finished(\")\n) {\n  throw new Error(\"transfer terminal state must not be emitted before the exclusive lease is released and session state restored\");\n}\nfor (const legacyToken of [\"HandoffPort\", \"channel_return_tx\", \"try_handoff\", \"worker.return_port(\"]) {\n  if (orchestratorSource.includes(legacyToken)) {\n    throw new Error(`legacy inline-transfer handoff token must be removed: ${legacyToken}`);\n  }\n}\n""",
)

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
    text = path.read_text()
    for lineno, line in enumerate(text.splitlines(), 1):
        if any(token in line for token in needles):
            violations.append(f"{path}:{lineno}: {line.strip()}")
if violations:
    raise SystemExit("legacy I/O references remain outside removable modules:\n" + "\n".join(violations))

# With zero external references, remove the obsolete stack rather than leaving compatibility shells.
replace("src-tauri/src/lib.rs", "mod channel;\n", "")
replace("src-tauri/src/kernel/mod.rs", "pub mod comm_handle;\n", "")
if Path("src-tauri/src/channel").exists():
    shutil.rmtree("src-tauri/src/channel")
for path in excluded:
    if path.exists():
        path.unlink()
