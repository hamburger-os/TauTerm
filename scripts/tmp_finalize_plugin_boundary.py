from pathlib import Path


def load(path: str) -> str:
    return Path(path).read_text(encoding="utf-8")


def save(path: str, text: str) -> None:
    Path(path).write_text(text, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# SessionDataPlane: prepare the subscription/pump first but hold callbacks behind an activation gate.
path = "src-tauri/src/session/runtime.rs"
text = load(path)
text = replace_once(text, "use std::sync::Arc;", "use std::sync::{mpsc, Arc};", "runtime import")
text = replace_once(
    text,
    "    shutdown_requested: Arc<AtomicBool>,\n}",
    "    shutdown_requested: Arc<AtomicBool>,\n    activation_tx: mpsc::Sender<()>,\n    activated: Arc<AtomicBool>,\n}",
    "runtime fields",
)
start = text.index("    pub fn attach(")
end = text.index("    pub fn handle(", start)
attach_block = '''    /// Create the receive pump in a paused state. The transport subscription is already
    /// installed, so early data/close events queue until `activate()` releases the registration
    /// barrier. Callers can therefore publish SessionStore state before callbacks observe it.
    pub fn attach_paused(
        runtime: DataPlaneRuntime,
        session_id: String,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
    ) -> Result<Self, TransportError> {
        let handle = runtime.handle.clone();
        let subscription = handle.subscribe()?;
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let event_shutdown_requested = shutdown_requested.clone();
        let (activation_tx, activation_rx) = mpsc::channel::<()>();
        let activated = Arc::new(AtomicBool::new(false));
        let event_thread = std::thread::Builder::new()
            .name(format!("session-data-{session_id}"))
            .spawn(move || {
                if activation_rx.recv().is_err()
                    || event_shutdown_requested.load(Ordering::Acquire)
                {
                    return;
                }
                loop {
                    match subscription.recv() {
                        Ok(DataPlaneEvent::Data(data)) => on_data(session_id.clone(), data),
                        Ok(DataPlaneEvent::Closed(info)) => {
                            on_disconnect(session_id.clone(), info.into());
                            break;
                        }
                        Err(_) => {
                            if !event_shutdown_requested.load(Ordering::Acquire) {
                                on_disconnect(
                                    session_id.clone(),
                                    DisconnectInfo::io_error(
                                        "transport runtime stopped unexpectedly",
                                    ),
                                );
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|error| TransportError::io("session_data_pump", error))?;
        Ok(Self {
            runtime: Some(runtime),
            handle,
            event_thread: Some(event_thread),
            shutdown_requested,
            activation_tx,
            activated,
        })
    }

    /// Convenience path for owners that do not need a registration barrier.
    pub fn attach(
        runtime: DataPlaneRuntime,
        session_id: String,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
    ) -> Result<Self, TransportError> {
        let owner = Self::attach_paused(runtime, session_id, on_data, on_disconnect)?;
        owner.activate();
        Ok(owner)
    }

    /// Release the registration barrier. All fallible setup completed in `attach_paused`, so
    /// activation is intentionally idempotent and infallible.
    pub fn activate(&self) {
        if self.activated.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.activation_tx.send(());
    }

'''
text = text[:start] + attach_block + text[end:]
text = replace_once(
    text,
    "        if self.shutdown_requested.swap(true, Ordering::AcqRel) {\n            return;\n        }\n        let _ = self.handle.shutdown();",
    "        if self.shutdown_requested.swap(true, Ordering::AcqRel) {\n            return;\n        }\n        // Wake a pump that may still be waiting for publication. It sees shutdown intent and\n        // exits without emitting a synthetic disconnect.\n        self.activate();\n        let _ = self.handle.shutdown();",
    "runtime shutdown activation",
)
marker = "    #[test]\n    fn unexpected_transport_actor_exit_surfaces_disconnect_and_clears_connected_state()"
paused_test = '''    #[test]
    fn paused_attachment_defers_disconnect_until_activation() {
        let panic_now = std::sync::Arc::new(AtomicBool::new(false));
        let runtime = DataPlaneRuntime::spawn(Box::new(PanicAfterGate {
            panic_now: panic_now.clone(),
        }));
        let (disconnect_tx, disconnect_rx) = mpsc::channel();
        let mut owner = SessionDataPlane::attach_paused(
            runtime,
            "paused-session".into(),
            Box::new(|_, _| {}),
            Box::new(move |_, info| {
                let _ = disconnect_tx.send(info);
            }),
        )
        .unwrap();

        panic_now.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(25));
        assert!(disconnect_rx.try_recv().is_err());

        owner.activate();
        let info = disconnect_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("queued disconnect should surface after activation");
        assert_eq!(info.reason, "transport runtime stopped unexpectedly");
        owner.shutdown();
    }

'''
text = replace_once(text, marker, paused_test + marker, "runtime paused test")
save(path, text)

# SessionStore creates roots paused and exposes one protocol-neutral activation entry point.
path = "src-tauri/src/kernel/session_store.rs"
text = load(path)
text = replace_once(
    text,
    "let data_plane = SessionDataPlane::attach(runtime, id.clone(), on_data, on_disconnect)",
    "let data_plane = SessionDataPlane::attach_paused(runtime, id.clone(), on_data, on_disconnect)",
    "root paused attach",
)
marker = "    /// 关闭单个子连接（两段式）。"
method = '''    /// Activate a previously registered root or child DataPlane.
    ///
    /// Registration-sensitive connectors publish SessionStore state and their connected event
    /// before releasing this barrier, so data/disconnect callbacks cannot race ahead of them.
    pub fn activate_data_plane(&self, session_id: &str) -> Result<(), String> {
        if let Some(handle) = self.sessions.get(session_id) {
            let data_plane = handle
                .data_plane
                .as_ref()
                .ok_or_else(|| format!("会话 {} 不包含 DataPlane", session_id))?;
            data_plane.activate();
            return Ok(());
        }
        let (parent_id, index) = self
            .find_sub_connection_index(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;
        let sub = self
            .sessions
            .get(&parent_id)
            .and_then(|handle| handle.sub_connections.get(index))
            .ok_or_else(|| self.session_not_found(session_id))?;
        let data_plane = sub
            .data_plane
            .as_ref()
            .ok_or_else(|| format!("子连接 {} 不包含 DataPlane", session_id))?;
        data_plane.activate();
        Ok(())
    }

'''
text = replace_once(text, marker, method + marker, "session activation method")
save(path, text)

# Generic root/child application helpers.
path = "src-tauri/src/plugin_application.rs"
text = load(path)
text = replace_once(
    text,
    "SessionDataPlane::attach(runtime, channel_id.clone(), on_data, on_disconnect)",
    "SessionDataPlane::attach_paused(runtime, channel_id.clone(), on_data, on_disconnect)",
    "generic child paused attach",
)
text = replace_once(
    text,
    "    );\n    Ok(session_id)\n}\n\n/// 在父配置上注册一个协议无关的终端子会话。",
    "    );\n    state\n        .session_store\n        .lock()\n        .map_err(|e| e.to_string())?\n        .activate_data_plane(&session_id)?;\n    Ok(session_id)\n}\n\n/// 在父配置上注册一个协议无关的终端子会话。",
    "generic root activation",
)
text = replace_once(
    text,
    "    }\n    Ok(channel_id)\n}\n\npub(crate) fn terminal_sub_channel_connected_payload",
    "    }\n    if announce_connected {\n        app_state\n            .session_store\n            .lock()\n            .map_err(|e| e.to_string())?\n            .activate_data_plane(&channel_id)?;\n    }\n    Ok(channel_id)\n}\n\npub(crate) fn terminal_sub_channel_connected_payload",
    "generic child activation",
)
save(path, text)

# Serial root publication ordering.
path = "src-tauri/src/plugins/serial/commands.rs"
text = load(path)
text = replace_once(
    text,
    "    );\n    Ok(sid)\n}",
    "    );\n    state\n        .session_store\n        .lock()\n        .map_err(|e| e.to_string())?\n        .activate_data_plane(&sid)?;\n    Ok(sid)\n}",
    "serial activation",
)
save(path, text)

# Network root + peer publication ordering.
path = "src-tauri/src/plugins/network/mod.rs"
text = load(path)
text = replace_once(
    text,
    "SessionDataPlane::attach(runtime, channel_id.clone(), on_data, on_disconnect)",
    "SessionDataPlane::attach_paused(runtime, channel_id.clone(), on_data, on_disconnect)",
    "network peer paused attach",
)
text = replace_once(
    text,
    "    );\n    Ok(channel_id)\n}\n\nfn emit_udp_datagram(",
    "    );\n    app_state\n        .session_store\n        .lock()\n        .map_err(|error| error.to_string())?\n        .activate_data_plane(&channel_id)?;\n    Ok(channel_id)\n}\n\nfn emit_udp_datagram(",
    "network peer activation",
)
save(path, text)

path = "src-tauri/src/plugins/network/commands.rs"
text = load(path)
text = replace_once(
    text,
    "    );\n    Ok(sid)\n}\n\n/// UDP 发送公共实现",
    "    );\n    state\n        .session_store\n        .lock()\n        .map_err(|e| e.to_string())?\n        .activate_data_plane(&sid)?;\n    Ok(sid)\n}\n\n/// UDP 发送公共实现",
    "network root activation",
)
save(path, text)

# SSH initial child activation follows parent+child publication.
path = "src-tauri/src/plugins/ssh/commands.rs"
text = load(path)
text = replace_once(
    text,
    "    let _ = app.emit(\"session-connected\", channel0_connected);\n\n    Ok(parent_id)",
    "    let _ = app.emit(\"session-connected\", channel0_connected);\n    state\n        .session_store\n        .lock()\n        .map_err(|e| e.to_string())?\n        .activate_data_plane(&channel0_id)?;\n\n    Ok(parent_id)",
    "ssh initial child activation",
)
save(path, text)

# Local Shell initial child uses the same parent->child->activate ordering as SSH.
path = "src-tauri/src/plugins/local_shell/commands.rs"
text = load(path)
text = replace_once(
    text,
    "    create_terminal_sub_channel, ConnectSessionRequest, SessionConnectFuture,\n",
    "    create_terminal_sub_channel, terminal_sub_channel_connected_payload, ConnectSessionRequest,\n    SessionConnectFuture,\n",
    "local shell helper import",
)
text = replace_once(
    text,
    "        initial_mode == ChannelOpenMode::Elevated,\n        true,\n",
    "        initial_mode == ChannelOpenMode::Elevated,\n        false,\n",
    "local shell delayed announce",
)
anchor = "    let connected_at = Some(\n"
child_payload = '''    let child_connected = terminal_sub_channel_connected_payload(&state, &parent_id, &channel_id)
        .inspect_err(|error| {
            log::error!("Local Shell 首个子会话发布前校验失败: {error}");
            if let Ok(mut store) = state.session_store.lock() {
                let _ = store.close_session(&parent_id);
            }
        })?;

'''
text = replace_once(text, anchor, child_payload + anchor, "local shell child payload")
text = replace_once(
    text,
    "    );\n    log::info!(\n        \"Local Shell 父会话已连接: {} (child: {})\",",
    "    );\n    let _ = app.emit(\"session-connected\", child_connected);\n    state\n        .session_store\n        .lock()\n        .map_err(|e| e.to_string())?\n        .activate_data_plane(&channel_id)?;\n    log::info!(\n        \"Local Shell 父会话已连接: {} (child: {})\",",
    "local shell child publish/activate",
)
save(path, text)

# Frontend: protocol-specific error presentation belongs to the plugin registration.
path = "src/core/plugin-registry.ts"
text = load(path)
text = replace_once(
    text,
    "  /** 对声明 elevated_session 的插件，由插件判断当前配置是否允许创建提权 Session。 */\n  canCreateElevatedSession?: (params: Record<string, unknown>) => boolean;",
    "  /** 插件拥有连接/子通道错误的用户可见格式；公共 Session 层不解释协议错误文本。 */\n  formatSessionError?: (error: unknown, operation: \"connect\" | \"open_channel\") => string;\n  /** 对声明 elevated_session 的插件，由插件判断当前配置是否允许创建提权 Session。 */\n  canCreateElevatedSession?: (params: Record<string, unknown>) => boolean;",
    "frontend error contribution",
)
save(path, text)

path = "src/context/SessionContext.tsx"
text = load(path)
text = replace_once(
    text,
    "      dispatch({ type: \"SET_ERROR\", error: String(e) });\n      if (sessionId) dispatch({ type: \"SET_TAB_STATE\", id: sessionId, state: \"disconnected\" });",
    "      const error = plugin?.formatSessionError?.(e, \"connect\") ?? String(e);\n      dispatch({ type: \"SET_ERROR\", error });\n      if (sessionId) dispatch({ type: \"SET_TAB_STATE\", id: sessionId, state: \"disconnected\" });",
    "connect error formatter",
)
text = replace_once(
    text,
    "    } catch (e) {\n      dispatch({ type: \"SET_ERROR\", error: String(e) });\n      return null;\n    }\n  }, []);\n\n  const closeChannel",
    "    } catch (e) {\n      const parent = tabsRef.current.find(tab => tab.id === parentSessionId);\n      const plugin = parent ? pluginRegistry.get(parent.pluginId) : undefined;\n      const error = plugin?.formatSessionError?.(e, \"open_channel\") ?? String(e);\n      dispatch({ type: \"SET_ERROR\", error });\n      return null;\n    }\n  }, []);\n\n  const closeChannel",
    "open channel error formatter",
)
save(path, text)

path = "src/plugins/local-shell/index.ts"
text = load(path)
text = replace_once(
    text,
    "import manifestJson from \"../../plugin-manifests/local-shell.json\";",
    "import i18n from \"../../i18n\";\nimport manifestJson from \"../../plugin-manifests/local-shell.json\";",
    "local shell i18n import",
)
text = replace_once(
    text,
    "  canCreateElevatedSession: params => params.shell_kind !== \"wsl\",",
    "  formatSessionError: (error, operation) => {\n    const raw = String(error);\n    const detail = raw.includes(\"User cancelled the UAC elevation prompt\")\n      ? i18n.t(\"localShell.elevationCancelled\")\n      : raw;\n    const key = operation === \"open_channel\" ? \"localShell.openFailed\" : \"localShell.connectFailed\";\n    return `${i18n.t(key)}: ${detail}`;\n  },\n  canCreateElevatedSession: params => params.shell_kind !== \"wsl\",",
    "local shell error formatter",
)
save(path, text)

# Owner docs.
path = "docs/modules/CORE.md"
text = load(path)
text = replace_once(
    text,
    "`Transferring` 只表示 Session 的 inline 独占资源被传输任务占用；真实底层资源仍归 DataPlane Runtime 所有。连接、断开、异常退出、子连接关闭和传输结束必须通过统一生命周期收敛。",
    "`Transferring` 只表示 Session 的 inline 独占资源被传输任务占用；真实底层资源仍归 DataPlane Runtime 所有。连接、断开、异常退出、子连接关闭和传输结束必须通过统一生命周期收敛。`SessionDataPlane` 对需要注册顺序保证的 Session/child 使用 paused attach：先建立订阅与事件线程但阻塞回调，待 SessionStore 注册和 `session-connected`/peer joined 发布完成后再显式 activate；因此数据与断开事件不能抢在权威 Session 状态之前。",
    "core lifecycle docs",
)
text = replace_once(
    text,
    "插件也可以通过 `persistedConnectionParams`、`reconnectGuard` 与 `sessionPresentation` 分别拥有持久化参数投影、重连前置策略和会话展示格式。",
    "插件也可以通过 `persistedConnectionParams`、`reconnectGuard`、`formatSessionError` 与 `sessionPresentation` 分别拥有持久化参数投影、重连前置策略、协议错误展示和会话展示格式。",
    "core frontend policy docs",
)
save(path, text)

path = "docs/modules/UI_FOUNDATION.md"
text = load(path)
needle = "- 插件私有运行态必须留在插件自己的 Session store/hook；公共状态栏上下文只提供 Session ID、连接状态、端点、通用参数和统计等协议无关信息。"
if needle not in text:
    raise SystemExit("UI_FOUNDATION error-policy anchor missing")
text = text.replace(
    needle,
    needle + "\n- 连接/子通道失败的协议专属错误格式化由插件 registration 贡献；公共 SessionContext 只负责调用 formatter 和维护通用连接状态，不解析 UAC、SSH、TFTP 等错误文本。",
    1,
)
save(path, text)

# Architecture guards.
path = "src-tauri/src/architecture_contract.rs"
text = load(path)
insert_before = "\n#[test]\nfn frontend_generic_plugin_registry_has_no_protocol_private_status_state()"
contract = r'''
#[test]
fn session_data_plane_registration_is_two_phase() {
    let runtime = read("src/session/runtime.rs");
    assert!(runtime.contains("pub fn attach_paused("));
    assert!(runtime.contains("pub fn activate(&self)"));

    let store = read("src/kernel/session_store.rs");
    assert!(store.contains("SessionDataPlane::attach_paused"));
    assert!(store.contains("pub fn activate_data_plane(&self"));

    let application = read("src/plugin_application.rs");
    assert!(application.contains("SessionDataPlane::attach_paused"));
    assert!(application.contains("activate_data_plane(&channel_id)"));

    let network = read("src/plugins/network/mod.rs");
    assert!(network.contains("SessionDataPlane::attach_paused"));
    assert!(network.contains("activate_data_plane(&channel_id)"));
}
'''
text = replace_once(
    text,
    insert_before,
    "\n" + contract + "\n#[test]\nfn frontend_generic_plugin_registry_has_no_protocol_private_status_state()",
    "lifecycle architecture contract",
)
marker = "    assert!(session_context.contains(\"reconnectGuard\"));"
text = replace_once(
    text,
    marker,
    marker + "\n    assert!(session_context.contains(\"formatSessionError\"));\n    assert!(!session_context.contains(\"User cancelled the UAC elevation prompt\"));\n    assert!(!session_context.contains(\"localShell.elevationCancelled\"));",
    "frontend error architecture contract",
)
save(path, text)

# Make forgotten inactive-root call sites an executable architecture violation.
callers = []
for candidate in Path("src-tauri/src").rglob("*.rs"):
    content = candidate.read_text(encoding="utf-8")
    if ".create_session(" in content:
        callers.append(candidate.as_posix())
allowed = {
    "src-tauri/src/plugin_application.rs",
    "src-tauri/src/plugins/network/commands.rs",
    "src-tauri/src/plugins/serial/commands.rs",
}
unexpected = sorted(set(callers) - allowed)
if unexpected:
    raise SystemExit(f"unexpected create_session callers: {unexpected}")
print("root create_session callers:", sorted(callers))
