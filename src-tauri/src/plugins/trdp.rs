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
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpCaptureInterface {
    pub name: String,
    pub description: String,
}

pub struct TrdpSideChannel {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    params: Mutex<Value>,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>>,
    next_request_id: AtomicU64,
    alive: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
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
        let request_id = format!(
            "r{}",
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        object.insert(
            "request_id".to_string(),
            Value::String(request_id.clone()),
        );

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
        let pending = Arc::clone(&self.pending);
        let alive = Arc::clone(&self.alive);
        let ready = Arc::clone(&self.ready);
        let shutting_down = Arc::clone(&self.shutting_down);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let mut payload = serde_json::from_str::<Value>(&line)
                    .unwrap_or_else(|_| json!({ "event": "bridge_output", "message": line }));
                let event_name = payload.get("event").and_then(Value::as_str);
                let request_id = payload
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);

                if matches!(event_name, Some("ack") | Some("error")) {
                    if let Some(request_id) = request_id {
                        let waiter = pending
                            .lock()
                            .ok()
                            .and_then(|mut requests| requests.remove(&request_id));
                        if let Some(waiter) = waiter {
                            let result = if event_name == Some("error") {
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
                    object.insert(
                        "session_id".into(),
                        Value::String(event_session_id.clone()),
                    );
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
                    let path = crate::kernel::session_store::SessionStore::sessions_file_path(
                        &event_app,
                    );
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
            let mut object = params
                .as_object()
                .cloned()
                .unwrap_or_default();
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

fn import_workspace(path: &str) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("读取 TRDP Workspace 失败: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("TRDP Workspace JSON 无效: {error}"))?;
    let object = value
        .as_object()
        .ok_or("TRDP Workspace 顶层必须是 JSON object")?;
    if object.get("format").and_then(Value::as_str) != Some("tauterm-trdp-workspace/v1") {
        return Err("不支持的 TRDP Workspace format，期望 tauterm-trdp-workspace/v1".to_string());
    }
    if let Some(objects) = object.get("objects") {
        if !objects.is_array() {
            return Err("TRDP Workspace objects 必须是数组".to_string());
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
) -> Result<Vec<capture::TrdpPacket>, String> {
    capture::trdp_open_capture(path, pd_ports, md_ports)
}

#[tauri::command]
pub fn trdp_save_capture(path: String, packets: Vec<capture::TrdpPacket>) -> Result<(), String> {
    capture::trdp_save_capture(path, packets)
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
