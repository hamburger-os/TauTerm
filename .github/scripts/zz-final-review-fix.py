from pathlib import Path


# Session lifecycle: disconnected/zombie handles must detach typed runtime registries even when
# they are removed without the normal close_session path. Keep shutdown non-blocking in Drop.
store_path = Path("src-tauri/src/kernel/session_store.rs")
store = store_path.read_text()
old = '''        self.transfer_scheduler.cancel_for_shutdown();
        if let Some(flag) = &self.stats_cancel_flag {'''
new = '''        if let Some(attachment) = self.attachment.take() {
            attachment.on_detached(&self.id);
        }
        if let Some(service) = self.service.take() {
            std::thread::spawn(move || service.shutdown());
        }
        self.file_transfer = None;
        self.channel_factory = None;
        self.transfer_scheduler.cancel_for_shutdown();
        if let Some(flag) = &self.stats_cancel_flag {'''
if old not in store:
    raise SystemExit("ActiveSessionHandle Drop anchor missing")
store = store.replace(old, new, 1)

old = '''        if let Some(raw) = id_override.as_ref() {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            if self
                .sessions
                .get(raw)
                .is_some_and(|h| h.state == SessionState::Disconnected)
            {
                self.sessions.remove(raw);
            }
        }
        self.purge_zombies();'''
new = '''        if let Some(raw) = id_override.as_ref() {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            match self.sessions.get(raw).map(|handle| handle.state.clone()) {
                Some(SessionState::Disconnected) => {
                    self.sessions.remove(raw);
                }
                Some(_) => {
                    return Err(format!("会话 {} 已存在且未断开，拒绝覆盖活动运行时", raw));
                }
                None => {}
            }
        }
        self.purge_zombies();'''
if old not in store:
    raise SystemExit("create_session id guard anchor missing")
store = store.replace(old, new, 1)

old = '''        if let Some(old) = self.sessions.remove(&id) {
            drop(old);
        }
        self.tab_order.retain(|tid| tid != &id);'''
new = '''        debug_assert!(!self.sessions.contains_key(&id));
        self.tab_order.retain(|tid| tid != &id);'''
if old not in store:
    raise SystemExit("create_session replacement anchor missing")
store = store.replace(old, new, 1)

old = '''        if self
            .sessions
            .get(&id)
            .is_some_and(|h| h.state == SessionState::Disconnected)
        {
            self.sessions.remove(&id);
        }
        self.purge_zombies();'''
new = '''        match self.sessions.get(&id).map(|handle| handle.state.clone()) {
            Some(SessionState::Disconnected) => {
                self.sessions.remove(&id);
            }
            Some(_) => {
                return Err(format!("会话 {} 已存在且未断开，拒绝覆盖活动运行时", id));
            }
            None => {}
        }
        self.purge_zombies();'''
if old not in store:
    raise SystemExit("create_container_session id guard anchor missing")
store = store.replace(old, new, 1)
store_path.write_text(store)

# Eliminate the old kernel SideChannel vocabulary. In the file-transfer subsystem the replacement
# term is Auxiliary; everywhere else protocol-specific objects are typed runtimes.
for path in Path("src-tauri/src").rglob("*.rs"):
    text = path.read_text()
    if "src-tauri/src/transfer/" in path.as_posix():
        text = text.replace("SideChannelTransferOrchestrator", "AuxiliaryTransferOrchestrator")
        text = text.replace("SideChannel", "Auxiliary")
        text = text.replace("side_channel", "auxiliary")
    else:
        text = text.replace("SideChannel", "Runtime")
        text = text.replace("side_channel", "runtime")
    text = text.replace("侧通道资源", "类型化运行时资源")
    path.write_text(text)

# Lifecycle contract tests must use the same Auxiliary naming as implementation.
lifecycle = Path("scripts/test-file-transfer-lifecycle.mjs")
text = lifecycle.read_text().replace("SideChannel", "Auxiliary").replace("side_channel", "auxiliary")
lifecycle.write_text(text)

# Fix stale SSH/iPerf prose left from the old erased runtime model.
ssh = Path("src-tauri/src/plugins/ssh/mod.rs")
text = ssh.read_text()
text = text.replace(
    "文件服务（SFTP）通过独立的独立协议能力操作，不中断终端 I/O 循环。",
    "文件服务（SFTP）通过显式 FileTransfer capability 操作，不中断终端 I/O 循环。",
)
text = text.replace(
    "/// 持有 SSH 会话引用和缓存的 SFTP 对象，通过 `ProtocolConnection::runtime`\n/// 传递给 `SessionStore`。SFTP 命令通过 `downcast_ref::<SshRuntime>()` 还原。",
    "/// 持有 SSH 会话引用和缓存的 SFTP 对象。SessionStore 只持有协议无关生命周期\n/// capability；SSH 命令通过插件自己的 typed runtime registry 按 session_id 获取本对象。",
)
text = text.replace(
    "/// 持有 SSH 会话引用和缓存的 SFTP 对象，由插件 typed runtime registry 按 session_id 管理。SFTP 命令通过 `downcast_ref::<SshRuntime>()` 还原。",
    "/// 持有 SSH 会话引用和缓存的 SFTP 对象。SessionStore 只持有协议无关生命周期\n/// capability；SSH 命令通过插件自己的 typed runtime registry 按 session_id 获取本对象。",
)
ssh.write_text(text)

iperf = Path("src-tauri/src/plugins/iperf/mod.rs")
text = iperf.read_text()
text = text.replace("//! 采用 Runtime 模式（对齐 TFTP），会话为容器模式（无终端 I/O 循环）。", "//! 采用容器 Session + typed runtime registry，不创建终端 DataPlane。")
text = text.replace("/// iperf 独立协议能力资源", "/// iperf 类型化运行时")
text = text.replace("/// 创建新的 iperf 独立协议能力", "/// 创建新的 iperf runtime")
text = text.replace("/// 通过 `ProtocolConnection::runtime` 传递给 `SessionStore`。", "/// SessionStore 只持有 SessionService；协议命令从插件 typed registry 获取本 runtime。")
iperf.write_text(text)

# Current architecture docs must use the new capability contract.
core = Path("docs/modules/CORE.md")
text = core.read_text()
text = text.replace(
    "协议 Adapter 负责建立协议资源，并以 `ProtocolConnection` 返回 `DataPlaneRuntime`、可选 SideChannel、可选子终端工厂等明确能力；",
    "协议 Adapter 负责建立协议资源，并以 `ProtocolConnection` 返回 `DataPlaneRuntime`、可选 `SessionService`、显式 `FileTransfer` capability、可选子终端工厂和 attach hook 等明确能力；",
)
core.write_text(text)

transfer = Path("docs/modules/TRANSFER.md")
text = transfer.read_text()
text = text.replace("**SideChannel**", "**Auxiliary**")
text = text.replace("ExclusiveIo/SideChannel", "ExclusiveIo/Auxiliary")
text = text.replace("SideChannel 命令", "辅助传输命令")
text = text.replace('Orchestrator --> Side["SideChannel"]', 'Orchestrator --> Side["Auxiliary FileTransfer"]')
text = text.replace("SideChannel 不应阻塞普通终端 I/O。", "辅助文件传输 capability 不应阻塞普通终端 I/O。")
text = text.replace("SideChannel 后台 task", "辅助文件传输后台 task")
text = text.replace("lease/side resource", "lease/auxiliary resource")
transfer.write_text(text)

for path in Path("docs/modules").rglob("*.md"):
    text = path.read_text().replace("SideChannel", "auxiliary capability").replace("side_channel", "runtime")
    path.write_text(text)

# No erased protocol-runtime architecture may remain. Generic downcasts elsewhere are not banned;
# the ban is specifically on the removed Session runtime Any/as_any path and concrete runtime casts.
runtime_names = ["SshRuntime", "NetworkRuntime", "TftpRuntime", "IperfRuntime", "TrdpRuntime", "ModbusRuntime"]
for root in [Path("src-tauri/src"), Path("docs/modules")]:
    for path in root.rglob("*"):
        if not path.is_file() or path.suffix not in {".rs", ".md"}:
            continue
        text = path.read_text(errors="ignore")
        for token in ["SideChannel", "side_channel", "get_side_channel", ".as_any()", "std::any::Any"]:
            if token in text:
                raise SystemExit(f"forbidden runtime token {token!r} remains in {path}")
        for runtime_name in runtime_names:
            if f"downcast_ref::<{runtime_name}" in text or f"downcast::<{runtime_name}" in text:
                raise SystemExit(f"forbidden runtime downcast for {runtime_name} remains in {path}")
