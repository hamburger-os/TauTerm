from pathlib import Path


def replace(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing: {path}: {old[:80]!r}")
    p.write_text(text.replace(old, new, 1))

# Diagnostics now reads canonical SessionIo counters. For container-style sessions without
# a root data plane, aggregate child counters instead.
replace(
    "src-tauri/src/diagnostics.rs",
    """            aggregate.child_connections += session.sub_connections.len() as u64;\n            aggregate.tx_bytes += session.tx_bytes.load(std::sync::atomic::Ordering::Relaxed);\n            aggregate.rx_bytes += session.rx_bytes.load(std::sync::atomic::Ordering::Relaxed);\n""",
    """            aggregate.child_connections += session.sub_connections.len() as u64;\n            if let Some(io) = session.io.as_ref() {\n                aggregate.tx_bytes += io.tx_bytes();\n                aggregate.rx_bytes += io.rx_bytes();\n            } else {\n                aggregate.tx_bytes += session\n                    .sub_connections\n                    .iter()\n                    .map(|child| child.io.tx_bytes())\n                    .sum::<u64>();\n                aggregate.rx_bytes += session\n                    .sub_connections\n                    .iter()\n                    .map(|child| child.io.rx_bytes())\n                    .sum::<u64>();\n            }\n""",
)

replace(
    "src-tauri/src/session/mod.rs",
    "pub use io::{SessionIo, SessionIoError, TargetedIo};\npub use runtime::SessionDataPlane;\npub use state::{DisconnectInfo, DisconnectKind};\n",
    "pub use io::SessionIo;\npub use runtime::SessionDataPlane;\npub use state::DisconnectInfo;\n",
)

replace(
    "src-tauri/src/plugins/modbus/codec/mod.rs",
    """pub use pdu::{\n    decode_request, encode_request, validate_response, DecodedRequest, FileRecordRead,\n    FileRecordWrite, ModbusRequest, ModbusResponse,\n};\n""",
    """pub use pdu::{\n    decode_request, encode_request, validate_response, FileRecordRead, FileRecordWrite, ModbusRequest,\n};\n""",
)

p = Path("src-tauri/src/plugins/tftp/mod.rs")
text = p.read_text().replace("SessionError::IoError", "SessionError::Io")
p.write_text(text)

# Keep a protocol-native connection state accessor for tests/diagnostics; terminal resize is the
# transport capability, not the deleted Channel::resize_pty API.
replace(
    "src-tauri/src/plugins/telnet/channel.rs",
    """    pub fn new(\n        telnet: Telnet,\n        probe: std::net::TcpStream,\n        on_echo_change: Box<dyn Fn(bool) + Send>,\n        session_id_slot: Arc<Mutex<Option<String>>>,\n    ) -> Self {\n""",
    """    pub fn new(\n        telnet: Telnet,\n        probe: std::net::TcpStream,\n        on_echo_change: Box<dyn Fn(bool) + Send>,\n        session_id_slot: Arc<Mutex<Option<String>>>,\n    ) -> Self {\n""",
)
replace(
    "src-tauri/src/plugins/telnet/channel.rs",
    """            connected: true,\n        }\n    }\n\n    /// 协商策略表""",
    """            connected: true,\n        }\n    }\n\n    pub(crate) fn is_connected(&self) -> bool {\n        self.connected\n    }\n\n    /// 协商策略表""",
)
replace(
    "src-tauri/src/plugins/telnet/mod.rs",
    "channel.resize_pty(132, 43).expect(\"NAWS 发送失败\");",
    "crate::transport::BlockingByteStream::resize_terminal(&mut channel, 132, 43)\n            .expect(\"NAWS 发送失败\");",
)

# Network peer receive data is mirrored into the root aggregate DataPlane for scripts/auto-reply.
replace(
    "src-tauri/src/kernel/session_store.rs",
    """    pub data_mode: String,\n    pub peer_handles: Arc<Mutex<HashMap<String, DataPlaneHandle>>>,\n}\n""",
    """    pub data_mode: String,\n    pub peer_handles: Arc<Mutex<HashMap<String, DataPlaneHandle>>>,\n    pub mirror_tx: Option<mpsc::Sender<Vec<u8>>>,\n}\n""",
)
replace(
    "src-tauri/src/kernel/session_store.rs",
    """            data_mode,\n            peer_handles,\n        } = registration;\n""",
    """            data_mode,\n            peer_handles,\n            mirror_tx,\n        } = registration;\n""",
)
replace(
    "src-tauri/src/kernel/session_store.rs",
    """        let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {\n            let payload = data.clone();\n            batcher.push(session_id.clone(), data);\n            let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {\n""",
    """        let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {\n            let payload = data.clone();\n            let mirror_payload = mirror_tx.as_ref().map(|_| data.clone());\n            batcher.push(session_id.clone(), data);\n            if let (Some(tx), Some(payload)) = (mirror_tx.as_ref(), mirror_payload) {\n                let _ = tx.send(payload);\n            }\n            let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {\n""",
)

# The network root owns its aggregate DataPlane. Peer/UDP callbacks keep their address-aware UI
# events; root on_data intentionally does not duplicate those events.
replace(
    "src-tauri/src/commands.rs",
    """    let conn = state\n        .network_adapter\n        .connect(&endpoint, &params)\n        .await\n        .map_err(|e| e.to_string())?;\n\n    let sid = {\n        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;\n        store.create_container_session(\n            ContainerSessionCreateOptions {\n                name: name.unwrap_or_else(|| \"网络调试\".to_string()),\n                plugin_id: \"network\".into(),\n                endpoint: endpoint.clone(),\n                params: params.clone(),\n                transfer_enabled: transfer_enabled.unwrap_or(false),\n                transfer_protocol,\n                send_bar_enabled: send_bar_enabled.unwrap_or(true),\n                id_override: session_id,\n            },\n            conn.side_channel.clone(),\n            None,\n            None,\n        )?\n    };\n\n    // 启动监听 / 接收线程（TCP Client 注册对端、TCP Server accept、UDP recv 路由）\n    if let Some(sc) = &conn.side_channel {\n""",
    """    let conn = state\n        .network_adapter\n        .connect(&endpoint, &params)\n        .await\n        .map_err(|e| e.to_string())?;\n    let network_side = conn.side_channel.clone();\n\n    let sid = {\n        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;\n        store.create_session(\n            SessionCreateOptions {\n                name: name.unwrap_or_else(|| \"网络调试\".to_string()),\n                plugin_id: \"network\".into(),\n                endpoint: endpoint.clone(),\n                params: params.clone(),\n                transfer_enabled: transfer_enabled.unwrap_or(false),\n                transfer_protocol,\n                send_bar_enabled: send_bar_enabled.unwrap_or(true),\n                id_override: session_id,\n            },\n            conn,\n            Box::new(|_, _| {}),\n            Box::new(|_, _| {}),\n            app.clone(),\n        )?\n    };\n\n    // 启动监听 / 接收线程（TCP Client 注册对端、TCP Server accept、UDP recv 路由）\n    if let Some(sc) = &network_side {\n""",
)
replace(
    "src-tauri/src/commands.rs",
    """    let udp_local_addr = conn\n        .side_channel\n        .as_ref()\n""",
    """    let udp_local_addr = network_side\n        .as_ref()\n""",
)
