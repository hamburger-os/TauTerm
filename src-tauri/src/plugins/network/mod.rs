//! Network debugging protocol plugin.
//!
//! TCP peers own independent DataPlane runtimes. The root network session owns a lightweight
//! aggregate DataPlane used by scripts/auto-reply and by the common send path. UDP remains a
//! datagram transport but feeds received payloads into that aggregate DataPlane so upper layers do
//! not need a second callback bus.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::kernel::data_batcher::base64_encode;
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogEntry};
use crate::kernel::plugin_adapter::{ProtocolAdapter, ProtocolConnection, SideChannel};
use crate::kernel::session_store::PeerChannelRegistration;
use crate::session::SessionError;
use crate::transport::tcp::{connect_tcp, TcpConnectConfig, TcpDriver, TcpListenerTransport};
use crate::transport::udp::{resolve_udp, UdpTransport};
use crate::transport::{
    BlockingByteStream, DataPlaneHandle, DataPlaneRuntime, ReadStatus, TransportError,
    TransportErrorKind,
};

struct NetworkCore {
    transport: String,
    role: String,
    send_target: Mutex<Option<String>>,
    peer_handles: Arc<Mutex<HashMap<String, DataPlaneHandle>>>,
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
pub struct NetworkSideChannel {
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

impl NetworkSideChannel {
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
                self.core.peer_handles.clone(),
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
            let peer_handles = self.core.peer_handles.clone();
            let mirror_tx = self.aggregate_tx.clone();
            std::thread::spawn(move || {
                while running.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok(Some((driver, _peer))) => {
                            let connected = app
                                .state::<crate::AppState>()
                                .session_store
                                .lock()
                                .map(|store| {
                                    store
                                        .list_peers(&session_id)
                                        .iter()
                                        .filter(|peer| peer.state == "connected")
                                        .count()
                                })
                                .unwrap_or(0);
                            if client_limit_reached(max_clients, connected) {
                                log::warn!("网络调试: TCP Server 并发达到上限 {max_clients}");
                                continue;
                            }
                            if let Err(error) = register_tcp_peer(
                                &app,
                                &session_id,
                                driver,
                                encoding.clone(),
                                data_mode.clone(),
                                peer_handles.clone(),
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

impl SideChannel for NetworkSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

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
    peer_handles: Arc<Mutex<HashMap<String, DataPlaneHandle>>>,
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
    let app_state = app.state::<crate::AppState>();
    let log_tx = app_state
        .log_engine
        .lock()
        .map_err(|error| error.to_string())?
        .sender();
    let result = app_state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?
        .register_peer_channel(
            app,
            log_tx,
            PeerChannelRegistration {
                parent_id: parent_id.to_string(),
                peer_name: String::new(),
                peer_addr,
                local_addr,
                runtime,
                encoding,
                data_mode,
                peer_handles,
                mirror_tx: Some(mirror_tx),
            },
        );
    result
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

pub struct NetworkAdapter;

impl NetworkAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for NetworkAdapter {
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
        let side = Arc::new(NetworkSideChannel::new(
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
            side_channel: Some(side),
            channel_factory: None,
            on_attached: None,
            teardown_delay: Duration::ZERO,
        })
    }

    fn content_type(&self) -> crate::kernel::plugin_adapter::ContentType {
        crate::kernel::plugin_adapter::ContentType::Custom
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
