//! TRDP session support.
//!
//! Node mode delegates protocol participation to a TCNOpen 3.0.0.0 based helper
//! (`tauterm-trdp-bridge`). TauTerm owns lifecycle, JSON-lines IPC and UI events;
//! TCNOpen remains responsible for PD/MD wire semantics. Monitor mode can parse
//! pcap/pcapng offline without the helper. Live capture uses the helper and the
//! host's libpcap/Npcap installation, so TauTerm does not redistribute Npcap.

pub mod capture;
pub mod xml;

use crate::commands::ConnectSessionRequest;
use crate::kernel::plugin_adapter::SideChannel;
use crate::kernel::session_store::ContainerSessionCreateOptions;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpCaptureInterface {
    pub name: String,
    pub description: String,
}

type PendingRequest = mpsc::Sender<Result<Value, String>>;
type PendingRequests = Arc<Mutex<HashMap<String, PendingRequest>>>;

pub struct TrdpSideChannel {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    params: Mutex<Value>,
    pending: PendingRequests,
    next_request_id: AtomicU64,
    alive: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
    capture_id: Arc<Mutex<Option<String>>>,
    capture_control: Mutex<()>,
}

impl TrdpSideChannel {
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

    fn new(params: Value) -> Self {
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            params: Mutex::new(params),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_request_id: AtomicU64::new(1),
            alive: Arc::new(AtomicBool::new(false)),
            ready: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            capture_id: Arc::new(Mutex::new(None)),
            capture_control: Mutex::new(()),
        }
    }

    fn bridge_candidates(resource_dir: Option<PathBuf>) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) =
            std::env::var_os("TAUTERM_TRDP_BRIDGE").filter(|value| !value.is_empty())
        {
            candidates.push(PathBuf::from(path));
        }

        let executable = if cfg!(windows) {
            "tauterm-trdp-bridge.exe"
        } else {
            "tauterm-trdp-bridge"
        };
        if let Some(directory) = resource_dir {
            candidates.push(directory.join(executable));
            candidates.push(directory.join("binaries").join(executable));
        }
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(directory) = current_exe.parent() {
                candidates.push(directory.join(executable));
                candidates.push(directory.join("binaries").join(executable));
            }
        }
        #[cfg(debug_assertions)]
        {
            if let Ok(cwd) = std::env::current_dir() {
                candidates.push(cwd.join("src-tauri").join("binaries").join(executable));
                candidates.push(cwd.join("binaries").join(executable));
            }
        }
        candidates
    }

    fn bridge_candidate_is_usable(path: &PathBuf) -> bool {
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }

        if metadata.len() <= 64 {
            if let Ok(bytes) = fs::read(path) {
                if bytes.starts_with(b"placeholder")
                    || bytes.starts_with(b"TAUTERM_TRDP_PLACEHOLDER")
                {
                    return false;
                }
            }
        }
        true
    }

    fn send_line(&self, command: &Value) -> Result<(), String> {
        if !self.alive.load(Ordering::Acquire) {
            return Err("TRDP bridge 未运行".to_string());
        }
        let mut input = self.stdin.lock().map_err(|error| error.to_string())?;
        let stdin = input.as_mut().ok_or("TRDP bridge stdin unavailable")?;
        serde_json::to_writer(&mut *stdin, command).map_err(|error| error.to_string())?;
        stdin.write_all(b"\n").map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())
    }

    fn request(&self, mut command: Value, timeout: Duration) -> Result<Value, String> {
        let object = command
            .as_object_mut()
            .ok_or("TRDP bridge command must be a JSON object")?;
        let request_id = format!("r{}", self.next_request_id.fetch_add(1, Ordering::Relaxed));
        object.insert("request_id".to_string(), Value::String(request_id.clone()));

        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|error| error.to_string())?
            .insert(request_id.clone(), tx);

        if let Err(error) = self.send_line(&command) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&request_id);
            }
            return Err(error);
        }

        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&request_id);
                }
                Err(format!(
                    "TRDP bridge request {request_id} timed out after {} ms",
                    timeout.as_millis()
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("TRDP bridge response channel closed".to_string())
            }
        }
    }

    fn start(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        if self
            .child
            .lock()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return if self.ready.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err("TRDP bridge 正在启动或未就绪".to_string())
            };
        }

        let params = self
            .params
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let resource_dir = app.path().resource_dir().ok();
        let bridge = Self::bridge_candidates(resource_dir)
            .into_iter()
            .find(Self::bridge_candidate_is_usable)
            .ok_or_else(|| {
                "TRDP 原生桥接组件未就绪。npm run tauri dev 会自动构建；也可运行 npm run trdp:build 诊断。".to_string()
            })?;

        let mut child = Command::new(&bridge)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("启动 TRDP bridge {} 失败: {error}", bridge.display()))?;
        let stdin = child.stdin.take().ok_or("TRDP bridge stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("TRDP bridge stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("TRDP bridge stderr unavailable")?;

        *self.stdin.lock().map_err(|error| error.to_string())? = Some(stdin);
        *self.child.lock().map_err(|error| error.to_string())? = Some(child);
        self.shutting_down.store(false, Ordering::Release);
        self.ready.store(false, Ordering::Release);
        self.alive.store(true, Ordering::Release);

        let event_session_id = session_id.to_string();
        let event_app = app.clone();
        let capture_id = Arc::clone(&self.capture_id);
        let pd_ports = vec![params
            .get("pd_port")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(17224)];
        let md_ports = {
            let udp = params
                .get("md_udp_port")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok())
                .unwrap_or(17225);
            let tcp = params
                .get("md_tcp_port")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok())
                .unwrap_or(17225);
            if udp == tcp {
                vec![udp]
            } else {
                vec![udp, tcp]
            }
        };
        let pending = Arc::clone(&self.pending);
        let alive = Arc::clone(&self.alive);
        let ready = Arc::clone(&self.ready);
        let shutting_down = Arc::clone(&self.shutting_down);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut decoder = capture::TrdpStreamDecoder::new(pd_ports, md_ports);
            let mut decoder_capture_id: Option<String> = None;
            let mut last_progress = Instant::now();
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let mut payload = serde_json::from_str::<Value>(&line)
                    .unwrap_or_else(|_| json!({ "event": "bridge_output", "message": line }));
                let event_name = payload
                    .get("event")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let request_id = payload
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);

                if event_name.as_deref() == Some("capture_frame") {
                    let current_capture_id = capture_id.lock().ok().and_then(|value| value.clone());
                    let Some(current_capture_id) = current_capture_id else {
                        continue;
                    };
                    if decoder_capture_id.as_deref() != Some(current_capture_id.as_str()) {
                        decoder.reset();
                        decoder_capture_id = Some(current_capture_id.clone());
                    }
                    let link = payload
                        .get("link")
                        .and_then(Value::as_str)
                        .unwrap_or("capture")
                        .to_string();
                    let link_type = payload
                        .get("link_type")
                        .and_then(Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or(1);
                    let timestamp_us = payload
                        .get("timestamp_us")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    let raw_frame_hex = payload
                        .get("raw_frame_hex")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let Some(raw_frame) = capture::decode_raw_frame_hex(raw_frame_hex) else {
                        continue;
                    };
                    let packets = decoder.feed_frame(&raw_frame, link_type, timestamp_us, &link);
                    let stats = capture::append_live_capture(
                        &current_capture_id,
                        link,
                        timestamp_us,
                        link_type,
                        raw_frame,
                        packets.clone(),
                    );
                    for packet in packets {
                        if let Ok(mut value) = serde_json::to_value(packet) {
                            if let Some(object) = value.as_object_mut() {
                                object.insert(
                                    "session_id".into(),
                                    Value::String(event_session_id.clone()),
                                );
                            }
                            let _ = event_app.emit("trdp-event", value);
                        }
                    }
                    if let Some((frame_count, packet_count, dropped_frames)) = stats {
                        if frame_count == 1 || last_progress.elapsed() >= Duration::from_millis(100)
                        {
                            last_progress = Instant::now();
                            let _ = event_app.emit(
                                "trdp-event",
                                json!({
                                    "event": "capture_progress",
                                    "session_id": event_session_id,
                                    "capture_id": current_capture_id,
                                    "frame_count": frame_count,
                                    "packet_count": packet_count,
                                    "dropped_frames": dropped_frames,
                                }),
                            );
                        }
                    }
                    continue;
                }

                // Native live-capture decoding is deliberately ignored. Raw
                // capture frames are decoded above by the same Rust decoder
                // used for offline pcap/pcapng, keeping one canonical model.
                if event_name.as_deref() == Some("packet")
                    && payload.get("kind").and_then(Value::as_str) == Some("capture")
                {
                    continue;
                }

                if matches!(event_name.as_deref(), Some("ack") | Some("error")) {
                    if let Some(request_id) = request_id {
                        let waiter = pending
                            .lock()
                            .ok()
                            .and_then(|mut requests| requests.remove(&request_id));
                        if let Some(waiter) = waiter {
                            let result = if event_name.as_deref() == Some("error") {
                                Err(payload
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .unwrap_or("TRDP bridge operation failed")
                                    .to_string())
                            } else {
                                Ok(payload)
                            };
                            let _ = waiter.send(result);
                            continue;
                        }
                    }
                }

                if let Some(object) = payload.as_object_mut() {
                    object.insert("session_id".into(), Value::String(event_session_id.clone()));
                }
                let _ = event_app.emit("trdp-event", payload);
            }

            alive.store(false, Ordering::Release);
            if let Ok(mut requests) = pending.lock() {
                for (_, waiter) in requests.drain() {
                    let _ = waiter.send(Err("TRDP bridge exited before replying".to_string()));
                }
            }

            let was_ready = ready.swap(false, Ordering::AcqRel);
            if was_ready && !shutting_down.load(Ordering::Acquire) {
                let state: State<'_, AppState> = event_app.state();
                if let Ok(mut store) = state.session_store.lock() {
                    store.mark_disconnected(&event_session_id);
                    let path =
                        crate::kernel::session_store::SessionStore::sessions_file_path(&event_app);
                    let _ = store.save_to_disk(&path);
                }
                let _ = event_app.emit(
                    "session-disconnected",
                    json!({
                        "session_id": event_session_id,
                        "reason": "TRDP native runtime exited unexpectedly"
                    }),
                );
            }
        });

        let error_session_id = session_id.to_string();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                log::warn!("TRDP bridge [{}]: {}", error_session_id, line);
            }
        });

        let open_command = if params.get("mode").and_then(Value::as_str) == Some("monitor") {
            json!({ "command": "monitor_open" })
        } else {
            let mut object = params.as_object().cloned().unwrap_or_default();
            object.insert("command".into(), Value::String("open".into()));
            Value::Object(object)
        };

        match self.request(open_command, Self::REQUEST_TIMEOUT) {
            Ok(_) => {
                self.ready.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                <Self as SideChannel>::shutdown(self);
                Err(error)
            }
        }
    }
}

impl SideChannel for TrdpSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        self.ready.store(false, Ordering::Release);

        if self.alive.load(Ordering::Acquire) {
            let _ = self.send_line(&json!({ "command": "shutdown" }));
        }
        if let Ok(mut input) = self.stdin.lock() {
            input.take();
        }

        if let Ok(mut child) = self.child.lock() {
            if let Some(mut process) = child.take() {
                let mut exited = false;
                for _ in 0..25 {
                    match process.try_wait() {
                        Ok(Some(_)) => {
                            exited = true;
                            break;
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                        Err(_) => break,
                    }
                }
                if !exited {
                    let _ = process.kill();
                    let _ = process.wait();
                }
            }
        }
        self.alive.store(false, Ordering::Release);
        if let Ok(mut requests) = self.pending.lock() {
            for (_, waiter) in requests.drain() {
                let _ = waiter.send(Err("TRDP bridge stopped".to_string()));
            }
        }
    }
}

/// Single connection router exposed to the frontend as `connect_session`.
/// Non-TRDP requests delegate to the existing microkernel command; TRDP sessions
/// use the container/side-channel runtime below. The Rust function name remains
/// unique so Tauri's generated command symbols do not collide across modules.
#[tauri::command(rename = "connect_session")]
pub async fn connect_session_trdp(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let plugin_id = request
        .plugin_id
        .clone()
        .unwrap_or_else(|| "serial".to_string());
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

    let side_channel = Arc::new(TrdpSideChannel::new(params.clone()));
    let session_name = name.unwrap_or_else(|| {
        if mode == "monitor" {
            "TRDP @ Monitor".to_string()
        } else {
            "TRDP @ Node".to_string()
        }
    });
    let session_id = {
        let mut store = state
            .session_store
            .lock()
            .map_err(|error| error.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "trdp".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled.unwrap_or(false),
                transfer_protocol,
                send_bar_enabled: send_bar_enabled.unwrap_or(false),
                id_override: session_id,
            },
            Some(side_channel.clone()),
            None,
            None,
        )?
    };

    if mode == "node" {
        if let Err(error) = side_channel.start(app.clone(), &session_id) {
            if let Ok(mut store) = state.session_store.lock() {
                let _ = store.close_session(&session_id);
            }
            return Err(error);
        }
    }

    let connected_at = {
        let store = state
            .session_store
            .lock()
            .map_err(|error| error.to_string())?;
        store
            .get_session(&session_id)
            .and_then(|handle| handle.connected_at)
    };
    let _ = app.emit(
        "session-connected",
        json!({
            "session_id": session_id,
            "endpoint": endpoint,
            "connection_type": "trdp",
            "plugin_id": "trdp",
            "name": session_name,
            "params": params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "transfer_protocol": Value::Null,
            "send_bar_enabled": false,
        }),
    );
    Ok(session_id)
}

fn side_channel(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<Arc<dyn SideChannel>, String> {
    let store = state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?;
    store
        .get_side_channel(session_id)
        .ok_or_else(|| "TRDP 会话不存在或已断开".to_string())
}

fn validate_workspace_object(value: &Value, index: usize) -> Result<(), String> {
    const ALLOWED_KEYS: &[&str] = &[
        "id",
        "kind",
        "name",
        "com_id",
        "link",
        "destination",
        "source",
        "cycle_us",
        "timeout_mode",
        "timeout_us",
        "timeout_behavior",
        "payload_hex",
        "transport",
        "etb_topo_count",
        "op_trn_topo_count",
        "red_id",
        "num_replies",
        "reply_timeout_us",
        "response_mode",
        "confirm_timeout_us",
        "reply_com_id",
        "reply_ip",
        "source_uri",
        "dest_uri",
    ];
    const U32_FIELDS: &[&str] = &[
        "com_id",
        "cycle_us",
        "timeout_us",
        "etb_topo_count",
        "op_trn_topo_count",
        "red_id",
        "num_replies",
        "reply_timeout_us",
        "confirm_timeout_us",
        "reply_com_id",
    ];

    let object = value
        .as_object()
        .ok_or_else(|| format!("TRDP Workspace objects[{index}] 必须是 object"))?;
    for key in object.keys() {
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "TRDP Workspace objects[{index}] 包含不支持字段 {key}"
            ));
        }
    }

    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("TRDP Workspace objects[{index}] 缺少 id"))?;
    if id.trim().is_empty() {
        return Err(format!("TRDP Workspace objects[{index}].id 不能为空"));
    }

    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("TRDP Workspace objects[{index}] 缺少 kind"))?;
    if !matches!(
        kind,
        "pd_publisher"
            | "pd_subscriber"
            | "pd_request"
            | "md_request"
            | "md_listener"
            | "md_notify"
    ) {
        return Err(format!("TRDP Workspace objects[{index}] kind 无效: {kind}"));
    }

    for field in U32_FIELDS {
        if let Some(raw) = object.get(*field) {
            let Some(number) = raw.as_u64() else {
                return Err(format!(
                    "TRDP Workspace objects[{index}].{field} 必须是无符号整数"
                ));
            };
            if number > u64::from(u32::MAX) {
                return Err(format!(
                    "TRDP Workspace objects[{index}].{field} 超出 u32 范围"
                ));
            }
        }
    }
    if object.get("com_id").and_then(Value::as_u64).unwrap_or(0) == 0 {
        return Err(format!(
            "TRDP Workspace objects[{index}].com_id 必须为非零整数"
        ));
    }

    let validate_enum = |field: &str, accepted: &[&str]| -> Result<(), String> {
        if let Some(raw) = object.get(field) {
            let value = raw
                .as_str()
                .ok_or_else(|| format!("TRDP Workspace objects[{index}].{field} 必须是字符串"))?;
            if !accepted.contains(&value) {
                return Err(format!(
                    "TRDP Workspace objects[{index}].{field} 无效: {value}"
                ));
            }
        }
        Ok(())
    };
    validate_enum("link", &["a", "b", "both"])?;
    validate_enum("timeout_mode", &["auto", "custom", "disabled"])?;
    validate_enum("timeout_behavior", &["keep", "zero"])?;
    validate_enum("transport", &["udp", "tcp"])?;
    validate_enum("response_mode", &["reply", "query"])?;

    if let Some(payload) = object.get("payload_hex").and_then(Value::as_str) {
        if payload.len() > 131_072
            || !payload.len().is_multiple_of(2)
            || !payload.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!(
                "TRDP Workspace objects[{index}].payload_hex 不是有效的受支持 HEX payload"
            ));
        }
    }

    for field in ["source", "destination", "reply_ip"] {
        if let Some(text) = object.get(field).and_then(Value::as_str) {
            if text.parse::<std::net::Ipv4Addr>().is_err() {
                return Err(format!(
                    "TRDP Workspace objects[{index}].{field} 不是有效 IPv4 地址"
                ));
            }
        }
    }
    Ok(())
}

const WORKSPACE_TOP_LEVEL_KEYS: &[&str] =
    &["format", "name", "xml", "objects", "redundancy_groups"];

fn validate_workspace_value(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("TRDP Workspace 顶层必须是 JSON object")?;
    for key in object.keys() {
        if !WORKSPACE_TOP_LEVEL_KEYS.contains(&key.as_str()) {
            return Err(format!("TRDP Workspace 包含不支持字段 {key}"));
        }
    }
    if object.get("format").and_then(Value::as_str) != Some("tauterm-trdp-workspace/v2") {
        return Err(
            "不支持的 TRDP Workspace format，当前仅接受 tauterm-trdp-workspace/v2".to_string(),
        );
    }

    let objects = object
        .get("objects")
        .and_then(Value::as_array)
        .ok_or("TRDP Workspace objects 必须是数组")?;
    let mut object_ids = HashSet::new();
    let mut referenced_redundancy_groups = HashSet::new();
    for (index, item) in objects.iter().enumerate() {
        validate_workspace_object(item, index)?;
        let item_object = item
            .as_object()
            .ok_or_else(|| format!("TRDP Workspace objects[{index}] 必须是 object"))?;
        let id = item_object
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("TRDP Workspace objects[{index}] 缺少 id"))?;
        if !object_ids.insert(id.to_string()) {
            return Err(format!("TRDP Workspace object id 重复: {id}"));
        }
        if let Some(red_id) = item_object
            .get("red_id")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
        {
            referenced_redundancy_groups.insert(red_id as u32);
        }
    }

    let mut redundancy_group_ids = HashSet::new();
    if let Some(groups) = object.get("redundancy_groups") {
        let groups = groups
            .as_object()
            .ok_or("TRDP Workspace redundancy_groups 必须是 object")?;
        for (red_id, state) in groups {
            let parsed = red_id
                .parse::<u32>()
                .map_err(|_| format!("TRDP redundancy group id 无效: {red_id}"))?;
            if parsed == 0 {
                return Err("TRDP redundancy group id 不能为 0".to_string());
            }
            if !matches!(state.as_str(), Some("leader") | Some("follower")) {
                return Err(format!(
                    "TRDP redundancy group {red_id} 状态必须为 leader 或 follower"
                ));
            }
            redundancy_group_ids.insert(parsed);
        }
    }
    for red_id in referenced_redundancy_groups {
        if !redundancy_group_ids.contains(&red_id) {
            return Err(format!(
                "TRDP object 引用了未定义的 redundancy group {red_id}"
            ));
        }
    }

    if let Some(xml) = object.get("xml") {
        let xml = xml.as_str().ok_or("TRDP Workspace xml 必须是路径字符串")?;
        if xml.trim().is_empty() {
            return Err("TRDP Workspace xml 不能为空路径".to_string());
        }
    }
    Ok(())
}

fn import_workspace(path: &str) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("读取 TRDP Workspace 失败: {error}"))?;
    let mut value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("TRDP Workspace JSON 无效: {error}"))?;
    validate_workspace_value(&value)?;

    if let Some(xml) = value.get("xml").and_then(Value::as_str).map(str::to_owned) {
        let workspace_path = std::path::Path::new(path);
        let xml_path = if std::path::Path::new(&xml).is_absolute() {
            PathBuf::from(&xml)
        } else {
            workspace_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(&xml)
        };
        if !xml_path.is_file() {
            return Err(format!(
                "TRDP Workspace 引用的 XML 不存在: {}",
                xml_path.display()
            ));
        }
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "xml_path".to_string(),
                Value::String(xml_path.to_string_lossy().into_owned()),
            );
        }
    }

    Ok(value)
}

/// Plugin-scoped command gateway. File-only operations are handled in Rust and
/// return structured results directly. Active protocol operations are forwarded
/// to the TCNOpen helper. Keeping this as one Tauri command avoids expanding the
/// global command registry for every TRDP operation.
#[tauri::command]
pub fn trdp_command(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    command: Value,
) -> Result<Value, String> {
    match command.get("command").and_then(Value::as_str) {
        Some("xml_import") => {
            let path = command
                .get("path")
                .and_then(Value::as_str)
                .ok_or("xml_import requires path")?;
            let imported = xml::trdp_import_xml(path.to_string())?;
            return serde_json::to_value(imported).map_err(|error| error.to_string());
        }
        Some("workspace_import") => {
            let path = command
                .get("path")
                .and_then(Value::as_str)
                .ok_or("workspace_import requires path")?;
            return import_workspace(path);
        }
        Some("workspace_get") => {
            let store = state
                .session_store
                .lock()
                .map_err(|error| error.to_string())?;
            let handle = store.get_session(&session_id).ok_or("TRDP 会话不存在")?;
            return Ok(handle
                .params
                .get("trdp_workspace")
                .cloned()
                .unwrap_or(Value::Null));
        }
        Some("workspace_store") => {
            let workspace = command
                .get("workspace")
                .cloned()
                .ok_or("workspace_store requires workspace")?;
            validate_workspace_value(&workspace)?;
            {
                let mut store = state
                    .session_store
                    .lock()
                    .map_err(|error| error.to_string())?;
                let handle = store
                    .get_session_mut(&session_id)
                    .ok_or("TRDP 会话不存在")?;
                let params = handle
                    .params
                    .as_object_mut()
                    .ok_or("TRDP 会话参数不是 JSON object")?;
                params.insert("trdp_workspace".to_string(), workspace);
                let path = crate::kernel::session_store::SessionStore::sessions_file_path(&app);
                store.save_to_disk(&path)?;
            }
            return Ok(json!({ "stored": true }));
        }
        Some("dataset_decode") => {
            let path = command
                .get("path")
                .and_then(Value::as_str)
                .ok_or("dataset_decode requires path")?;
            let dataset_id = command
                .get("dataset_id")
                .and_then(Value::as_u64)
                .ok_or("dataset_decode requires dataset_id")? as u32;
            let payload_hex = command
                .get("payload_hex")
                .and_then(Value::as_str)
                .ok_or("dataset_decode requires payload_hex")?;
            return xml::trdp_decode_dataset(path.to_string(), dataset_id, payload_hex.to_string());
        }
        Some("dataset_encode") => {
            let path = command
                .get("path")
                .and_then(Value::as_str)
                .ok_or("dataset_encode requires path")?;
            let dataset_id = command
                .get("dataset_id")
                .and_then(Value::as_u64)
                .ok_or("dataset_encode requires dataset_id")? as u32;
            let values = command
                .get("values")
                .cloned()
                .ok_or("dataset_encode requires values")?;
            return xml::trdp_encode_dataset(path.to_string(), dataset_id, values);
        }
        _ => {}
    }

    let side_channel = side_channel(&state, &session_id)?;
    let trdp = side_channel
        .as_any()
        .downcast_ref::<TrdpSideChannel>()
        .ok_or("会话不是 TRDP 会话")?;
    if trdp
        .child
        .lock()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        trdp.start(app, &session_id)?;
    }
    let operation = command
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if operation == "capture_start" {
        let _control = trdp
            .capture_control
            .lock()
            .map_err(|error| error.to_string())?;
        let previous_capture = trdp
            .capture_id
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let new_capture = capture::create_live_capture();
        *trdp.capture_id.lock().map_err(|error| error.to_string())? = Some(new_capture.clone());

        match trdp.request(command, TrdpSideChannel::REQUEST_TIMEOUT) {
            Ok(_) => {
                if let Some(previous_capture) = previous_capture {
                    capture::release_capture(&previous_capture);
                }
                return Ok(json!({ "capture_id": new_capture }));
            }
            Err(error) => {
                capture::release_capture(&new_capture);
                *trdp
                    .capture_id
                    .lock()
                    .map_err(|lock_error| lock_error.to_string())? = previous_capture;
                return Err(error);
            }
        }
    }

    if operation == "capture_stop" {
        let _control = trdp
            .capture_control
            .lock()
            .map_err(|error| error.to_string())?;
        return trdp.request(command, TrdpSideChannel::REQUEST_TIMEOUT);
    }

    trdp.request(command, TrdpSideChannel::REQUEST_TIMEOUT)
}

#[tauri::command]
pub fn trdp_capture_interfaces(app: AppHandle) -> Result<Vec<TrdpCaptureInterface>, String> {
    let resource_dir = app.path().resource_dir().ok();
    let bridge = TrdpSideChannel::bridge_candidates(resource_dir)
        .into_iter()
        .find(TrdpSideChannel::bridge_candidate_is_usable)
        .ok_or_else(|| {
            "TRDP 原生桥接组件未就绪。`npm run tauri dev` 会自动构建该组件；如果开发启动失败，请确认 CMake 3.20+ 与 Windows C++ 构建工具链可用。也可单独运行 `npm run trdp:build` 诊断原生构建。".to_string()
        })?;

    let mut child = Command::new(&bridge)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 TRDP bridge {} 失败: {error}", bridge.display()))?;

    {
        let mut input = child.stdin.take().ok_or("TRDP bridge stdin unavailable")?;
        input
            .write_all(b"{\"command\":\"capture_list\"}\n{\"command\":\"shutdown\"}\n")
            .map_err(|error| error.to_string())?;
        input.flush().map_err(|error| error.to_string())?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or("TRDP bridge stdout unavailable")?;
    let reader = BufReader::new(stdout);
    let mut interfaces: Option<Vec<TrdpCaptureInterface>> = None;
    let mut bridge_error: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("event").and_then(Value::as_str) {
            Some("capture_interfaces") => {
                interfaces = Some(
                    serde_json::from_value(
                        value
                            .get("interfaces")
                            .cloned()
                            .unwrap_or_else(|| json!([])),
                    )
                    .map_err(|error| error.to_string())?,
                );
            }
            Some("error") => {
                bridge_error = value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            _ => {}
        }
    }
    let _ = child.wait();

    if let Some(error) = bridge_error {
        return Err(error);
    }
    interfaces.ok_or_else(|| "TRDP bridge 未返回抓包接口列表".to_string())
}

#[tauri::command]
pub fn trdp_open_capture(
    path: String,
    pd_ports: Option<Vec<u16>>,
    md_ports: Option<Vec<u16>>,
) -> Result<capture::TrdpCaptureResult, String> {
    capture::trdp_open_capture(path, pd_ports, md_ports)
}

#[tauri::command]
pub fn trdp_capture_packets(
    capture_id: String,
    offset: usize,
    limit: usize,
) -> Result<Vec<capture::TrdpPacket>, String> {
    capture::capture_packets(&capture_id, offset, limit)
}

#[tauri::command]
pub fn trdp_save_capture(path: String, capture_id: String) -> Result<(), String> {
    capture::trdp_save_capture(path, capture_id)
}

#[tauri::command]
pub fn trdp_release_capture(capture_id: String) {
    capture::release_capture(&capture_id);
}

#[tauri::command]
pub fn trdp_import_xml(path: String) -> Result<xml::TrdpXmlImport, String> {
    xml::trdp_import_xml(path)
}

#[tauri::command]
pub fn trdp_decode_dataset(
    path: String,
    dataset_id: u32,
    payload_hex: String,
) -> Result<Value, String> {
    xml::trdp_decode_dataset(path, dataset_id, payload_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn workspace_v2_rejects_unknown_fields() {
        let value = json!({
            "kind": "pd_publisher",
            "com_id": 1001,
            "destination": "239.1.1.1",
            "legacy_flag": true
        });
        let error = validate_workspace_object(&value, 0).expect_err("unknown field must fail");
        assert!(error.contains("legacy_flag"));
    }

    #[test]
    fn workspace_v2_rejects_duplicate_ids_and_missing_redundancy_groups() {
        let duplicate_ids = json!({
            "format": "tauterm-trdp-workspace/v2",
            "objects": [
                {"id":"same","kind":"pd_subscriber","com_id":1},
                {"id":"same","kind":"pd_subscriber","com_id":2}
            ]
        });
        assert!(validate_workspace_value(&duplicate_ids)
            .expect_err("duplicate ids must fail")
            .contains("重复"));

        let missing_group = json!({
            "format": "tauterm-trdp-workspace/v2",
            "objects": [
                {"id":"publisher-1","kind":"pd_publisher","com_id":1,"red_id":7}
            ]
        });
        assert!(validate_workspace_value(&missing_group)
            .expect_err("missing redundancy group must fail")
            .contains("redundancy group 7"));
    }

    #[test]
    fn workspace_import_is_v2_only_and_resolves_relative_xml() {
        let directory = tempfile::tempdir().expect("tempdir");
        let xml_path = directory.path().join("node.xml");
        fs::write(&xml_path, "<device />").expect("xml");

        let workspace_path = directory.path().join("workspace.json");
        let mut file = fs::File::create(&workspace_path).expect("workspace file");
        write!(
            file,
            "{}",
            json!({
                "format": "tauterm-trdp-workspace/v2",
                "xml": "node.xml",
                "objects": [{
                    "id": "subscriber-1",
                    "kind": "pd_subscriber",
                    "com_id": 1001,
                    "destination": "239.1.1.1",
                    "timeout_mode": "auto"
                }],
                "redundancy_groups": {
                    "7": "leader"
                }
            })
        )
        .expect("workspace");

        let imported = import_workspace(&workspace_path.to_string_lossy()).expect("import v2");
        let expected_xml = xml_path.to_string_lossy().into_owned();
        assert_eq!(
            imported.get("xml_path").and_then(Value::as_str),
            Some(expected_xml.as_str())
        );

        fs::write(
            &workspace_path,
            r#"{"format":"tauterm-trdp-workspace/v1","objects":[]}"#,
        )
        .expect("legacy workspace");
        assert!(import_workspace(&workspace_path.to_string_lossy()).is_err());
    }
}
