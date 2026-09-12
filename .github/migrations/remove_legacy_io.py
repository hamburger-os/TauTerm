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
