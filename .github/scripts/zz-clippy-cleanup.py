from pathlib import Path
import re


def must_replace(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    text = p.read_text()
    actual = text.count(old)
    if actual < count:
        raise SystemExit(f"anchor missing in {path}: expected >= {count}, got {actual}: {old[:100]!r}")
    p.write_text(text.replace(old, new, count))

# Redundant field names created by runtime terminology cleanup.
for path in ["src-tauri/src/plugins/iperf/mod.rs", "src-tauri/src/plugins/tftp/mod.rs"]:
    p = Path(path)
    text = p.read_text().replace("RuntimeAttach { runtime: runtime }", "RuntimeAttach { runtime }")
    p.write_text(text)

# Keep constructor visibility no broader than the type in its signature.
must_replace(
    "src-tauri/src/plugins/ssh/handler.rs",
    "    pub fn new(verifier_tx: tokio::sync::mpsc::Sender<HostKeyVerification>) -> Self {",
    "    pub(crate) fn new(verifier_tx: tokio::sync::mpsc::Sender<HostKeyVerification>) -> Self {",
)

# ProtocolConnection already exposes file_transfer explicitly; the unused adapter forwarding method
# is duplicate API surface.
p = Path("src-tauri/src/kernel/plugin_adapter.rs")
text = p.read_text()
text = re.sub(
    r"\n    fn create_file_transfer\(\n        &self,\n        connection: &ProtocolConnection,\n    \) -> Option<Arc<dyn FileTransfer>> \{\n        connection\.file_transfer\.clone\(\)\n    \}\n",
    "\n",
    text,
    count=1,
)
p.write_text(text)

# TcpFramer exposes only behavior used by production; test-only introspection stays test-only.
p = Path("src-tauri/src/plugins/modbus/codec/tcp.rs")
text = p.read_text()
text = re.sub(r"\n    pub fn clear\(&mut self\) \{\n        self\.buffer\.clear\(\);\n    \}\n", "\n", text, count=1)
text = text.replace("    pub fn buffered_len(&self) -> usize {", "    #[cfg(test)]\n    fn buffered_len(&self) -> usize {")
p.write_text(text)

# Telnet attach slot is already captured by the echo callback and attach hook; the driver did not
# read its duplicate Arc field. Remove duplicate ownership and the unused status accessor.
p = Path("src-tauri/src/plugins/telnet/channel.rs")
text = p.read_text()
text = text.replace("//! 协商/子协商收发）实现内核 `Channel` trait。", "//! 协商/子协商收发）实现 transport `BlockingByteStream`。")
text = text.replace("use std::sync::{Arc, Mutex};\n", "")
text = re.sub(
    r"    /// session_id 槽：I/O 循环启动时经 `on_session_started` 注入，\n    /// 回调据此携带正确的会话标识（槽未注入时仅记录日志，不丢关键状态）。\n    session_id_slot: Arc<Mutex<Option<String>>>,\n",
    "",
    text,
    count=1,
)
text = text.replace(
    "        on_echo_change: Box<dyn Fn(bool) + Send>,\n        session_id_slot: Arc<Mutex<Option<String>>>,\n",
    "        on_echo_change: Box<dyn Fn(bool) + Send>,\n",
)
text = text.replace("            on_echo_change,\n            session_id_slot,\n", "            on_echo_change,\n")
text = re.sub(r"\n    pub\(crate\) fn is_connected\(&self\) -> bool \{\n        self\.connected\n    \}\n", "\n", text, count=1)
p.write_text(text)

p = Path("src-tauri/src/plugins/telnet/mod.rs")
text = p.read_text().replace(
    "let driver = TelnetDriver::new(telnet, probe, on_echo_change, session_id_slot.clone());",
    "let driver = TelnetDriver::new(telnet, probe, on_echo_change);",
)
text = text.replace(
    "// 回显状态 → 前端事件。session_id 由 I/O 循环启动时经\n        // `Channel::on_session_started` 注入槽中；I/O 循环先于任何协商\n        // 事件调用该钩子，故正常流程下事件必带正确标识、永不丢失。",
    "// 回显状态 → 前端事件。session_id 由 SessionAttach 在会话注册完成后注入；\n        // 回调和 attach hook 共享同一个 slot。",
)
p.write_text(text)

# Remove unused serial discovery duplicate; endpoint discovery is owned by the serial plugin.
p = Path("src-tauri/src/transport/serial.rs")
text = p.read_text()
text = re.sub(
    r"\n#\[derive\(Debug, Clone, Serialize\)\]\npub struct SerialEndpoint \{.*?\n\}\n\npub fn discover_serial_endpoints\(\) -> Result<Vec<SerialEndpoint>, TransportError> \{.*?\n\}\n",
    "\n",
    text,
    count=1,
    flags=re.S,
)
text = text.replace(
    "        let mut config = SerialTransportConfig::default();\n        config.data_bits = 9;",
    "        let config = SerialTransportConfig {\n            data_bits: 9,\n            ..Default::default()\n        };",
)
p.write_text(text)

# Remove unused listener accessor.
p = Path("src-tauri/src/transport/tcp.rs")
text = p.read_text()
text = re.sub(
    r"\n    pub fn local_addr\(&self\) -> Result<SocketAddr, TransportError> \{\n        self\.listener\n            \.local_addr\(\)\n            \.map_err\(\|error\| TransportError::io\(\"tcp_listener_local_addr\", error\)\)\n    \}\n",
    "\n",
    text,
    count=1,
)
p.write_text(text)

# Derivable empty capability bundle default.
p = Path("src-tauri/src/kernel/session_store.rs")
text = p.read_text().replace("/// 容器会话创建参数。\npub struct ContainerSessionRuntime {", "/// 容器会话创建参数。\n#[derive(Default)]\npub struct ContainerSessionRuntime {")
text = re.sub(r"\nimpl Default for ContainerSessionRuntime \{.*?\n\}\n\n", "\n", text, count=1, flags=re.S)
p.write_text(text)

# Modbus TCP receive consumes only the first complete frame for the in-flight transaction.
p = Path("src-tauri/src/plugins/modbus/client.rs")
text = p.read_text().replace("                for frame in frames {", "                if let Some(frame) = frames.into_iter().next() {")
text = text.replace(
    "        let mut config = ModbusConfig::default();\n        config.mode = ModbusMode::Tcp;\n        config.read_retries = 1;",
    "        let config = ModbusConfig {\n            mode: ModbusMode::Tcp,\n            read_retries: 1,\n            ..Default::default()\n        };",
)
text = text.replace(
    "        let mut config = ModbusConfig::default();\n        config.mode = ModbusMode::Tcp;\n        config.read_retries = 3;\n        config.retry_writes = false;",
    "        let config = ModbusConfig {\n            mode: ModbusMode::Tcp,\n            read_retries: 3,\n            retry_writes: false,\n            ..Default::default()\n        };",
)
text = text.replace(
    "        let mut config = ModbusConfig::default();\n        config.mode = ModbusMode::Rtu;\n        config.unit_id = 0;",
    "        let config = ModbusConfig {\n            mode: ModbusMode::Rtu,\n            unit_id: 0,\n            ..Default::default()\n        };",
)
p.write_text(text)

# Rust 1.98 idioms for fixed-size chunking.
p = Path("src-tauri/src/plugins/modbus/codec/ascii.rs")
text = p.read_text().replace("if hex.len() % 2 != 0 {", "if !hex.len().is_multiple_of(2) {")
text = text.replace("    for pair in hex.chunks_exact(2) {", "    for pair in hex.as_chunks::<2>().0 {")
p.write_text(text)

p = Path("src-tauri/src/plugins/modbus/codec/pdu.rs")
text = p.read_text().replace("(data.len() - 1) % 7 != 0", "!(data.len() - 1).is_multiple_of(7)")
text = text.replace("for chunk in data[1..].chunks_exact(7) {", "for chunk in data[1..].as_chunks::<7>().0 {")
text = text.replace("        .chunks_exact(2)\n        .map(|chunk|", "        .as_chunks::<2>().0\n        .iter()\n        .map(|chunk|")
p.write_text(text)

# Pattern matching can encode valid function sets directly.
p = Path("src-tauri/src/plugins/modbus/polling.rs")
text = p.read_text()
text = text.replace(
    "                ModbusRequest::ReadBits { function, .. } if matches!(function, 0x01 | 0x02) => {}",
    "                ModbusRequest::ReadBits { function: 0x01 | 0x02, .. } => {}",
)
text = text.replace(
    "                ModbusRequest::ReadRegisters { function, .. }\n                    if matches!(function, 0x03 | 0x04) => {}",
    "                ModbusRequest::ReadRegisters { function: 0x03 | 0x04, .. } => {}",
)
p.write_text(text)

# Rust identifiers follow idiomatic casing while preserving the public JSON contract exactly.
p = Path("src-tauri/src/plugins/modbus/value.rs")
text = p.read_text()
text = text.replace(
    "pub enum ByteOrder {\n    ABCD,\n    BADC,\n    CDAB,\n    DCBA,\n}",
    "pub enum ByteOrder {\n    #[serde(rename = \"ABCD\")]\n    Abcd,\n    #[serde(rename = \"BADC\")]\n    Badc,\n    #[serde(rename = \"CDAB\")]\n    Cdab,\n    #[serde(rename = \"DCBA\")]\n    Dcba,\n}",
)
for old, new in [("ABCD", "Abcd"), ("BADC", "Badc"), ("CDAB", "Cdab"), ("DCBA", "Dcba")]:
    text = text.replace(f"ByteOrder::{old}", f"ByteOrder::{new}")
p.write_text(text)

# ExtendedData pattern includes the stderr stream discriminator directly.
p = Path("src-tauri/src/plugins/ssh/driver.rs")
text = p.read_text().replace(
    "Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {",
    "Some(russh::ChannelMsg::ExtendedData { data, ext: 1 }) => {",
)
p.write_text(text)
