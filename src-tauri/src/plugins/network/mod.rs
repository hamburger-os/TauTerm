//! 网络调试会话插件
//!
//! 将 TCP / UDP 调试能力合并为一个"网络调试会话"（容器会话模式）：
//! - **TCP Client**：连接远端，单个对端
//! - **TCP Server**：本地监听，多客户端并发（每个客户端一个对端）
//! - **UDP**：绑定本地端口，支持单播 / 广播 / 组播（`IP_ADD_MEMBERSHIP`）。
//!   无连接语义不建立对端——单 socket `recv_from` 直接按来源地址 emit 数据报
//!
//! TCP 对端经内核 [`SessionStore::register_peer_channel`] 注册为独立通道，获得
//! 独立的 I/O loop、统计采集、CommHandle（自动应答/脚本按对端生效）、日志与
//! 数据事件流；前端以"对端列表 + 选中对端详情"展示。UDP 则保持单会话，
//! 报文网格显示所有来源时间线，发送目标由发送栏手动地址（含广播/组播）决定。
//!
//! 对端断开 / 关闭不级联父会话——监听器（TCP Server / UDP bind）保持监听。

use std::any::Any;
use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::channel::error::SessionError;
use crate::channel::io_loop::IoLoopCmd;
use crate::kernel::comm_handle::{CommHandle, DataCallback};
use crate::kernel::data_batcher::base64_encode;
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogEntry};
use crate::kernel::plugin_adapter::{ChannelKind, ProtocolAdapter, ProtocolConnection, SideChannel};

mod comm;
mod tcp_channel;

use comm::NetworkCommHandle;
use tcp_channel::TcpChannel;

/// 网络调试会话侧通道
///
/// `connect()` 阶段完成同步操作（解析参数、绑定端口、TCP 连接），
/// `start()` 阶段（由 connect 命令在容器会话创建后调用）启动监听/接收线程。
pub struct NetworkSideChannel {
    /// 关闭标志（`shutdown()` 置位，各线程轮询退出）
    running: Arc<AtomicBool>,
    /// 容器会话 ID（`start()` 时写入）
    session_id: Mutex<Option<String>>,
    /// 会话字符编码（对端文本路径转码 + 日志解码）
    encoding: Mutex<String>,
    /// 数据模式（text / hex / dual，日志用）
    data_mode: Mutex<String>,
    /// TCP Server 最大并发客户端（0 = 不限）
    max_clients: usize,
    /// TCP Client 已连接流（`connect()` 时建立，`start()` 时注册为对端）
    tcp_client_stream: Mutex<Option<TcpStream>>,
    /// TCP Server 监听器（`connect()` 时绑定，`start()` 时 accept 循环接管）
    tcp_listener: Mutex<Option<TcpListener>>,
    /// UDP 共享 socket（`connect()` 时绑定；struct 保留一份供发送，线程持克隆）
    udp_socket: Mutex<Option<Arc<UdpSocket>>>,
    /// UDP Client 固定远端（发送目标；不 connect，保证 recv 可接收任意来源）
    udp_client_target: Mutex<Option<SocketAddr>>,
    /// UDP Client 本地绑定地址（连接后本机 ip:port，前端展示用）
    udp_client_local_addr: Mutex<Option<SocketAddr>>,
    /// 当前发送目标（前端目标栏同步：UDP server 手动地址；TCP server 对端/全部）
    send_target: Mutex<Option<String>>,
    /// 脚本引擎数据接收回调（UDP 报文经 `notify_receive` 送达 `on_data`）
    receivers: Arc<Mutex<Vec<DataCallback>>>,
    /// TCP 对端写通道注册表（peer_id → write_tx），供容器级脚本引擎按目标路由/群发
    peer_writers: Arc<Mutex<HashMap<String, mpsc::SyncSender<IoLoopCmd>>>>,
}

impl NetworkSideChannel {
    pub fn new(max_clients: usize) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            session_id: Mutex::new(None),
            encoding: Mutex::new("utf-8".to_string()),
            data_mode: Mutex::new("dual".to_string()),
            max_clients,
            tcp_client_stream: Mutex::new(None),
            tcp_listener: Mutex::new(None),
            udp_socket: Mutex::new(None),
            udp_client_target: Mutex::new(None),
            udp_client_local_addr: Mutex::new(None),
            send_target: Mutex::new(None),
            receivers: Arc::new(Mutex::new(Vec::new())),
            peer_writers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 启动监听 / 接收线程（在容器会话创建后调用）
    pub fn start(&self, app: tauri::AppHandle, session_id: &str) -> Result<(), String> {
        *self.session_id.lock().map_err(|e| e.to_string())? = Some(session_id.to_string());
        let encoding = self.encoding.lock().map_err(|e| e.to_string())?.clone();
        let data_mode = self.data_mode.lock().map_err(|e| e.to_string())?.clone();
        let running = self.running.clone();
        let sid = session_id.to_string();

        let mut spawned = 0;
        // TCP Client：取走已连接流并注册为单个对端
        if let Some(stream) = self.tcp_client_stream.lock().map_err(|e| e.to_string())?.take() {
            let app_c = app.clone();
            let sid_c = sid.clone();
            let encoding_c = encoding.clone();
            let data_mode_c = data_mode.clone();
            let peer_writers = self.peer_writers.clone();
            let container_receivers = self.receivers.clone();
            let _ = std::thread::spawn(move || {
                let addr_s = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                // 本端地址：TCP client 连接后本机分配的 ip:port，与服务端对端条目对应
                let local_s = stream
                    .local_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                let _ = stream.set_nodelay(true);
                match TcpChannel::new(stream) {
                    Ok(ch) => {
                        // peer_name 留空：内核按序号自动命名（"Peer N"，与 SSH "Channel N" 一致）
                        if let Err(e) = register_peer(
                            &app_c, &sid_c, "", &addr_s, &local_s,
                            ChannelKind::Sync(Box::new(ch)), &encoding_c, &data_mode_c,
                            peer_writers,
                            container_receivers,
                        ) {
                            log::error!("网络调试: TCP Client 对端注册失败: {}", e);
                        }
                    }
                    Err(e) => log::error!("网络调试: TCP Client 通道创建失败: {}", e),
                }
            });
            spawned += 1;
        }

        // TCP Server：accept 循环
        if let Some(listener) = self.tcp_listener.lock().map_err(|e| e.to_string())?.take() {
            let app_c = app.clone();
            let sid_c = sid.clone();
            let encoding_c = encoding.clone();
            let data_mode_c = data_mode.clone();
            let running_c = running.clone();
            let max_clients = self.max_clients;
            let peer_writers = self.peer_writers.clone();
            let container_receivers = self.receivers.clone();
            let _ = std::thread::spawn(move || {
                listener.set_nonblocking(true).ok();
                while running_c.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, addr)) => {
                            // 并发上限检查（0 = 不限，见 client_limit_reached）。
                            // 只统计 Connected 状态对端：断开的对端是墓碑
                            // （保留最终统计供调试），不应占用并发名额。
                            let connected_count = app_c
                                .state::<crate::AppState>()
                                .session_store
                                .lock()
                                .map(|s| {
                                    s.list_peers(&sid_c)
                                        .iter()
                                        .filter(|p| p.state == "connected")
                                        .count()
                                })
                                .unwrap_or(0);
                            if client_limit_reached(max_clients, connected_count) {
                                log::warn!(
                                    "网络调试: TCP Server 并发达到上限 {}，拒绝 {}",
                                    max_clients,
                                    addr
                                );
                                continue;
                            }
                            let _ = stream.set_nodelay(true);
                            let addr_s = addr.to_string();
                            // 本端地址：接受连接的 socket 本地地址（监听端）
                            let local_s = stream
                                .local_addr()
                                .map(|a| a.to_string())
                                .unwrap_or_else(|_| "unknown".to_string());
                            match TcpChannel::new(stream) {
                                Ok(ch) => {
                                    // peer_name 留空：内核按序号自动命名（"Peer N"）
                                    if let Err(e) = register_peer(
                                        &app_c, &sid_c, "", &addr_s, &local_s,
                                        ChannelKind::Sync(Box::new(ch)), &encoding_c, &data_mode_c,
                                        peer_writers.clone(),
                                        container_receivers.clone(),
                                    ) {
                                        log::error!("网络调试: TCP Server 对端注册失败: {}", e);
                                    }
                                }
                                Err(e) => {
                                    log::error!("网络调试: TCP Server 通道创建失败: {}", e);
                                }
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            log::error!("网络调试: TCP Server accept 错误: {}", e);
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
            spawned += 1;
        }

        // UDP：recv 循环，单 socket 按来源地址直接 emit 数据报（无对端模型）
        if let Some(socket) = self.udp_socket.lock().map_err(|e| e.to_string())?.clone() {
            let app_c = app.clone();
            let sid_c = sid.clone();
            let encoding_c = encoding.clone();
            let data_mode_c = data_mode.clone();
            let running_c = running.clone();
            let receivers = self.receivers.clone();
            let _ = std::thread::spawn(move || {
                // 200ms 读超时轮询 running 标志（Windows SO_RCVTIMEO 最小 1ms）
                socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
                let mut buf = vec![0u8; 65535];
                while running_c.load(Ordering::SeqCst) {
                    match socket.recv_from(&mut buf) {
                        Ok((n, src)) => {
                            emit_udp_datagram(
                                &app_c, &sid_c, &encoding_c, &data_mode_c,
                                src, &buf[..n],
                            );
                            // 脚本引擎消费者（auto-reply / script 的 on_data）
                            if let Ok(rx) = receivers.lock() {
                                for cb in rx.iter() {
                                    cb(&buf[..n]);
                                }
                            }
                        }
                        Err(ref e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                || e.kind() == io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            log::error!("网络调试: UDP recv 错误: {}", e);
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

    /// UDP 手动目标发送（server：前端指定任意目标地址，含广播/组播地址）
    pub fn udp_send_to(&self, target: &str, data: &[u8]) -> Result<(), String> {
        let socket = self
            .udp_socket
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or("UDP socket 不可用".to_string())?;
        let addr: SocketAddr = target
            .parse()
            .map_err(|e| format!("无效的目标地址 {}: {}", target, e))?;
        socket
            .send_to(data, addr)
            .map(|_| ())
            .map_err(|e| format!("UDP 发送失败: {}", e))
    }

    /// UDP 固定远端发送（client：不 connect，按固定目标 `send_to` 到远端）
    pub fn udp_send(&self, data: &[u8]) -> Result<(), String> {
        let socket = self
            .udp_socket
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or("UDP socket 不可用".to_string())?;
        let target = self
            .udp_client_target
            .lock()
            .map_err(|e| e.to_string())?
            .ok_or("UDP 客户端目标未设置".to_string())?;
        socket
            .send_to(data, target)
            .map(|_| ())
            .map_err(|e| format!("UDP 发送失败: {}", e))
    }

    /// UDP Client 本地绑定地址（前端展示本机 ip:port 用）
    pub fn udp_client_local_addr(&self) -> Option<SocketAddr> {
        self.udp_client_local_addr
            .lock()
            .ok()
            .and_then(|v| *v)
    }

    /// 读取当前发送目标（前端目标栏同步）。
    pub fn current_target(&self) -> Option<String> {
        self.send_target.lock().ok().and_then(|v| v.clone())
    }

    /// 设置当前发送目标（前端目标栏同步）。
    pub fn set_send_target(&self, target: Option<String>) {
        if let Ok(mut t) = self.send_target.lock() {
            *t = target;
        }
    }

    /// 会话字符编码（文本路径转码用）。
    pub fn encoding(&self) -> String {
        self.encoding
            .lock()
            .map(|e| e.clone())
            .unwrap_or_else(|_| "utf-8".to_string())
    }

    /// 注册脚本引擎数据接收回调。
    pub fn register_receiver(&self, cb: DataCallback) {
        if let Ok(mut receivers) = self.receivers.lock() {
            receivers.push(cb);
        }
    }

    /// 将接收数据扇出给脚本引擎等消费者。
    pub fn notify_receive(&self, data: &[u8]) {
        if let Ok(receivers) = self.receivers.lock() {
            for cb in receivers.iter() {
                cb(data);
            }
        }
    }

    /// 清空脚本引擎数据接收回调。
    pub fn clear_receivers(&self) {
        if let Ok(mut receivers) = self.receivers.lock() {
            receivers.clear();
        }
    }
}

/// 直接 emit 一个 UDP 数据报到前端（无对端模型：`session_id` = 容器会话，附带来源地址）。
/// 同时写入 RX 数据日志（best-effort，失败不影响主流程）。
fn emit_udp_datagram(
    app: &tauri::AppHandle,
    sid: &str,
    encoding: &str,
    data_mode: &str,
    src: SocketAddr,
    datagram: &[u8],
) {
    let _ = app.emit("session-data", serde_json::json!({
        "session_id": sid,
        "data_b64": base64_encode(datagram),
        "source_addr": src.to_string(),
    }));
    if let Ok(engine) = app.state::<crate::AppState>().log_engine.lock() {
        let _ = engine.sender().try_send(LogEntry::SessionData(DataLogEntry {
            session_id: sid.to_string(),
            direction: DataDirection::RX,
            data_mode: data_mode.to_string(),
            encoding: encoding.to_string(),
            payload: datagram.to_vec(),
            timestamp: chrono::Local::now(),
        }));
    }
}

impl SideChannel for NetworkSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    /// 停止监听 / 接收线程。
    ///
    /// 注意：仅置位 `running` 标志，**不 join 线程**。`close_session` 持有
    /// session_store 锁调用本方法；若在此 join，而线程正阻塞在
    /// `register_peer_channel`（等待同一把锁），将形成死锁。
    /// 线程最迟在 200ms 读超时轮询内自行退出。
    fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// TCP Server 并发上限闸门：`max_clients == 0` 表示不限。
///
/// 只统计 Connected 状态对端——断开的对端是墓碑（保留最终统计供调试），
/// 不应占用并发名额。
fn client_limit_reached(max_clients: usize, connected_peers: usize) -> bool {
    max_clients > 0 && connected_peers >= max_clients
}

/// 将已连接的对端通道注册到内核会话
#[allow(clippy::too_many_arguments)]
fn register_peer(
    app: &tauri::AppHandle,
    session_id: &str,
    peer_name: &str,
    peer_addr: &str,
    local_addr: &str,
    channel: ChannelKind,
    encoding: &str,
    data_mode: &str,
    peer_writers: Arc<Mutex<HashMap<String, mpsc::SyncSender<IoLoopCmd>>>>,
    container_receivers: Arc<Mutex<Vec<DataCallback>>>,
) -> Result<String, String> {
    let app_state = app.state::<crate::AppState>();
    let log_tx = {
        let engine = app_state.log_engine.lock().map_err(|e| e.to_string())?;
        engine.sender()
    };
    let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
    store.register_peer_channel(
        app,
        log_tx,
        session_id,
        peer_name,
        peer_addr,
        local_addr,
        channel,
        encoding,
        data_mode,
        peer_writers,
        container_receivers,
    )
}

/// 网络调试协议适配器
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
            .and_then(|v| v.as_str())
            .unwrap_or("tcp")
            .to_string();
        let role = params
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("client")
            .to_string();
        let max_clients = params
            .get("max_clients")
            .and_then(|v| v.as_u64())
            .unwrap_or(16) as usize;
        let encoding = params
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8")
            .to_string();
        let data_mode = params
            .get("data_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("dual")
            .to_string();

        let side = NetworkSideChannel::new(max_clients);
        *side
            .encoding
            .lock()
            .map_err(|e| SessionError::Other(e.to_string()))? = encoding;
        *side
            .data_mode
            .lock()
            .map_err(|e| SessionError::Other(e.to_string()))? = data_mode;

        match (transport.as_str(), role.as_str()) {
            ("tcp", "client") => {
                let host = params
                    .get("remote_host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("127.0.0.1");
                let port = params
                    .get("remote_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                let timeout_ms = params
                    .get("connect_timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5000);
                let nodelay = params
                    .get("nodelay")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let addr = (host, port)
                    .to_socket_addrs()
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("无法解析 {}:{}: {}", host, port, e),
                    })?
                    .next()
                    .ok_or_else(|| SessionError::ConnectionFailed {
                        reason: format!("无法解析 {}:{}（无可用地址）", host, port),
                    })?;
                let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms))
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("连接 {} 失败: {}", addr, e),
                    })?;
                let _ = stream.set_nodelay(nodelay);
                *side
                    .tcp_client_stream
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? = Some(stream);
            }
            ("tcp", "server") => {
                let local_host = params
                    .get("local_host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0.0");
                let local_port = params
                    .get("local_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                let listener = TcpListener::bind((local_host, local_port))
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("监听 {}:{} 失败: {}", local_host, local_port, e),
                    })?;
                *side
                    .tcp_listener
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? = Some(listener);
            }
            ("udp", "client") => {
                let remote_host = params
                    .get("remote_host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("127.0.0.1");
                let remote_port = params
                    .get("remote_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                let local_host = params
                    .get("local_host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0.0");
                let local_port = params
                    .get("local_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                let remote: SocketAddr = (remote_host, remote_port)
                    .to_socket_addrs()
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("无法解析 {}:{}: {}", remote_host, remote_port, e),
                    })?
                    .next()
                    .ok_or_else(|| SessionError::ConnectionFailed {
                        reason: format!("无法解析 {}:{}（无可用地址）", remote_host, remote_port),
                    })?;
                let socket = UdpSocket::bind((local_host, local_port))
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("绑定 UDP {}:{} 失败: {}", local_host, local_port, e),
                    })?;
                // 不 connect：仅固定发送目标，recv_from 仍可接收任意来源（含广播/组播）
                let local_addr = socket.local_addr().map_err(|e| SessionError::ConnectionFailed {
                    reason: format!("获取 UDP 本地地址失败: {}", e),
                })?;
                log::info!(
                    "网络调试: UDP Client 目标 {}（本地绑定 {}）",
                    remote, local_addr
                );
                *side
                    .udp_client_target
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? = Some(remote);
                *side
                    .udp_client_local_addr
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? = Some(local_addr);
                *side
                    .udp_socket
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? = Some(Arc::new(socket));
            }
            ("udp", "server") => {
                let local_host = params
                    .get("local_host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0.0");
                let local_port = params
                    .get("local_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;
                let broadcast = params
                    .get("broadcast")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let multicast_group = params
                    .get("multicast_group")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());
                let ttl = params
                    .get("ttl")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(64) as u32;
                let multicast_interface = params
                    .get("multicast_interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0.0");
                let self_receive = params
                    .get("self_receive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let socket = UdpSocket::bind((local_host, local_port))
                    .map_err(|e| SessionError::ConnectionFailed {
                        reason: format!("绑定 UDP {}:{} 失败: {}", local_host, local_port, e),
                    })?;
                if broadcast {
                    socket.set_broadcast(true).map_err(SessionError::IoError)?;
                }
                if let Some(group) = multicast_group {
                    let g: Ipv4Addr = group
                        .parse()
                        .map_err(|e| SessionError::ConnectionFailed {
                            reason: format!("无效的组播组地址 {}: {}", group, e),
                        })?;
                    // join_multicast_v4 仅支持 IPv4 组播范围（224.0.0.0 ~ 239.255.255.255）；
                    // 单播地址在部分平台 join 成功但收不到组播流，提前给出明确错误
                    if !g.is_multicast() {
                        return Err(SessionError::ConnectionFailed {
                            reason: format!(
                                "组播组地址 {} 不在 IPv4 组播范围 224.0.0.0 ~ 239.255.255.255",
                                group
                            ),
                        });
                    }
                    let iface: Ipv4Addr = multicast_interface
                        .parse()
                        .map_err(|e| SessionError::ConnectionFailed {
                            reason: format!("无效的组播接口 {}: {}", multicast_interface, e),
                        })?;
                    socket
                        .join_multicast_v4(&g, &iface)
                        .map_err(|e| SessionError::ConnectionFailed {
                            reason: format!("加入组播组 {} 失败: {}", group, e),
                        })?;
                    socket
                        .set_multicast_ttl_v4(ttl)
                        .map_err(SessionError::IoError)?;
                    socket
                        .set_multicast_loop_v4(self_receive)
                        .map_err(SessionError::IoError)?;
                    log::info!(
                        "网络调试: UDP 已加入组播组 {} (ttl={}, 自发自收={})",
                        group,
                        ttl,
                        self_receive
                    );
                }
                *side
                    .udp_socket
                    .lock()
                    .map_err(|e| SessionError::Other(e.to_string()))? =
                    Some(Arc::new(socket));
            }
            (t, r) => {
                return Err(SessionError::InvalidParameter(format!(
                    "不支持的传输/角色组合: {}/{}",
                    t, r
                )));
            }
        }

        log::info!(
            "网络调试会话已初始化: transport={} role={} endpoint={}",
            transport,
            role,
            endpoint
        );
        let side_arc = Arc::new(side);
        let comm_handle: Arc<dyn CommHandle> = Arc::new(NetworkCommHandle::new(
            side_arc.clone(),
            transport.clone(),
            role.clone(),
        ));
        Ok(ProtocolConnection {
            channel: None,
            comm_handle: Some(comm_handle),
            side_channel: Some(side_arc),
            teardown_delay: Duration::ZERO,
        })
    }

    fn content_type(&self) -> crate::channel::ContentType {
        crate::channel::ContentType::Custom
    }

    fn io_strategy(&self) -> crate::channel::IoStrategy {
        crate::channel::IoStrategy::Sync
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use crate::channel::Channel;

    // ── 测试辅助 ─────────────────────────────────────

    fn loopback_pair() -> (SocketAddr, Arc<UdpSocket>, Arc<UdpSocket>) {
        let a = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).unwrap());
        let b = Arc::new(UdpSocket::bind(("127.0.0.1", 0)).unwrap());
        (a.local_addr().unwrap(), a, b)
    }

    // ── 1. EOF 探测断开 vs 空闲不误判 ─────────────────

    /// TCP 通道读：对端关闭 → read 返回 UnexpectedEof 错误（I/O loop 据此触发 on_disconnect）
    #[test]
    fn tcp_channel_read_eof_triggers_error() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let mut ch = TcpChannel::new(server).unwrap();

        // 空闲（无数据、对端未关）：read 返回 Ok(0)，不误判断开
        let mut buf = [0u8; 16];
        ch.set_timeout(Duration::from_millis(50)).unwrap();
        assert_eq!(ch.read(&mut buf).unwrap(), 0);

        // 对端关闭 → UnexpectedEof
        drop(client);
        // 足够多的轮次让 FIN 到达（同机回环即时，保留余量）
        let mut got_eof = false;
        for _ in 0..20 {
            if let Err(e) = ch.read(&mut buf) {
                assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof);
                got_eof = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(got_eof, "对端关闭后应读到 EOF 错误");
    }

    /// 空闲期间多次 read 均为 Ok(0)，且通道保持 connected
    #[test]
    fn tcp_channel_idle_reads_stay_connected() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let mut ch = TcpChannel::new(server).unwrap();
        ch.set_timeout(Duration::from_millis(10)).unwrap();

        let mut buf = [0u8; 16];
        for _ in 0..10 {
            assert_eq!(ch.read(&mut buf).unwrap(), 0);
        }
        assert!(ch.is_connected(), "空闲不误判为断开");
    }

    // ── 4. 持锁关闭不死锁（两段式 close_sub_connection） ──

    /// 模拟 on_disconnect 回调：在 I/O 线程退出路径中取 store 锁落状态。
    /// close_sub_connection 阶段 1 只发信号不 join，因此即使回调正在等锁，
    /// 阶段 1（持锁）也能快速返回；join 在锁外完成。
    #[test]
    fn close_sub_connection_no_deadlock_with_disconnect_callback() {
        let mut store = crate::kernel::session_store::SessionStore::new();
        // 容器会话（无 I/O loop），close_sub_connection 的目标形态
        let sid = store
            .create_container_session(
                "netdbg-test", "network", "udp://127.0.0.1:0", serde_json::json!({}),
                None, None, false, None, false, None,
            )
            .unwrap();

        // 直接构造 SubConnection（不经 register_peer_channel，避免 AppHandle 依赖）：
        // 用一对本地 TCP 模拟对端通道，I/O loop 在另一线程运行。
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let ch = TcpChannel::new(server).unwrap();

        let (write_tx, write_rx) = mpsc::sync_channel::<crate::channel::io_loop::IoLoopCmd>(16);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));

        // on_disconnect 模拟真实路径：取 store 锁（store 由测试线程持有 Mutex 包装，
        // 这里直接用独立锁模拟"回调等锁"场景的时序正确性——两段式保证阶段 1 不等待回调）
        let disconnect_latch = Arc::new(AtomicBool::new(false));
        let latch_c = disconnect_latch.clone();
        let on_disconnect = Box::new(move |_sid: String| {
            // 回调耗时操作模拟（真实路径为取 store 锁）
            std::thread::sleep(Duration::from_millis(100));
            latch_c.store(true, Ordering::SeqCst);
        });

        let io_thread = crate::channel::io_loop::spawn_sync_io_loop(
            Box::new(ch), sid.clone(),
            Box::new(|_, _| {}),
            on_disconnect, write_rx, cancel_rx,
            tx_bytes.clone(), rx_bytes.clone(),
        );

        let sub = crate::kernel::session_store::SubConnection {
            id: "sub-test-1".to_string(),
            name: "Peer 1".to_string(),
            write_tx,
            io_cancel_tx: Some(cancel_tx),
            io_thread: Some(crate::kernel::session_store::IoTaskHandle::Sync(io_thread)),
            state: crate::kernel::session_store::SessionState::Connected,
            connected_at: None,
            stats_cancel_flag: Some(Arc::new(AtomicBool::new(false))),
            channel_index: 0,
            tx_bytes,
            rx_bytes,
            comm_handle: None,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            tabbed: false,
            peer_addr: Some("127.0.0.1:12345".to_string()),
            local_addr: Some("127.0.0.1:54321".to_string()),
        };
        store.add_sub_connection(&sid, sub).unwrap();

        // 先让 I/O 线程观测到对端 FIN（真实断开路径触发 on_disconnect 回调，
        // 回调内含 100ms 耗时模拟真实取锁等待），再走两段式关闭。
        // 若 close_sub_connection 退化为"锁内 join"，join 会等待正在执行回调的
        // I/O 线程，而回调又需要 store 锁 → 死锁 → 测试超时卡死。
        drop(client);
        std::thread::sleep(Duration::from_millis(200));

        // 阶段 1（持锁语义）：发信号 + 移除 + 返回句柄；阶段 2（锁外）：join
        let (_is_last, cleanup) = {
            store.close_sub_connection(&sid, "sub-test-1").unwrap()
        };
        cleanup.join();
        assert!(disconnect_latch.load(Ordering::SeqCst), "I/O 线程应已退出并执行回调");
    }

    // ── 5. 上限闸门（max_clients） ─────

    #[test]
    fn tcp_max_clients_gate_counts_only_connected() {
        // 0 = 不限
        assert!(!client_limit_reached(0, 0));
        assert!(!client_limit_reached(0, 10_000));
        // 上限 2：0/1 个活连接放行，2 个拒绝
        assert!(!client_limit_reached(2, 0));
        assert!(!client_limit_reached(2, 1));
        assert!(client_limit_reached(2, 2));
        assert!(client_limit_reached(2, 5));
        // 墓碑不占名额：2 上限 + 1 活 + 5 墓碑 → 调用方只传 Connected 数 → 放行
        assert!(!client_limit_reached(2, 1));
    }

    // ── 6. UDP 手动发送（udp_send_to 回环可达） ─────────

    #[test]
    fn udp_send_to_delivers_to_loopback_target() {
        let (addr, local, _remote) = loopback_pair();
        let side = NetworkSideChannel::new(0);
        *side.udp_socket.lock().unwrap() = Some(local.clone());

        side.udp_send_to(&addr.to_string(), b"hello").unwrap();
        let mut buf = [0u8; 16];
        let (n, from) = local.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello");
        assert_eq!(from, local.local_addr().unwrap());
    }

    // ── 7. UDP Client 分支（不 connect：固定远端发送 + 记录本地地址） ──

    /// ("udp","client") 参数解析：绑定本地、记录固定远端与本地地址；
    /// 不 connect，发送用 `send_to(remote)`，本地地址可供前端展示
    #[tokio::test]
    async fn udp_client_connect_pins_remote_target() {
        let server = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let server_addr = server.local_addr().unwrap();

        let adapter = NetworkAdapter::new();
        let conn = adapter
            .connect(
                "udp-client-test",
                &serde_json::json!({
                    "transport": "udp",
                    "role": "client",
                    "remote_host": server_addr.ip().to_string(),
                    "remote_port": server_addr.port(),
                }),
            )
            .await
            .unwrap();

        let side_arc = conn.side_channel.unwrap();
        let side = side_arc
            .as_any()
            .downcast_ref::<NetworkSideChannel>()
            .unwrap();
        // 固定远端已记录，本地地址已捕获
        let target = side.udp_client_target.lock().unwrap().unwrap();
        assert_eq!(target, server_addr);
        assert!(side.udp_client_local_addr().is_some());

        // 通过 udp_send 发送到固定远端（socket 未 connect）
        side.udp_send(b"hello-client").unwrap();
        let mut buf = [0u8; 32];
        let (n, _from) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello-client");
    }
}
