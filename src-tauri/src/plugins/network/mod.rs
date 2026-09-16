//! Network debugging protocol plugin.
//!
//! TCP peers own independent DataPlane runtimes. The root network session owns a lightweight
//! aggregate DataPlane used by scripts/auto-reply and by the common send path. Peer identity,
//! addressing and Network-specific events remain inside this plugin; SessionStore only owns the
//! generic child DataPlane/SessionIo lifecycle.

pub const PLUGIN_ID: &str = "network";

pub(crate) mod commands;

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::kernel::data_batcher::{base64_encode, DataBatcher};
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogEntry};
use crate::kernel::plugin_adapter::{
    ProtocolAdapter, ProtocolConnection, SessionAttach, SessionService,
};
use crate::kernel::plugin_runtime::SessionRuntimeRegistry;
use crate::kernel::session_store::{SessionState, SessionStore, SubConnection};
use crate::session::{DisconnectInfo, SessionDataPlane, SessionError, SessionIo};
use crate::transport::tcp::{connect_tcp, TcpConnectConfig, TcpDriver, TcpListenerTransport};
use crate::transport::udp::{resolve_udp, UdpTransport};
use crate::transport::{
    BlockingByteStream, DataPlaneHandle, DataPlaneRuntime, ReadStatus, TransportError,
    TransportErrorKind,
};

#[derive(Debug, Clone, Serialize)]
pub struct NetworkPeerInfo {
    pub peer_id: String,
    pub name: String,
    pub addr: String,
    pub local_addr: String,
    pub state: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connected_at: Option<u64>,
}

struct PeerRuntime {
    index: u32,
    name: String,
    addr: String,
    local_addr: String,
    io: Arc<SessionIo>,
    connected_at: Option<u64>,
    connected: bool,
}

struct NetworkCore {
    transport: String,
    role: String,
    send_target: Mutex<Option<String>>,
    peer_handles: Arc<Mutex<HashMap<String, DataPlaneHandle>>>,
    peers: Arc<Mutex<HashMap<String, PeerRuntime>>>,
    next_peer_index: AtomicU32,
    udp_socket: Mutex<Option<Arc<UdpTransport>>>,
    udp_client_target: Mutex<Option<SocketAddr>>,
}

impl NetworkCore {
    fn new(transport: String, role: String) -> Self {
        Self {
            transport,
            role,
            send_target: Mutex::new(None),
            peer_handles: Arc::new(Mutex::new(HashMap::new())),
            peers: Arc::new(Mutex::new(HashMap::new())),
            next_peer_index: AtomicU32::new(0),
            udp_socket: Mutex::new(None),
            udp_client_target: Mutex::new(None),
        }
    }

    fn set_target(&self, target: Option<String>) {
        if let Ok(mut slot) = self.send_target.lock() {
            *slot = target;
        }
    }

    fn current_target(&self) -> Option<String> {
        self.send_target.lock().ok().and_then(|slot| slot.clone())
    }

    fn active_peer_count(&self) -> usize {
        self.peer_handles
            .lock()
            .map(|peers| peers.len())
            .unwrap_or(0)
    }

    fn list_peers(&self) -> Vec<NetworkPeerInfo> {
        let Ok(peers) = self.peers.lock() else {
            return Vec::new();
        };
        let mut peers = peers
            .iter()
            .map(|(peer_id, peer)| {
                (
                    peer.index,
                    NetworkPeerInfo {
                        peer_id: peer_id.clone(),
                        name: peer.name.clone(),
                        addr: peer.addr.clone(),
                        local_addr: peer.local_addr.clone(),
                        state: if peer.connected {
                            "connected".to_string()
                        } else {
                            "disconnected".to_string()
                        },
                        tx_bytes: peer.io.tx_bytes(),
                        rx_bytes: peer.io.rx_bytes(),
                        connected_at: peer.connected_at,
                    },
                )
            })
            .collect::<Vec<_>>();
        peers.sort_by_key(|(index, _)| *index);
        peers.into_iter().map(|(_, peer)| peer).collect()
    }

    fn mark_peer_disconnected(&self, peer_id: &str) {
        if let Ok(mut handles) = self.peer_handles.lock() {
            handles.remove(peer_id);
        }
        if let Ok(mut peers) = self.peers.lock() {
            if let Some(peer) = peers.get_mut(peer_id) {
                peer.connected = false;
            }
        }
        if let Ok(mut target) = self.send_target.lock() {
            if target.as_deref() == Some(peer_id) {
                *target = None;
            }
        }
    }

    fn remove_peer(&self, peer_id: &str) {
        self.mark_peer_disconnected(peer_id);
        if let Ok(mut peers) = self.peers.lock() {
            peers.remove(peer_id);
        }
    }

    fn udp_send_to(&self, target: &str, data: &[u8]) -> Result<(), String> {
        let socket = self
            .udp_socket
            .lock()
            .map_err(|error| error.to_string())?
            .clone()
            .ok_or_else(|| "UDP socket 不可用".to_string())?;
        let address: SocketAddr = target
            .parse()
            .map_err(|error| format!("无效的目标地址 {target}: {error}"))?;
        socket
            .send_to(data, address)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn udp_send(&self, data: &[u8]) -> Result<(), String> {
        let target = self
            .udp_client_target
            .lock()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "UDP 客户端目标未设置".to_string())?;
        self.udp_send_to(&target.to_string(), data)
    }

    fn send(&self, data: &[u8]) -> Result<(), TransportError> {
        match (self.transport.as_str(), self.role.as_str()) {
            ("udp", "server") => {
                let target = self.current_target().ok_or_else(|| {
                    TransportError::new(
                        TransportErrorKind::InvalidConfiguration,
                        "network_send",
                        "无可用发送目标",
                    )
                })?;
                self.udp_send_to(&target, data).map_err(|reason| {
                    TransportError::new(TransportErrorKind::Io, "network_udp_send", reason)
                })
            }
            ("udp", "client") => self.udp_send(data).map_err(|reason| {
                TransportError::new(TransportErrorKind::Io, "network_udp_send", reason)
            }),
            ("tcp", _) => {
                let target = self.current_target();
                let peers = self
                    .peer_handles
                    .lock()
                    .map_err(|error| {
                        TransportError::new(
                            TransportErrorKind::Io,
                            "network_peer_registry",
                            error.to_string(),
                        )
                    })?
                    .iter()
                    .map(|(id, handle)| (id.clone(), handle.clone()))
                    .collect::<Vec<_>>();

                match target.as_deref() {
                    Some("__all__") => {
                        let mut first_error = None;
                        for (_, handle) in peers {
                            if let Err(error) = handle.write(data) {
                                first_error.get_or_insert(error);
                            }
                        }
                        first_error.map_or(Ok(()), Err)
                    }
                    Some(peer_id) => peers
                        .iter()
                        .find(|(id, _)| id == peer_id)
                        .ok_or_else(|| {
                            TransportError::new(
                                TransportErrorKind::RemoteClosed,
                                "network_send",
                                "目标对端不存在或已断开",
                            )
                        })?
                        .1
                        .write(data),
                    None if peers.len() == 1 => peers[0].1.write(data),
                    None => Err(TransportError::new(
                        TransportErrorKind::InvalidConfiguration,
                        "network_send",
                        "无可用发送目标",
                    )),
                }
            }
            _ => Err(TransportError::new(
                TransportErrorKind::Unsupported,
                "network_send",
                "不支持的传输类型",
            )),
        }
    }
}

struct NetworkMuxDriver {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: VecDeque<u8>,
    core: Arc<NetworkCore>,
    running: Arc<AtomicBool>,
}

impl NetworkMuxDriver {
    fn new(rx: mpsc::Receiver<Vec<u8>>, core: Arc<NetworkCore>, running: Arc<AtomicBool>) -> Self {
        Self {
            rx,
            pending: VecDeque::new(),
            core,
            running,
        }
    }

    fn drain(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.pending.len());
        for slot in &mut buf[..n] {
            *slot = self.pending.pop_front().expect("pending length checked");
        }
        n
    }
}

impl BlockingByteStream for NetworkMuxDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        if !self.pending.is_empty() {
            return Ok(ReadStatus::Data(self.drain(buf)));
        }
        match self.rx.recv_timeout(Duration::from_millis(20)) {
            Ok(data) => {
                self.pending.extend(data);
                Ok(ReadStatus::Data(self.drain(buf)))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(ReadStatus::Idle),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(ReadStatus::Eof),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.core.send(data)
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// Protocol-specific service for listener lifecycle, UDP addressing and TCP peer registry.
pub struct NetworkRuntime {
    running: Arc<AtomicBool>,
    session_id: Mutex<Option<String>>,
    max_clients: usize,
    tcp_client: Mutex<Option<TcpDriver>>,
    tcp_listener: Mutex<Option<TcpListenerTransport>>,
    aggregate_tx: mpsc::Sender<Vec<u8>>,
    core: Arc<NetworkCore>,
    encoding: String,
    data_mode: String,
    udp_client_local_addr: Mutex<Option<SocketAddr>>,
}

struct RuntimeAttach {
    runtime: Arc<NetworkRuntime>,
    runtimes: SessionRuntimeRegistry<NetworkRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}

impl NetworkRuntime {
    fn new(
        max_clients: usize,
        aggregate_tx: mpsc::Sender<Vec<u8>>,
        core: Arc<NetworkCore>,
        encoding: String,
        data_mode: String,
    ) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            session_id: Mutex::new(None),
            max_clients,
            tcp_client: Mutex::new(None),
            tcp_listener: Mutex::new(None),
            aggregate_tx,
            core,
            encoding,
            data_mode,
            udp_client_local_addr: Mutex::new(None),
        }
    }

    pub fn start(&self, app: tauri::AppHandle, session_id: &str) -> Result<(), String> {
        *self.session_id.lock().map_err(|error| error.to_string())? = Some(session_id.to_string());
        let mut spawned = 0usize;

        if let Some(driver) = self
            .tcp_client
            .lock()
            .map_err(|error| error.to_string())?
            .take()
        {
            register_tcp_peer(
                &app,
                session_id,
                driver,
                self.encoding.clone(),
                self.data_mode.clone(),
                self.core.clone(),
                self.aggregate_tx.clone(),
            )?;
            spawned += 1;
        }

        if let Some(listener) = self
            .tcp_listener
            .lock()
            .map_err(|error| error.to_string())?
            .take()
        {
            let app = app.clone();
            let session_id = session_id.to_string();
            let running = self.running.clone();
            let max_clients = self.max_clients;
            let encoding = self.encoding.clone();
            let data_mode = self.data_mode.clone();
            let core = self.core.clone();
            let mirror_tx = self.aggregate_tx.clone();
            std::thread::spawn(move || {
                while running.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok(Some((driver, _peer))) => {
                            if client_limit_reached(max_clients, core.active_peer_count()) {
                                log::warn!("网络调试: TCP Server 并发达到上限 {max_clients}");
                                continue;
                            }
                            if let Err(error) = register_tcp_peer(
                                &app,
                                &session_id,
                                driver,
                                encoding.clone(),
                                data_mode.clone(),
                                core.clone(),
                                mirror_tx.clone(),
                            ) {
                                log::error!("网络调试: TCP 对端注册失败: {error}");
                            }
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                        Err(error) => {
                            log::error!("网络调试: TCP accept 失败: {error}");
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
            spawned += 1;
        }

        if let Some(socket) = self
            .core
            .udp_socket
            .lock()
            .map_err(|error| error.to_string())?
            .clone()
        {
            let socket = Arc::new(socket.try_clone().map_err(|error| error.to_string())?);
            let app = app.clone();
            let session_id = session_id.to_string();
            let running = self.running.clone();
            let encoding = self.encoding.clone();
            let data_mode = self.data_mode.clone();
            let aggregate_tx = self.aggregate_tx.clone();
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 65_535];
                while running.load(Ordering::SeqCst) {
                    match socket.recv_from(&mut buf) {
                        Ok(Some((n, source))) => {
                            emit_udp_datagram(
                                &app,
                                &session_id,
                                &encoding,
                                &data_mode,
                                source,
                                &buf[..n],
                            );
                            let _ = aggregate_tx.send(buf[..n].to_vec());
                        }
                        Ok(None) => {}
                        Err(error) => {
                            log::error!("网络调试: UDP recv 失败: {error}");
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
            spawned += 1;
        }

        if spawned == 0 {
            return Err("网络调试会话未初始化任何传输通道".to_string());
        }
        Ok(())
    }

    pub fn list_peers(&self) -> Vec<NetworkPeerInfo> {
        self.core.list_peers()
    }

    pub fn remove_peer(&self, peer_id: &str) {
        self.core.remove_peer(peer_id);
    }

    pub fn udp_send_to(&self, target: &str, data: &[u8]) -> Result<(), String> {
        self.core.udp_send_to(target, data)
    }

    pub fn udp_send(&self, data: &[u8]) -> Result<(), String> {
        self.core.udp_send(data)
    }

    pub fn udp_client_local_addr(&self) -> Option<SocketAddr> {
        self.udp_client_local_addr
            .lock()
            .ok()
            .and_then(|slot| *slot)
    }

    pub fn set_send_target(&self, target: Option<String>) {
        self.core.set_target(target);
    }
}

impl SessionService for NetworkRuntime {
    fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn client_limit_reached(max_clients: usize, connected_peers: usize) -> bool {
    max_clients > 0 && connected_peers >= max_clients
}

fn register_tcp_peer(
    app: &tauri::AppHandle,
    parent_id: &str,
    driver: TcpDriver,
    encoding: String,
    data_mode: String,
    core: Arc<NetworkCore>,
    mirror_tx: mpsc::Sender<Vec<u8>>,
) -> Result<String, String> {
    let peer_addr = driver
        .peer_addr()
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let local_addr = driver
        .local_addr()
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let runtime = DataPlaneRuntime::spawn(Box::new(driver));
    let channel_id = uuid::Uuid::new_v4().to_string();
    let index = core.next_peer_index.fetch_add(1, Ordering::Relaxed);
    let peer_name = format!("Peer {}", index + 1);
    let io = Arc::new(SessionIo::new(
        Some(runtime.handle.clone()),
        None,
        encoding.clone(),
    ));
    core.peer_handles
        .lock()
        .map_err(|error| error.to_string())?
        .insert(channel_id.clone(), runtime.handle.clone());

    let app_state = app.state::<crate::AppState>();
    let log_tx = app_state
        .log_engine
        .lock()
        .map_err(|error| error.to_string())?
        .sender();
    let app_data = app.clone();
    let batcher = DataBatcher::new(move |batched| {
        let _ = app_data.emit(
            "session-data",
            serde_json::json!({
                "session_id": batched.session_id,
                "data_b64": batched.data_b64,
            }),
        );
    });
    let encoding_log = encoding.clone();
    let data_mode_log = data_mode.clone();
    let on_data: Box<dyn Fn(String, Vec<u8>) + Send> = Box::new(move |session_id, data| {
        let payload = data.clone();
        let mirror_payload = data.clone();
        batcher.push(session_id.clone(), data);
        let _ = mirror_tx.send(mirror_payload);
        let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {
            session_id,
            direction: DataDirection::RX,
            data_mode: data_mode_log.clone(),
            encoding: encoding_log.clone(),
            payload,
            timestamp: chrono::Local::now(),
        }));
    });

    let app_disconnect = app.clone();
    let parent_for_disconnect = parent_id.to_string();
    let peer_for_disconnect = channel_id.clone();
    let io_for_disconnect = io.clone();
    let core_for_disconnect = core.clone();
    let disconnect_parent = core.role == "client";
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> = Box::new(move |_id, info| {
        core_for_disconnect.mark_peer_disconnected(&peer_for_disconnect);
        let mut parent_was_disconnected = false;
        if let Ok(mut store) = app_disconnect
            .state::<crate::AppState>()
            .session_store
            .lock()
        {
            store.mark_sub_disconnected(
                &parent_for_disconnect,
                &peer_for_disconnect,
                info.retain_terminal,
            );
            if disconnect_parent
                && store.session_state(&parent_for_disconnect) == Some(SessionState::Connected)
            {
                store.mark_disconnected(&parent_for_disconnect);
                parent_was_disconnected = true;
            }
        }
        let _ = app_disconnect.emit(
            "netdbg-peer-left",
            serde_json::json!({
                "session_id": parent_for_disconnect,
                "peer_id": peer_for_disconnect,
                "tx_bytes": io_for_disconnect.tx_bytes(),
                "rx_bytes": io_for_disconnect.rx_bytes(),
            }),
        );
        if parent_was_disconnected {
            let _ = app_disconnect.emit(
                "session-disconnected",
                serde_json::json!({
                    "session_id": parent_for_disconnect,
                    "reason": info.reason,
                    "disconnect_info": info,
                }),
            );
        }
    });
    let data_plane =
        SessionDataPlane::attach_paused(runtime, channel_id.clone(), on_data, on_disconnect)
            .map_err(|error| error.to_string())?;
    let stats_cancel_flag = Arc::new(AtomicBool::new(false));
    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    SessionStore::spawn_stats_collector(
        app.clone(),
        channel_id.clone(),
        io.clone(),
        connected_at,
        stats_cancel_flag.clone(),
    );

    let mut child = SubConnection::background(
        channel_id.clone(),
        peer_name.clone(),
        data_plane,
        io.clone(),
        index,
    );
    child.connected_at = connected_at;
    child.stats_cancel_flag = Some(stats_cancel_flag.clone());
    if let Err(error) = app_state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?
        .add_sub_connection(parent_id, child)
    {
        stats_cancel_flag.store(true, Ordering::SeqCst);
        if let Ok(mut handles) = core.peer_handles.lock() {
            handles.remove(&channel_id);
        }
        return Err(error);
    }
    core.peers
        .lock()
        .map_err(|error| error.to_string())?
        .insert(
            channel_id.clone(),
            PeerRuntime {
                index,
                name: peer_name.clone(),
                addr: peer_addr.clone(),
                local_addr: local_addr.clone(),
                io,
                connected_at,
                connected: true,
            },
        );
    let _ = app.emit(
        "netdbg-peer-joined",
        serde_json::json!({
            "session_id": parent_id,
            "peer_id": channel_id,
            "peer_name": peer_name,
            "peer_addr": peer_addr,
            "local_addr": local_addr,
        }),
    );
    app_state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?
        .activate_data_plane(&channel_id)?;
    Ok(channel_id)
}

fn emit_udp_datagram(
    app: &tauri::AppHandle,
    session_id: &str,
    encoding: &str,
    data_mode: &str,
    source: SocketAddr,
    datagram: &[u8],
) {
    let _ = app.emit(
        "session-data",
        serde_json::json!({
            "session_id": session_id,
            "data_b64": base64_encode(datagram),
            "source_addr": source.to_string(),
        }),
    );
    if let Ok(engine) = app.state::<crate::AppState>().log_engine.lock() {
        let _ = engine
            .sender()
            .try_send(LogEntry::SessionData(DataLogEntry {
                session_id: session_id.to_string(),
                direction: DataDirection::RX,
                data_mode: data_mode.to_string(),
                encoding: encoding.to_string(),
                payload: datagram.to_vec(),
                timestamp: chrono::Local::now(),
            }));
    }
}

pub struct NetworkAdapter {
    runtimes: SessionRuntimeRegistry<NetworkRuntime>,
}

impl NetworkAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<NetworkRuntime>> {
        self.runtimes.get(session_id)
    }

    pub fn list_peers(&self, session_id: &str) -> Vec<NetworkPeerInfo> {
        self.runtime(session_id)
            .map(|runtime| runtime.list_peers())
            .unwrap_or_default()
    }
}

impl Default for NetworkAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for NetworkAdapter {
    fn plugin_id(&self) -> Option<&'static str> {
        Some(PLUGIN_ID)
    }

    async fn connect(
        &self,
        endpoint: &str,
        params: &Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let transport = params
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or("tcp")
            .to_string();
        let role = params
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("client")
            .to_string();
        let max_clients = params
            .get("max_clients")
            .and_then(Value::as_u64)
            .unwrap_or(16) as usize;
        let encoding = params
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or("utf-8")
            .to_string();
        let data_mode = params
            .get("data_mode")
            .and_then(Value::as_str)
            .unwrap_or("dual")
            .to_string();

        let (aggregate_tx, aggregate_rx) = mpsc::channel();
        let core = Arc::new(NetworkCore::new(transport.clone(), role.clone()));
        let side = Arc::new(NetworkRuntime::new(
            max_clients,
            aggregate_tx,
            core.clone(),
            encoding,
            data_mode,
        ));

        match (transport.as_str(), role.as_str()) {
            ("tcp", "client") => {
                let host = params
                    .get("remote_host")
                    .and_then(Value::as_str)
                    .unwrap_or("127.0.0.1");
                let port = params
                    .get("remote_port")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u16;
                let config = TcpConnectConfig {
                    connect_timeout_ms: params
                        .get("connect_timeout_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(5_000),
                    read_timeout_ms: 20,
                    nodelay: params
                        .get("nodelay")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                };
                let driver = connect_tcp(host, port, &config)?;
                *side
                    .tcp_client
                    .lock()
                    .map_err(|error| SessionError::Other(error.to_string()))? = Some(driver);
            }
            ("tcp", "server") => {
                let host = params
                    .get("local_host")
                    .and_then(Value::as_str)
                    .unwrap_or("0.0.0.0");
                let port = params
                    .get("local_port")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u16;
                let config = TcpConnectConfig {
                    connect_timeout_ms: 5_000,
                    read_timeout_ms: 20,
                    nodelay: params
                        .get("nodelay")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                };
                *side
                    .tcp_listener
                    .lock()
                    .map_err(|error| SessionError::Other(error.to_string()))? =
                    Some(TcpListenerTransport::bind(host, port, config)?);
            }
            ("udp", role @ ("client" | "server")) => {
                let host = params
                    .get("local_host")
                    .and_then(Value::as_str)
                    .unwrap_or("0.0.0.0");
                let port = params
                    .get("local_port")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u16;
                let socket = Arc::new(UdpTransport::bind(host, port)?);
                socket.set_read_timeout(Duration::from_millis(200))?;

                if role == "client" {
                    let remote_host = params
                        .get("remote_host")
                        .and_then(Value::as_str)
                        .unwrap_or("127.0.0.1");
                    let remote_port = params
                        .get("remote_port")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u16;
                    let remote = resolve_udp(remote_host, remote_port)?;
                    *core
                        .udp_client_target
                        .lock()
                        .map_err(|error| SessionError::Other(error.to_string()))? = Some(remote);
                    *side
                        .udp_client_local_addr
                        .lock()
                        .map_err(|error| SessionError::Other(error.to_string()))? =
                        Some(socket.local_addr()?);
                } else {
                    if params
                        .get("broadcast")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        socket.set_broadcast(true)?;
                    }
                    if let Some(group) = params
                        .get("multicast_group")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        let group: Ipv4Addr = group.parse().map_err(|error| {
                            SessionError::InvalidParameter(format!(
                                "无效的组播组地址 {group}: {error}"
                            ))
                        })?;
                        if !group.is_multicast() {
                            return Err(SessionError::InvalidParameter(format!(
                                "组播组地址 {group} 不在 IPv4 组播范围"
                            )));
                        }
                        let interface: Ipv4Addr = params
                            .get("multicast_interface")
                            .and_then(Value::as_str)
                            .unwrap_or("0.0.0.0")
                            .parse()
                            .map_err(|error| {
                                SessionError::InvalidParameter(format!("无效的组播接口: {error}"))
                            })?;
                        socket.join_multicast_v4(group, interface)?;
                        socket.set_multicast_ttl_v4(
                            params.get("ttl").and_then(Value::as_u64).unwrap_or(64) as u32,
                        )?;
                        socket.set_multicast_loop_v4(
                            params
                                .get("self_receive")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                        )?;
                    }
                }
                *core
                    .udp_socket
                    .lock()
                    .map_err(|error| SessionError::Other(error.to_string()))? = Some(socket);
            }
            (transport, role) => {
                return Err(SessionError::InvalidParameter(format!(
                    "不支持的传输/角色组合: {transport}/{role}"
                )));
            }
        }

        let runtime = DataPlaneRuntime::spawn(Box::new(NetworkMuxDriver::new(
            aggregate_rx,
            core.clone(),
            side.running.clone(),
        )));
        log::info!("网络调试会话已初始化: transport={transport} role={role} endpoint={endpoint}");
        Ok(ProtocolConnection {
            data_plane: Some(runtime),
            service: Some(side.clone()),
            file_transfer: None,
            channel_factory: None,
            on_attached: Some(Arc::new(RuntimeAttach {
                runtime: side,
                runtimes: self.runtimes.clone(),
            })),
            teardown_delay: Duration::ZERO,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_limit_zero_means_unlimited() {
        assert!(!client_limit_reached(0, 10_000));
        assert!(!client_limit_reached(3, 2));
        assert!(client_limit_reached(3, 3));
    }

    #[test]
    fn aggregate_driver_reports_idle_and_data() {
        let (tx, rx) = mpsc::channel();
        let core = Arc::new(NetworkCore::new("tcp".into(), "client".into()));
        let running = Arc::new(AtomicBool::new(true));
        let mut driver = NetworkMuxDriver::new(rx, core, running);
        let mut buf = [0u8; 8];
        assert!(matches!(driver.read(&mut buf).unwrap(), ReadStatus::Idle));
        tx.send(vec![1, 2, 3]).unwrap();
        assert!(matches!(
            driver.read(&mut buf).unwrap(),
            ReadStatus::Data(3)
        ));
        assert_eq!(&buf[..3], &[1, 2, 3]);
    }

    #[test]
    fn udp_transport_round_trip() {
        let a = UdpTransport::bind("127.0.0.1", 0).unwrap();
        let b = UdpTransport::bind("127.0.0.1", 0).unwrap();
        b.set_read_timeout(Duration::from_millis(100)).unwrap();
        a.send_to(b"ping", b.local_addr().unwrap()).unwrap();
        let mut buf = [0u8; 16];
        let (n, _) = b.recv_from(&mut buf).unwrap().expect("datagram");
        assert_eq!(&buf[..n], b"ping");
    }
}
