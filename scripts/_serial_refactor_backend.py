from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]

def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")

def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")

def exact(path: str, old: str, new: str, count: int = 1) -> None:
    text = read(path)
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} matches, found {actual}: {old[:120]!r}")
    write(path, text.replace(old, new, count))

def sub(path: str, pattern: str, replacement: str, count: int = 1) -> None:
    text = read(path)
    changed, actual = re.subn(pattern, replacement, text, count=count, flags=re.S | re.M)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} regex matches, found {actual}: {pattern[:120]!r}")
    write(path, changed)

# ---------------------------------------------------------------------------
# DataPlane subscriptions: bounded, fail-closed consumers; expose exclusive
# ownership state so bridge workers can pause without consuming external bytes.
# ---------------------------------------------------------------------------
p = "src-tauri/src/transport/runtime.rs"
exact(p, "const STARTUP_BUFFER_LIMIT: usize = 64 * 1024;\n", "const STARTUP_BUFFER_LIMIT: usize = 64 * 1024;\nconst SUBSCRIPTION_CAPACITY: usize = 256;\n")
exact(p, "    receiver: mpsc::Receiver<DataPlaneEvent>,\n", "    receiver: mpsc::Receiver<DataPlaneEvent>,\n")
exact(p, "        let (event_tx, event_rx) = mpsc::channel();\n", "        let (event_tx, event_rx) = mpsc::sync_channel(SUBSCRIPTION_CAPACITY);\n")
exact(p, "        subscriber: mpsc::Sender<DataPlaneEvent>,\n", "        subscriber: mpsc::SyncSender<DataPlaneEvent>,\n")
exact(p, "    subscribers: Vec<(u64, mpsc::Sender<DataPlaneEvent>)>,\n", "    subscribers: Vec<(u64, mpsc::SyncSender<DataPlaneEvent>)>,\n")
exact(p, "    subscribers: &mut Vec<(u64, mpsc::Sender<DataPlaneEvent>)>,\n", "    subscribers: &mut Vec<(u64, mpsc::SyncSender<DataPlaneEvent>)>,\n")
exact(p, """        state
            .subscribers
            .retain(|(_, subscriber)| subscriber.send(DataPlaneEvent::Data(data.clone())).is_ok());
""", """        state.subscribers.retain(|(id, subscriber)| {
            match subscriber.try_send(DataPlaneEvent::Data(data.clone())) {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(_)) => {
                    log::warn!("DataPlane subscriber {} exceeded bounded backlog; detaching consumer", id);
                    false
                }
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
""")
exact(p, """                if subscriber.send(DataPlaneEvent::Data(data)).is_err() {
                    alive = false;
                    break;
                }
""", """                if subscriber.try_send(DataPlaneEvent::Data(data)).is_err() {
                    alive = false;
                    break;
                }
""")
# broadcast_close is later in the file and also owns the sender type / send call.
text = read(p)
text = text.replace("Vec<(u64, mpsc::Sender<DataPlaneEvent>)>", "Vec<(u64, mpsc::SyncSender<DataPlaneEvent>)>")
text = text.replace("subscriber.send(DataPlaneEvent::Closed(info.clone())).is_ok()", "subscriber.try_send(DataPlaneEvent::Closed(info.clone())).is_ok()")
write(p, text)
exact(p, """    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
""", """    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn is_exclusive(&self) -> bool {
        self.exclusive_active.load(Ordering::Acquire)
    }
""")

p = "src-tauri/src/session/io.rs"
exact(p, """    pub fn is_connected(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(DataPlaneHandle::is_connected)
    }
""", """    pub fn is_connected(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(DataPlaneHandle::is_connected)
    }

    pub fn is_exclusive(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(DataPlaneHandle::is_exclusive)
    }
""")

# ---------------------------------------------------------------------------
# Serial transport: typed link fields and runtime-owned read cadence.
# ---------------------------------------------------------------------------
write("src-tauri/src/transport/serial.rs", r'''use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus};

const SERIAL_READ_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialParity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SerialStopBits {
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialFlowControl {
    None,
    RtsCts,
    XonXoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialTransportConfig {
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: SerialParity,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: SerialStopBits,
    #[serde(default = "default_flow_control")]
    pub flow_control: SerialFlowControl,
}

impl Default for SerialTransportConfig {
    fn default() -> Self {
        Self {
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            parity: default_parity(),
            stop_bits: default_stop_bits(),
            flow_control: default_flow_control(),
        }
    }
}

fn default_baud_rate() -> u32 { 115_200 }
fn default_data_bits() -> u8 { 8 }
fn default_parity() -> SerialParity { SerialParity::None }
fn default_stop_bits() -> SerialStopBits { SerialStopBits::One }
fn default_flow_control() -> SerialFlowControl { SerialFlowControl::None }

pub fn open_serial(
    endpoint: &str,
    config: &SerialTransportConfig,
) -> Result<SerialDriver, TransportError> {
    validate_config(config)?;
    let data_bits = match config.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        8 => serialport::DataBits::Eight,
        _ => unreachable!("validated"),
    };
    let parity = match config.parity {
        SerialParity::None => serialport::Parity::None,
        SerialParity::Even => serialport::Parity::Even,
        SerialParity::Odd => serialport::Parity::Odd,
    };
    let stop_bits = match config.stop_bits {
        SerialStopBits::One => serialport::StopBits::One,
        SerialStopBits::Two => serialport::StopBits::Two,
    };
    let flow_control = match config.flow_control {
        SerialFlowControl::None => serialport::FlowControl::None,
        SerialFlowControl::RtsCts => serialport::FlowControl::Hardware,
        SerialFlowControl::XonXoff => serialport::FlowControl::Software,
    };

    // One connect request performs one physical open. Retry/reconnect belongs to the Session layer.
    let port = serialport::new(endpoint, config.baud_rate)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow_control)
        .timeout(SERIAL_READ_TIMEOUT)
        .open()
        .map_err(map_open_error)?;

    // Never purge immediately after open: startup/boot bytes are valid input.
    Ok(SerialDriver { port })
}

fn map_open_error(error: serialport::Error) -> TransportError {
    let kind = match error.kind() {
        serialport::ErrorKind::NoDevice => TransportErrorKind::DeviceNotFound,
        serialport::ErrorKind::InvalidInput => TransportErrorKind::InvalidConfiguration,
        serialport::ErrorKind::Unknown => TransportErrorKind::Connect,
        serialport::ErrorKind::Io(io_kind) => match io_kind {
            std::io::ErrorKind::PermissionDenied => TransportErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound => TransportErrorKind::DeviceNotFound,
            std::io::ErrorKind::TimedOut => TransportErrorKind::Timeout,
            std::io::ErrorKind::WouldBlock => TransportErrorKind::DeviceBusy,
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                TransportErrorKind::ConnectionReset
            }
            _ => TransportErrorKind::Connect,
        },
    };
    TransportError::new(kind, "serial_open", error.to_string())
}

fn validate_config(config: &SerialTransportConfig) -> Result<(), TransportError> {
    if matches!(config.data_bits, 5..=8) && config.baud_rate > 0 {
        Ok(())
    } else {
        Err(TransportError::new(
            TransportErrorKind::InvalidConfiguration,
            "serial_config",
            "invalid serial transport configuration",
        ))
    }
}

pub struct SerialDriver {
    port: Box<dyn serialport::SerialPort>,
}

impl BlockingByteStream for SerialDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        match self.port.read(buf) {
            Ok(0) => Ok(ReadStatus::Idle),
            Ok(n) => Ok(ReadStatus::Data(n)),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(ReadStatus::Idle)
            }
            Err(error) => Err(TransportError::io("serial_read", error)),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.port
            .write_all(data)
            .map_err(|error| TransportError::io("serial_write", error))
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        self.port
            .flush()
            .map_err(|error| TransportError::io("serial_flush", error))
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_defaults_are_stable() {
        let config = SerialTransportConfig::default();
        assert_eq!(config.baud_rate, 115_200);
        assert_eq!(config.data_bits, 8);
        assert_eq!(config.parity, SerialParity::None);
        assert_eq!(config.stop_bits, SerialStopBits::One);
        assert_eq!(config.flow_control, SerialFlowControl::None);
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn typed_link_fields_parse_from_wire_schema() {
        let config: SerialTransportConfig = serde_json::from_value(serde_json::json!({
            "baud_rate": 921600,
            "data_bits": 7,
            "parity": "even",
            "stop_bits": "2",
            "flow_control": "rts_cts"
        })).unwrap();
        assert_eq!(config.parity, SerialParity::Even);
        assert_eq!(config.stop_bits, SerialStopBits::Two);
        assert_eq!(config.flow_control, SerialFlowControl::RtsCts);
    }

    #[test]
    fn invalid_serial_config_is_rejected_before_open() {
        let config = SerialTransportConfig {
            data_bits: 9,
            ..Default::default()
        };
        assert_eq!(
            validate_config(&config).unwrap_err().kind,
            TransportErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn serialport_error_kind_is_mapped_without_parsing_localized_text() {
        let permission = map_open_error(serialport::Error::new(
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "localized message",
        ));
        assert_eq!(permission.kind, TransportErrorKind::PermissionDenied);

        let invalid = map_open_error(serialport::Error::new(
            serialport::ErrorKind::InvalidInput,
            "localized message",
        ));
        assert_eq!(invalid.kind, TransportErrorKind::InvalidConfiguration);
    }
}
''')

# Serial adapter tests now compare enums, not strings.
p = "src-tauri/src/plugins/serial/mod.rs"
exact(p, "use crate::transport::serial::{open_serial, SerialTransportConfig};\n", "use crate::transport::serial::{\n    open_serial, SerialFlowControl, SerialParity, SerialStopBits, SerialTransportConfig,\n};\n")
exact(p, "        assert_eq!(config.parity, \"even\");\n        assert_eq!(config.stop_bits, \"2\");\n        assert_eq!(config.flow_control, \"rts_cts\");\n", "        assert_eq!(config.parity, SerialParity::Even);\n        assert_eq!(config.stop_bits, SerialStopBits::Two);\n        assert_eq!(config.flow_control, SerialFlowControl::RtsCts);\n")

# ---------------------------------------------------------------------------
# Virtual bridge: consume a dedicated DataPlane subscription and confirmed
# SessionIo writes. No intermediate try_send queues, no silent byte drops.
# ---------------------------------------------------------------------------
write("src-tauri/src/virtual_port/bridge.rs", r'''//! Virtual serial endpoint bridge.
//!
//! The physical serial driver remains exclusively owned by DataPlaneRuntime. This bridge is only
//! another shared-mode consumer/producer: physical -> virtual uses its own bounded subscription;
//! virtual -> physical uses confirmed SessionIo writes. When an inline transfer owns the driver,
//! the bridge pauses before reading external bytes so it does not consume data it cannot forward.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serialport::SerialPort;

use crate::session::SessionIo;
use crate::transport::DataPlaneEvent;

const VPORT_READ_TIMEOUT_MS: u64 = 5;

type BridgeErrorHandler = Box<dyn Fn(String) + Send + 'static>;

pub struct VirtualPortBridge {
    cancel_flag: Arc<AtomicBool>,
    bridge_thread: Option<std::thread::JoinHandle<()>>,
}

impl VirtualPortBridge {
    pub fn spawn(
        virtual_port_names: Vec<String>,
        baud_rate: u32,
        io: Arc<SessionIo>,
        on_error: BridgeErrorHandler,
    ) -> Result<Self, String> {
        let subscription = io.subscribe().map_err(|error| error.to_string())?;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();

        let bridge_thread = std::thread::spawn(move || {
            if let Err(error) = bridge_loop(
                virtual_port_names,
                baud_rate,
                subscription,
                io,
                &cancel_clone,
            ) {
                log::error!("Virtual port bridge failed: {}", error);
                on_error(error);
            }
        });

        Ok(Self {
            cancel_flag,
            bridge_thread: Some(bridge_thread),
        })
    }

    pub fn shutdown(mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(thread) = self.bridge_thread.take() {
            let start = std::time::Instant::now();
            loop {
                if thread.is_finished() {
                    if let Err(e) = thread.join() {
                        let msg = if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "unknown panic".into()
                        };
                        log::error!("Bridge thread panic: {}", msg);
                    }
                    break;
                }
                if start.elapsed() > Duration::from_secs(5) {
                    log::error!("Bridge thread did not exit within 5 seconds, abandoning wait");
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

impl Drop for VirtualPortBridge {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

fn open_bridge_endpoint(name: &str, baud_rate: u32) -> Result<Box<dyn SerialPort>, String> {
    #[cfg(not(target_os = "windows"))]
    if let Some(master) = crate::virtual_port::pty::take_master_for_slave(name) {
        log::info!("Native PTY master attached for {}", name);
        return Ok(master);
    }

    serialport::new(name, baud_rate)
        .timeout(Duration::from_millis(VPORT_READ_TIMEOUT_MS))
        .open()
        .map_err(|e| format!("failed to open virtual endpoint {name}: {e}"))
}

fn write_to_virtual_ports(virtual_ports: &mut [Box<dyn SerialPort>], data: &[u8]) {
    for vport in virtual_ports.iter_mut() {
        if vport.write_all(data).is_err() {
            log::trace!("Write to virtual endpoint failed (peer closed)");
        }
    }
    for vport in virtual_ports.iter_mut() {
        let _ = vport.flush();
    }
}

fn bridge_loop(
    virtual_port_names: Vec<String>,
    baud_rate: u32,
    subscription: crate::transport::DataPlaneSubscription,
    io: Arc<SessionIo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut virtual_ports: Vec<Box<dyn SerialPort>> = Vec::new();
    for name in &virtual_port_names {
        match open_bridge_endpoint(name, baud_rate) {
            Ok(port) => {
                virtual_ports.push(port);
                log::info!("Virtual endpoint {} attached to bridge", name);
            }
            Err(error) => log::error!("{}", error),
        }
    }

    if virtual_ports.is_empty() {
        return Err("no virtual endpoints were available for bridging".into());
    }

    let mut read_buf = [0u8; 4096];
    let mut pending_write: Option<Vec<u8>> = None;

    while !cancel.load(Ordering::SeqCst) {
        // Drain a bounded amount of physical data each turn so virtual -> physical traffic is not
        // starved by a continuously busy device.
        for _ in 0..32 {
            match subscription.try_recv() {
                Ok(DataPlaneEvent::Data(data)) => write_to_virtual_ports(&mut virtual_ports, &data),
                Ok(DataPlaneEvent::Closed(_)) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("data-plane bridge subscription detached or overflowed".into());
                }
            }
        }

        // Exclusive X/Y/ZModem transfer owns the same physical driver. Do not read external bytes
        // until shared ownership resumes; this preserves those bytes in the virtual endpoint buffer.
        if io.is_exclusive() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        if let Some(data) = pending_write.take() {
            match io.send(&data) {
                Ok(()) => {}
                Err(_) if io.is_exclusive() => {
                    pending_write = Some(data);
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => return Err(format!("virtual endpoint writeback failed: {error}")),
            }
        }

        for vport in &mut virtual_ports {
            match vport.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    let data = read_buf[..n].to_vec();
                    match io.send(&data) {
                        Ok(()) => {}
                        Err(_) if io.is_exclusive() => {
                            pending_write = Some(data);
                            break;
                        }
                        Err(error) => {
                            return Err(format!("virtual endpoint writeback failed: {error}"));
                        }
                    }
                }
                Ok(_) => {}
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    // External applications can disconnect/reconnect independently.
                }
            }
        }
    }

    log::info!("Bridge thread exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::{Read, Write};

    struct MockPort {
        buffer: Vec<u8>,
        read_pos: usize,
    }

    impl MockPort {
        fn new() -> Self {
            Self { buffer: Vec::new(), read_pos: 0 }
        }
    }

    impl Read for MockPort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let available = self.buffer.len() - self.read_pos;
            if available == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            let n = buf.len().min(available);
            buf[..n].copy_from_slice(&self.buffer[self.read_pos..self.read_pos + n]);
            self.read_pos += n;
            Ok(n)
        }
    }

    impl Write for MockPort {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn physical_bytes_are_forwarded_losslessly_to_virtual_endpoint() {
        let mut physical = MockPort::new();
        let mut virtual_a = MockPort::new();
        let mut buf = [0u8; 256];
        physical.buffer.extend_from_slice(b"HELLO");
        let n = physical.read(&mut buf).unwrap();
        virtual_a.write_all(&buf[..n]).unwrap();
        assert_eq!(&virtual_a.buffer, b"HELLO");
    }
}
''')

# ---------------------------------------------------------------------------
# SessionStore naming cleanup and SavedSession single truth for virtual-port
# settings (they live in params, not duplicated top-level fields).
# ---------------------------------------------------------------------------
p = "src-tauri/src/kernel/session_store.rs"
text = read(p).replace("virtual_external_pathridge", "virtual_port_bridge")
text = text.replace("    pub virtual_port_enabled: bool,\n    pub virtual_port_count: u32,\n", "")
write(p, text)

# ---------------------------------------------------------------------------
# Commands: generic on_data callback no longer knows about virtual ports; bridge
# subscribes to DataPlane directly and writes through SessionIo.
# ---------------------------------------------------------------------------
p = "src-tauri/src/commands.rs"
text = read(p)
text = re.sub(r"// ── 可调参数常量.*?// ── 数据结构", "// ── 数据结构", text, count=1, flags=re.S)
text = re.sub(r"/// BridgeChannel = .*?\n\);\n\n", "", text, count=1, flags=re.S)
text = re.sub(
    r"fn create_on_data_callback\(.*?\n\}\n\n/// 串口会话连接",
    '''fn create_on_data_callback(\n    app: &AppHandle,\n    log_tx: std::sync::mpsc::SyncSender<LogEntry>,\n    data_mode: String,\n    encoding: String,\n) -> Box<dyn Fn(String, Vec<u8>) + Send> {\n    let app_clone = app.clone();\n    let overflow_app = app.clone();\n    let batcher = crate::kernel::data_batcher::DataBatcher::new(move |batched| {\n        let _ = app_clone.emit(\n            "session-data",\n            serde_json::json!({\n                "session_id": batched.session_id,\n                "data_b64": batched.data_b64,\n            }),\n        );\n    });\n\n    Box::new(move |session_id, data| {\n        let data_for_log = data.clone();\n        if let Some(total_dropped) = batcher.push(session_id.clone(), data) {\n            if total_dropped == 1 || total_dropped.is_power_of_two() {\n                let _ = overflow_app.emit(\n                    "session-display-overflow",\n                    serde_json::json!({\n                        "session_id": session_id,\n                        "dropped_chunks": total_dropped,\n                    }),\n                );\n            }\n        }\n        try_send_session_log(\n            &log_tx,\n            DataLogEntry {\n                session_id,\n                direction: DataDirection::RX,\n                data_mode: data_mode.clone(),\n                encoding: encoding.clone(),\n                payload: data_for_log,\n                timestamp: Local::now(),\n            },\n        );\n    })\n}\n\n/// 串口会话连接''',
    text,
    count=1,
    flags=re.S,
)
if text.count("/// 串口会话连接") != 1:
    raise SystemExit("commands.rs: create_on_data_callback replacement failed")
# Remove channel construction and bridge_tx from serial connect.
text = re.sub(
    r"    // 桥接数据通道 .*?let bridge_tx = bridge\.as_ref\(\)\.map\(\|\(tx, _\)\| tx\.clone\(\)\);\n\n",
    "",
    text,
    count=1,
    flags=re.S,
)
text = text.replace("        encoding_for_log,\n        bridge_tx,\n", "        encoding_for_log,\n")
text = text.replace("create_on_data_callback(&app, log_tx, data_mode, encoding, None)", "create_on_data_callback(&app, log_tx, data_mode, encoding)")
text = text.replace("create_on_data_callback(app, log_tx, data_mode, encoding.clone(), None)", "create_on_data_callback(app, log_tx, data_mode, encoding.clone())")
# Replace virtual bridge setup inside successful endpoint creation.
pattern = r'''        if !pairs\.is_empty\(\) \{.*?        \} else \{\n            // 使用真实失败原因'''
replacement = r'''        if !pairs.is_empty() {
            let virtual_port_names: Vec<String> =
                pairs.iter().map(|pair| pair.bridge_path.clone()).collect();
            let virtual_baud_rate = params_clone
                .get("baud_rate")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32)
                .ok_or_else(|| "串口配置缺少有效 baud_rate".to_string())?;
            let io = {
                let store = state.session_store.lock().map_err(|error| error.to_string())?;
                store
                    .get_io_for(&session_id)
                    .ok_or_else(|| "串口会话缺少共享 I/O capability".to_string())?
            };
            let error_app = app.clone();
            let error_session_id = session_id.clone();
            let bridge = VirtualPortBridge::spawn(
                virtual_port_names,
                virtual_baud_rate,
                io,
                Box::new(move |reason| {
                    let _ = error_app.emit(
                        "virtual-port-failed",
                        serde_json::json!({
                            "session_id": error_session_id,
                            "kind": "bridge_failed",
                            "reason": reason,
                        }),
                    );
                }),
            )?;

            {
                let mut store = state.session_store.lock().map_err(|error| error.to_string())?;
                if let Some(handle) = store.get_session_mut(&session_id) {
                    handle.virtual_port_bridge = Some(bridge);
                    handle.virtual_endpoints = pairs.clone();
                }
            }

            let _ = app.emit(
                "virtual-port-created",
                serde_json::json!({
                    "session_id": session_id,
                    "endpoints": &vport_endpoints_json,
                }),
            );
        } else {
            // 使用真实失败原因'''
text2, n = re.subn(pattern, replacement, text, count=1, flags=re.S)
if n != 1:
    raise SystemExit(f"commands.rs: bridge setup replacement count={n}")
text = text2
text = text.replace("    // virtual_enabled=true 时 bridge_rx 被 VirtualPortBridge::spawn() 消费，\n    // virtual_enabled=false 时 bridge Option 在此 drop（通道未创建）。\n    // bridge_tx 仅在 virtual_enabled=true 时存在，每个 on_data 回调检查并跳过 None 情况。\n\n", "")
# SavedSessionInfo / SavedSession no longer duplicate virtual settings.
text = text.replace("    pub virtual_port_enabled: bool,\n    pub virtual_port_count: u32,\n", "")
text = text.replace("            virtual_port_enabled: s.virtual_port_enabled,\n            virtual_port_count: s.virtual_port_count,\n", "")
text = re.sub(r'''        virtual_port_enabled: params\n            \.get\("virtual_port_enabled"\)\n            \.and_then\(\|v\| v\.as_bool\(\)\)\n            \.unwrap_or\(false\),\n        virtual_port_count: params\n            \.get\("virtual_port_count"\)\n            \.and_then\(\|v\| v\.as_u64\(\)\)\n            \.map\(\|v\| v as u32\)\n            \.unwrap_or\(0\),\n''', "", text, count=1)
# Test helper construction at bottom.
text = text.replace("            virtual_port_enabled: false,\n            virtual_port_count: 0,\n", "")
write(p, text)

print("serial backend refactor applied")
