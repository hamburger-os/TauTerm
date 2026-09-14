from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def edit(path: str, old: str, new: str, count: int = 1) -> None:
    file = ROOT / path
    text = file.read_text(encoding="utf-8")
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} matches, found {actual}: {old[:100]!r}")
    file.write_text(text.replace(old, new, count), encoding="utf-8")

# Never let a full bounded subscriber block the DataPlane actor while publishing close.
edit(
    "src-tauri/src/transport/runtime.rs",
    '''    subscribers.retain(|(_, subscriber)| {
        subscriber
            .send(DataPlaneEvent::Closed(info.clone()))
            .is_ok()
    });
''',
    '''    subscribers.retain(|(id, subscriber)| {
        match subscriber.try_send(DataPlaneEvent::Closed(info.clone())) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                log::warn!(
                    "DataPlane subscriber {} backlog was full while closing; detaching consumer",
                    id
                );
                false
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        }
    });
''',
)

# Remove stale bridge comments after the direct SessionIo/DataPlane refactor.
edit(
    "src-tauri/src/commands.rs",
    '''/// 创建 on_data 回调（含 DataBatcher + 日志记录 + 可选虚拟端口转发）。
///
/// DataBatcher 的所有权被移入回调闭包（通过 `batcher.push()` 消费数据），
/// 因此只返回 `Box<dyn Fn>`；`DataBatcher::Drop` 在会话断开时自动 flush + 清理。
///
/// `bridge_tx` 为可选虚拟端口转发通道（仅串口会话提供）。
/// 全部会话类型共用此函数，消除 ~60 行重复代码。
''',
    '''/// 创建共享 on_data 回调（DataBatcher + 日志记录）。
///
/// DataBatcher 的所有权被移入回调闭包（通过 `batcher.push()` 消费数据），
/// 因此只返回 `Box<dyn Fn>`；`DataBatcher::Drop` 在会话断开时自动 flush + 清理。
/// 虚拟串口不经过 UI 回调旁路，而是直接订阅 DataPlane。
''',
)
edit(
    "src-tauri/src/commands.rs",
    '''    // 共享 on_data 回调：DataBatcher + 日志 + 虚拟端口转发
    // 数据推送至脚本引擎由 SessionDataPlane subscription 统一扇出
''',
    '''    // 共享 on_data 回调只负责 UI 批处理与日志；脚本/虚拟串口均通过
    // DataPlane subscription 独立消费，避免耦合或静默丢字节。
''',
)
edit(
    "src-tauri/src/commands.rs",
    '''    // ── Virtual port pair creation + bridge thread setup ──
    // TODO: Extract into setup_virtual_external_pathridge() helper once the parameter
    // surface stabilizes (currently touches vpm, session_store, app, bridge channel).
''',
    '''    // ── Virtual port pair creation + direct DataPlane/SessionIo bridge ──
''',
)

# Session-connected must respect the same current plugin schema as create/edit/load paths.
p = ROOT / "src/context/SessionContext.tsx"
text = p.read_text(encoding="utf-8")
old = '''          const eventPluginId = event.payload.plugin_id || event.payload.connection_type || "serial";
          const eventSendBarEnabled = pluginRegistry.resolveSendBarEnabled(eventPluginId, event.payload.send_bar_enabled);
'''
new = '''          const eventPluginId = event.payload.plugin_id || event.payload.connection_type || "serial";
          const eventParams = pluginRegistry.get(eventPluginId)?.normalizeConnectionParams?.(event.payload.params) ?? event.payload.params;
          const eventSendBarEnabled = pluginRegistry.resolveSendBarEnabled(eventPluginId, event.payload.send_bar_enabled);
'''
if text.count(old) != 1:
    raise SystemExit(f"SessionContext: expected one event plugin anchor, found {text.count(old)}")
text = text.replace(old, new, 1)
# Restrict replacements to the session-connected listener before virtual-port-created listener.
start = text.index('const eventPluginId = event.payload.plugin_id')
end = text.index('const u2b = await listen', start)
segment = text[start:end]
segment = segment.replace('params: event.payload.params,', 'params: eventParams,')
segment = segment.replace('(event.payload.params?.file_service_enabled as boolean)', '(eventParams?.file_service_enabled as boolean)')
segment = segment.replace('(event.payload.params?.file_service_protocol as string)', '(eventParams?.file_service_protocol as string)')
segment = segment.replace('(event.payload.params?.journald_enabled as boolean)', '(eventParams?.journald_enabled as boolean)')
segment = segment.replace('                virtualPortEnabled: (event.payload.params?.virtual_port_enabled as boolean) ?? false,\n', '')
segment = segment.replace('                virtualPortCount: (event.payload.params?.virtual_port_count as number) ?? 0,\n', '')
text = text[:start] + segment + text[end:]
old_doc = '  /** 虚拟端口失败原因分类（driver_missing | files_missing | permission | create_failed），供前端本地化 */'
if text.count(old_doc) != 1:
    raise SystemExit(f"SessionContext: expected one VPort error doc, found {text.count(old_doc)}")
text = text.replace(
    old_doc,
    '  /** 虚拟端口失败原因分类（driver_missing | files_missing | permission | create_failed | bridge_failed），供前端本地化 */',
    1,
)
p.write_text(text, encoding="utf-8")

print("serial review fixes applied")
