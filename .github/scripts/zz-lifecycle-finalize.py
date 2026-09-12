from pathlib import Path


def must_replace(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    text = p.read_text()
    actual = text.count(old)
    if actual < count:
        raise SystemExit(f"anchor missing in {path}: expected >= {count}, got {actual}: {old[:100]!r}")
    p.write_text(text.replace(old, new, count))

# Container sessions must preserve the protocol adapter's generic teardown contract just like
# ordinary DataPlane sessions. This is lifecycle metadata, not protocol semantics.
path = "src-tauri/src/kernel/session_store.rs"
must_replace(
    path,
    "    pub attachment: Option<Arc<dyn SessionAttach>>,\n}",
    "    pub attachment: Option<Arc<dyn SessionAttach>>,\n    pub teardown_delay: Duration,\n}",
)
must_replace(
    path,
    "        let ContainerSessionRuntime {\n            service,\n            file_transfer,\n            channel_factory,\n            io,\n            attachment,\n        } = runtime;",
    "        let ContainerSessionRuntime {\n            service,\n            file_transfer,\n            channel_factory,\n            io,\n            attachment,\n            teardown_delay,\n        } = runtime;",
)
must_replace(path, "            teardown_delay: Duration::ZERO,", "            teardown_delay,", 1)
must_replace(
    path,
    "/// 每个子连接有独立的 I/O loop、write channel 和统计信息。",
    "/// 每个子连接有独立的 DataPlane、SessionIo 和统计信息。",
)
must_replace(
    path,
    "/// 多对端模型：对端不占独立标签页（`tabbed = false`），拥有独立的 I/O loop、",
    "/// 多对端模型：对端不占独立标签页（`tabbed = false`），拥有独立的 DataPlane、",
)

path = "src-tauri/src/commands.rs"
must_replace(
    path,
    "//! 通过 SerialAdapter + SessionStore + Channel 架构管理会话。",
    "//! 通过协议 Adapter + SessionStore + DataPlane/SessionIo 架构管理会话。",
)
# Local Shell container is assembled directly rather than from ProtocolConnection.
must_replace(
    path,
    "                attachment: None,\n            },",
    "                attachment: None,\n                teardown_delay: std::time::Duration::ZERO,\n            },",
    1,
)
# SSH keeps the generic teardown metadata before moving connection capabilities.
must_replace(
    path,
    "    let service = conn.service;\n    let file_transfer = conn.file_transfer;\n    let channel_factory = conn.channel_factory;\n    let attachment = conn.on_attached;",
    "    let teardown_delay = conn.teardown_delay;\n    let service = conn.service;\n    let file_transfer = conn.file_transfer;\n    let channel_factory = conn.channel_factory;\n    let attachment = conn.on_attached;",
)
must_replace(
    path,
    "                io: None,\n                attachment,\n            },",
    "                io: None,\n                attachment,\n                teardown_delay,\n            },",
    1,
)
# TFTP and iperf are ProtocolConnection-backed headless/container sessions.
must_replace(
    path,
    "                io: None,\n                attachment: conn.on_attached,\n            },",
    "                io: None,\n                attachment: conn.on_attached,\n                teardown_delay: conn.teardown_delay,\n            },",
    2,
)
