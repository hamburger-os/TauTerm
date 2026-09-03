//! TRDP session support.
//!
//! Node mode delegates all protocol participation to a TCNOpen 3.0.0.0 based helper
//! (`tauterm-trdp-bridge`).  The Rust/Tauri process only owns lifecycle, IPC and UI events;
//! TCNOpen handles PD/MD wire semantics.  Monitor mode can parse pcap/pcapng offline without
//! the native helper.  Live capture is delegated to the same helper, which dynamically loads
//! libpcap/Npcap so TauTerm does not redistribute Npcap.

use crate::commands::ConnectSessionRequest;
use crate::kernel::plugin_adapter::SideChannel;
use crate::kernel::session_store::ContainerSessionCreateOptions;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::any::Any;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

const PD_PORT: u16 = 17224;
const MD_PORT: u16 = 17225;

pub struct TrdpSideChannel {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    session_id: Mutex<String>,
    params: Mutex<Value>,
}

impl TrdpSideChannel {
    fn new(params: Value) -> Self {
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            session_id: Mutex::new(String::new()),
            params: Mutex::new(params),
        }
    }

    fn bridge_candidates(params: &Value) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(path) = params.get("bridge_path").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
            out.push(PathBuf::from(path));
        }
        let exe_name = if cfg!(windows) { "tauterm-trdp-bridge.exe" } else { "tauterm-trdp-bridge" };
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                out.push(dir.join(exe_name));
                out.push(dir.join("binaries").join(exe_name));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            out.push(cwd.join("src-tauri").join("binaries").join(exe_name));
            out.push(cwd.join("binaries").join(exe_name));
        }
        out
    }

    fn start(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        if self.child.lock().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        let params = self.params.lock().map_err(|e| e.to_string())?.clone();
        let bridge = Self::bridge_candidates(&params)
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                "TCNOpen bridge 未安装。先运行 scripts/bootstrap-trdp.ps1（Windows）或 scripts/bootstrap-trdp.sh（Linux/macOS），然后重新连接 TRDP Node。".to_string()
            })?;

        let mut child = Command::new(&bridge)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 TRDP bridge {} 失败: {e}", bridge.display()))?;
        let stdin = child.stdin.take().ok_or("TRDP bridge stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("TRDP bridge stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("TRDP bridge stderr unavailable")?;

        *self.session_id.lock().map_err(|e| e.to_string())? = session_id.to_string();
        *self.stdin.lock().map_err(|e| e.to_string())? = Some(stdin);

        let sid = session_id.to_string();
        let app_events = app.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let mut payload = serde_json::from_str::<Value>(&line)
                    .unwrap_or_else(|_| json!({ "event": "bridge_output", "message": line }));
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("session_id".into(), Value::String(sid.clone()));
                }
                let _ = app_events.emit("trdp-event", payload);
            }
        });

        let sid_err = session_id.to_string();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                log::warn!("TRDP bridge [{}]: {}", sid_err, line);
            }
        });

        *self.child.lock().map_err(|e| e.to_string())? = Some(child);
        self.send(json!({ "command": "open", "params": params }))
    }

    fn send(&self, command: Value) -> Result<(), String> {
        let mut guard = self.stdin.lock().map_err(|e| e.to_string())?;
        let stdin = guard.as_mut().ok_or("TRDP bridge 尚未启动")?;
        serde_json::to_writer(&mut *stdin, &command).map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }
}

impl SideChannel for TrdpSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        if let Ok(mut input) = self.stdin.lock() {
            if let Some(stdin) = input.as_mut() {
                let _ = stdin.write_all(b"{\"command\":\"shutdown\"}\n");
                let _ = stdin.flush();
            }
            input.take();
        }
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut process) = child.take() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }
}

/// Replaces the normal connect command in the Tauri handler. Non-TRDP requests are delegated
/// unchanged to the existing core command, keeping the TRDP integration isolated.
#[tauri::command]
pub async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let plugin_id = request.plugin_id.clone().unwrap_or_else(|| "serial".into());
    if plugin_id != "trdp" {
        return crate::commands::connect_session(app, state, request).await;
    }

    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    let mode = params.get("mode").and_then(Value::as_str).unwrap_or("node");
    if !matches!(mode, "node" | "monitor") {
        return Err(format!("未知 TRDP 会话模式: {mode}"));
    }

    let side = Arc::new(TrdpSideChannel::new(params.clone()));
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: name.unwrap_or_else(|| if mode == "monitor" { "TRDP Monitor".into() } else { "TRDP Node".into() }),
                plugin_id: "trdp".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled.unwrap_or(false),
                transfer_protocol,
                send_bar_enabled: send_bar_enabled.unwrap_or(false),
                id_override: session_id,
            },
            Some(side.clone()),
            None,
            None,
        )?
    };

    if mode == "node" {
        if let Err(error) = side.start(app.clone(), &sid) {
            if let Ok(mut store) = state.session_store.lock() {
                let _ = store.close_session(&sid);
            }
            return Err(error);
        }
    }

    let connected_at = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.get_session(&sid).and_then(|handle| handle.connected_at)
    };
    let _ = app.emit("session-connected", json!({
        "session_id": sid,
        "endpoint": endpoint,
        "connection_type": "trdp",
        "plugin_id": "trdp",
        "name": if mode == "monitor" { "TRDP Monitor" } else { "TRDP Node" },
        "params": params,
        "connected_at": connected_at,
        "transfer_enabled": false,
        "transfer_protocol": Value::Null,
        "send_bar_enabled": false,
    }));
    Ok(sid)
}

fn side_channel(state: &State<'_, AppState>, session_id: &str) -> Result<Arc<dyn SideChannel>, String> {
    let store = state.session_store.lock().map_err(|e| e.to_string())?;
    store.get_side_channel(session_id).ok_or_else(|| "TRDP 会话不存在或已断开".to_string())
}

#[tauri::command]
pub fn trdp_command(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    command: Value,
) -> Result<(), String> {
    let sc = side_channel(&state, &session_id)?;
    let trdp = sc.as_any().downcast_ref::<TrdpSideChannel>().ok_or("会话不是 TRDP 会话")?;
    if trdp.child.lock().map_err(|e| e.to_string())?.is_none() {
        trdp.start(app, &session_id)?;
    }
    trdp.send(command)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpPacket {
    pub event: String,
    pub link: String,
    pub timestamp_us: u64,
    pub src_ip: String,
    pub dest_ip: String,
    pub src_port: u16,
    pub dest_port: u16,
    pub transport: String,
    pub msg_type: String,
    pub com_id: u32,
    pub seq_count: u32,
    pub protocol_version: u16,
    pub etb_topo_count: u32,
    pub op_trn_topo_count: u32,
    pub data_len: u32,
    pub payload_hex: String,
    pub raw_frame_hex: String,
    pub sdt_detected: bool,
}

fn be16(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?]))
}
fn be32(data: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?, *data.get(pos + 2)?, *data.get(pos + 3)?]))
}
fn hex(data: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        out.push(TABLE[(b >> 4) as usize] as char);
        out.push(TABLE[(b & 0xf) as usize] as char);
    }
    out
}
fn ipv4(data: &[u8]) -> String {
    format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3])
}

fn decode_frame(frame: &[u8], linktype: u16, timestamp_us: u64) -> Option<TrdpPacket> {
    let mut ip = match linktype {
        1 => 14usize,   // Ethernet
        113 => 16usize, // Linux cooked v1
        276 => 20usize, // Linux cooked v2
        _ => return None,
    };
    if linktype == 1 {
        let mut ether_type = be16(frame, 12)?;
        if matches!(ether_type, 0x8100 | 0x88a8) {
            ether_type = be16(frame, 16)?;
            ip = 18;
        }
        if ether_type != 0x0800 {
            return None;
        }
    }
    if frame.get(ip)? >> 4 != 4 {
        return None;
    }
    let ihl = ((frame.get(ip)? & 0x0f) as usize) * 4;
    if ihl < 20 || frame.len() < ip + ihl {
        return None;
    }
    let protocol = *frame.get(ip + 9)?;
    let src_ip = ipv4(frame.get(ip + 12..ip + 16)?);
    let dest_ip = ipv4(frame.get(ip + 16..ip + 20)?);
    let l4 = ip + ihl;
    let (transport, src_port, dest_port, payload) = match protocol {
        17 => {
            let src = be16(frame, l4)?;
            let dst = be16(frame, l4 + 2)?;
            ("udp", src, dst, l4 + 8)
        }
        6 => {
            let src = be16(frame, l4)?;
            let dst = be16(frame, l4 + 2)?;
            let hlen = ((*frame.get(l4 + 12)? >> 4) as usize) * 4;
            ("tcp", src, dst, l4 + hlen)
        }
        _ => return None,
    };
    if frame.len() < payload + 24 {
        return None;
    }
    if src_port != PD_PORT && dest_port != PD_PORT && src_port != MD_PORT && dest_port != MD_PORT {
        return None;
    }
    let seq_count = be32(frame, payload)?;
    let protocol_version = be16(frame, payload + 4)?;
    let msg_bytes = frame.get(payload + 6..payload + 8)?;
    if !msg_bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    let msg_type = String::from_utf8_lossy(msg_bytes).to_string();
    let com_id = be32(frame, payload + 8)?;
    let etb = be32(frame, payload + 12)?;
    let op = be32(frame, payload + 16)?;
    let data_len = be32(frame, payload + 20)?;
    let header_len = if msg_type.starts_with('M') { 116usize } else { 40usize };
    let data_start = payload.saturating_add(header_len).min(frame.len());
    let data_end = data_start.saturating_add(data_len as usize).min(frame.len());
    let payload_data = frame.get(data_start..data_end).unwrap_or(&[]);

    Some(TrdpPacket {
        event: "packet".into(),
        link: "capture".into(),
        timestamp_us,
        src_ip,
        dest_ip,
        src_port,
        dest_port,
        transport: transport.into(),
        msg_type,
        com_id,
        seq_count,
        protocol_version,
        etb_topo_count: etb,
        op_trn_topo_count: op,
        data_len,
        payload_hex: hex(payload_data),
        raw_frame_hex: hex(frame),
        sdt_detected: false,
    })
}

fn read_u16(bytes: &[u8], pos: usize, little: bool) -> Option<u16> {
    let raw = [*bytes.get(pos)?, *bytes.get(pos + 1)?];
    Some(if little { u16::from_le_bytes(raw) } else { u16::from_be_bytes(raw) })
}
fn read_u32(bytes: &[u8], pos: usize, little: bool) -> Option<u32> {
    let raw = [*bytes.get(pos)?, *bytes.get(pos + 1)?, *bytes.get(pos + 2)?, *bytes.get(pos + 3)?];
    Some(if little { u32::from_le_bytes(raw) } else { u32::from_be_bytes(raw) })
}

fn parse_pcap(bytes: &[u8]) -> Result<Vec<TrdpPacket>, String> {
    if bytes.len() < 24 {
        return Err("pcap 文件过短".into());
    }
    let (little, nanos) = match &bytes[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        _ => return Err("不支持的 pcap magic".into()),
    };
    let linktype = read_u32(bytes, 20, little).ok_or("pcap header invalid")? as u16;
    let mut pos = 24usize;
    let mut out = Vec::new();
    while pos + 16 <= bytes.len() {
        let sec = read_u32(bytes, pos, little).unwrap_or(0) as u64;
        let frac = read_u32(bytes, pos + 4, little).unwrap_or(0) as u64;
        let caplen = read_u32(bytes, pos + 8, little).unwrap_or(0) as usize;
        pos += 16;
        if pos + caplen > bytes.len() { break; }
        let ts = sec.saturating_mul(1_000_000) + if nanos { frac / 1000 } else { frac };
        if let Some(packet) = decode_frame(&bytes[pos..pos + caplen], linktype, ts) {
            out.push(packet);
        }
        pos += caplen;
    }
    Ok(out)
}

fn parse_pcapng(bytes: &[u8]) -> Result<Vec<TrdpPacket>, String> {
    let mut pos = 0usize;
    let mut little = true;
    let mut interfaces: Vec<u16> = Vec::new();
    let mut out = Vec::new();
    while pos + 12 <= bytes.len() {
        let raw_type = &bytes[pos..pos + 4];
        if raw_type == [0x0a, 0x0d, 0x0d, 0x0a] {
            if pos + 12 > bytes.len() { break; }
            little = match &bytes[pos + 8..pos + 12] {
                [0x4d, 0x3c, 0x2b, 0x1a] => true,
                [0x1a, 0x2b, 0x3c, 0x4d] => false,
                _ => return Err("pcapng byte-order magic invalid".into()),
            };
            interfaces.clear();
        }
        let block_type = read_u32(bytes, pos, little).ok_or("pcapng block invalid")?;
        let total = read_u32(bytes, pos + 4, little).ok_or("pcapng length invalid")? as usize;
        if total < 12 || pos + total > bytes.len() { break; }
        match block_type {
            1 if total >= 20 => {
                interfaces.push(read_u16(bytes, pos + 8, little).unwrap_or(1));
            }
            6 if total >= 32 => {
                let iface = read_u32(bytes, pos + 8, little).unwrap_or(0) as usize;
                let ts_hi = read_u32(bytes, pos + 12, little).unwrap_or(0) as u64;
                let ts_lo = read_u32(bytes, pos + 16, little).unwrap_or(0) as u64;
                let caplen = read_u32(bytes, pos + 20, little).unwrap_or(0) as usize;
                let start = pos + 28;
                if start + caplen <= pos + total {
                    let ts = (ts_hi << 32) | ts_lo; // default pcapng resolution is microseconds
                    let linktype = interfaces.get(iface).copied().unwrap_or(1);
                    if let Some(packet) = decode_frame(&bytes[start..start + caplen], linktype, ts) {
                        out.push(packet);
                    }
                }
            }
            _ => {}
        }
        pos += total;
    }
    Ok(out)
}

#[tauri::command]
pub fn trdp_open_capture(path: String) -> Result<Vec<TrdpPacket>, String> {
    let bytes = fs::read(&path).map_err(|e| format!("读取抓包失败: {e}"))?;
    if bytes.starts_with(&[0x0a, 0x0d, 0x0d, 0x0a]) {
        parse_pcapng(&bytes)
    } else {
        parse_pcap(&bytes)
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) { return None; }
    (0..value.len()).step_by(2).map(|i| u8::from_str_radix(&value[i..i + 2], 16).ok()).collect()
}

fn append_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn append_u64_as_pair(out: &mut Vec<u8>, value: u64) {
    append_u32(out, (value >> 32) as u32);
    append_u32(out, value as u32);
}

#[tauri::command]
pub fn trdp_save_capture(path: String, packets: Vec<TrdpPacket>) -> Result<(), String> {
    let mut out = Vec::new();
    // Section Header Block, little endian.
    append_u32(&mut out, 0x0a0d0d0a); append_u32(&mut out, 28); append_u32(&mut out, 0x1a2b3c4d);
    out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&u64::MAX.to_le_bytes()); append_u32(&mut out, 28);
    // Interface Description Block: Ethernet, snaplen 65535.
    append_u32(&mut out, 1); append_u32(&mut out, 20); out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&0u16.to_le_bytes()); append_u32(&mut out, 65535); append_u32(&mut out, 20);
    for packet in packets {
        let Some(frame) = decode_hex(&packet.raw_frame_hex) else { continue; };
        let padded = (frame.len() + 3) & !3;
        let total = 32 + padded;
        append_u32(&mut out, 6); append_u32(&mut out, total as u32); append_u32(&mut out, 0);
        append_u64_as_pair(&mut out, packet.timestamp_us); append_u32(&mut out, frame.len() as u32); append_u32(&mut out, frame.len() as u32);
        out.extend_from_slice(&frame); out.resize(out.len() + (padded - frame.len()), 0); append_u32(&mut out, total as u32);
    }
    fs::write(Path::new(&path), out).map_err(|e| format!("保存 pcapng 失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_trdp_ports() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 40];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[10, 0, 0, 2]);
        frame[34..36].copy_from_slice(&1000u16.to_be_bytes());
        frame[36..38].copy_from_slice(&1001u16.to_be_bytes());
        assert!(decode_frame(&frame, 1, 0).is_none());
    }

    #[test]
    fn parses_pd_header() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 44];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes()); frame[14] = 0x45; frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]); frame[30..34].copy_from_slice(&[239, 1, 1, 1]);
        frame[34..36].copy_from_slice(&PD_PORT.to_be_bytes()); frame[36..38].copy_from_slice(&PD_PORT.to_be_bytes());
        let p = 42; frame[p..p + 4].copy_from_slice(&7u32.to_be_bytes()); frame[p + 4..p + 6].copy_from_slice(&0x0100u16.to_be_bytes()); frame[p + 6..p + 8].copy_from_slice(b"Pd"); frame[p + 8..p + 12].copy_from_slice(&1001u32.to_be_bytes()); frame[p + 20..p + 24].copy_from_slice(&4u32.to_be_bytes()); frame[p + 40..p + 44].copy_from_slice(&[1, 2, 3, 4]);
        let packet = decode_frame(&frame, 1, 123).expect("packet");
        assert_eq!(packet.com_id, 1001); assert_eq!(packet.seq_count, 7); assert_eq!(packet.payload_hex, "01020304");
    }
}
