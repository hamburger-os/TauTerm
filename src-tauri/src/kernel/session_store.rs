//! 会话存储
//!
//! 管理所有活跃终端会话的 I/O 生命周期。
//! 基于 `Channel` trait 和 `IoLoopCmd`，与协议无关。
//!
//! ## 架构
//!
//! SessionStore
//! ├── sessions: HashMap<TabId, ActiveSessionHandle>
//! ├── active_id: Option<TabId>
//! └── tab_order: Vec<TabId>
//!
//! ActiveSessionHandle
//! ├── id: TabId (uuid v4)
//! ├── name: String
//! ├── write_tx: SyncSender<IoLoopCmd>
//! ├── io_thread: Option<JoinHandle>
//! └── state: SessionState

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use crate::channel::Channel;
use crate::channel::io_loop::{IoLoopCmd, spawn_sync_io_loop};
use crate::channel::async_io_loop::spawn_async_io_loop;
use crate::kernel::comm_handle::{CommHandle, DataCallback};
use crate::kernel::data_batcher::DataBatcher;
use crate::kernel::log_engine::{DataDirection, DataLogEntry, LogEntry};
use crate::kernel::plugin_adapter::{ChannelKind, ProtocolConnection, SideChannel};
use crate::kernel::script_engine::{ScriptCmd, spawn_script_thread};
use crate::virtual_port::bridge::VirtualPortBridge;
use crate::virtual_port::backend::PortPair;

pub type TabId = String;

/// I/O 任务句柄枚举
///
/// - `Sync`：由 `spawn_sync_io_loop` 返回的 std::thread 句柄（串口）
/// - `Async`：由 `spawn_async_io_loop` 返回的 tokio task 句柄（SSH）
pub enum IoTaskHandle {
    Sync(std::thread::JoinHandle<()>),
    Async(tokio::task::JoinHandle<()>),
}

/// 会话状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Transferring,
}

/// 子连接关闭的第二阶段句柄（锁外 join）。
///
/// 由 [`SessionStore::close_sub_connection`] 在持锁阶段返回。
/// **不变式**：持有本句柄期间不得获取 session_store 锁 —— I/O 线程的
/// on_disconnect 回调可能正在等待该锁，join 前取锁 = 死锁。
///
/// - `Sync` I/O 线程：读超时 50ms 轮询 + Shutdown 命令驱动退出，join 快速返回；
/// - `Async` I/O task：在 tokio runtime 中限时 join，超时 abort（远程僵死防御）；
/// - 脚本线程：协作式关闭标志 + Shutdown 命令，join 快速返回。
pub struct SubConnectionCleanup {
    channel_id: String,
    io_thread: Option<IoTaskHandle>,
    script_thread: Option<std::thread::JoinHandle<()>>,
}

impl SubConnectionCleanup {
    /// 阶段 2：在 **锁外** join I/O 线程/任务与脚本线程，等待资源真实释放。
    ///
    /// 必须在释放 session_store 锁后调用（见类型级不变式）。
    pub fn join(self) {
        let ch_id = self.channel_id;
        if let Some(io_thread) = self.io_thread {
            match io_thread {
                IoTaskHandle::Sync(thread) => {
                    // 读超时 50ms 轮询 + Shutdown 命令驱动退出，join 快速返回
                    let _ = thread.join();
                }
                IoTaskHandle::Async(mut task) => {
                    // 与 close_session 相同的双场景处理：
                    // 1. tokio runtime 内（Tauri async 命令）→ block_in_place + block_on
                    // 2. runtime 外（同步命令 / Drop 清理）→ 临时 runtime
                    //    限时 join：远程 TCP 僵死时超时 abort，防御性释放
                    let wait = async {
                        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
                            Ok(_) => log::debug!("子连接 I/O task 已清理: {}", ch_id),
                            Err(_elapsed) => {
                                task.abort();
                                // 等待 abort 完成，确保 Drop 析构执行
                                let _ = tokio::time::timeout(
                                    Duration::from_secs(5), task,
                                ).await;
                                log::warn!("子连接 I/O task 已强制中止: {}", ch_id);
                            }
                        }
                    };
                    match tokio::runtime::Handle::try_current() {
                        Ok(handle) => {
                            tokio::task::block_in_place(|| handle.block_on(wait));
                        }
                        Err(_) => match tokio::runtime::Runtime::new() {
                            Ok(rt) => rt.block_on(wait),
                            Err(e) => log::warn!(
                                "无法创建临时 tokio runtime 清理子连接 I/O task: {} ({})",
                                e, ch_id
                            ),
                        },
                    }
                }
            }
        }
        if let Some(thread) = self.script_thread {
            let _ = thread.join();
        }
    }
}

/// I/O 统计快照
#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub tab_id: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connected_at: Option<u64>,
}

/// 会话内对端信息（网络调试会话的自定义视图用）
#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub name: String,
    pub addr: String,
    /// 本端地址（TCP client 连接后本机分配的 ip:port，可与服务端对端条目对应）
    pub local_addr: String,
    pub state: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connected_at: Option<u64>,
}

/// 子连接句柄（SSH 多连接 / 网络调试会话多对端）。
///
/// 每个子连接有独立的 I/O loop、write channel 和统计信息。
/// 关闭子连接不影响父 session，关闭父 session 级联清理所有子连接。
///
/// 两种用途：
/// - SSH 通道（`tabbed = true`）：前端表现为独立标签页；
/// - 网络调试对端（`tabbed = false`）：会话内实体，前端在自定义视图中展示。
pub struct SubConnection {
    /// 子连接唯一 ID（UUID v4）
    pub id: TabId,
    /// 显示名称（"Shell 1" / "Peer 1" / 对端地址）
    pub name: String,
    /// 发送 I/O 命令的 channel
    pub write_tx: mpsc::SyncSender<IoLoopCmd>,
    /// I/O 取消信号
    pub io_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// I/O 线程/任务句柄
    pub io_thread: Option<IoTaskHandle>,
    /// 当前状态
    pub state: SessionState,
    /// 连接建立时间戳
    pub connected_at: Option<u64>,
    /// 统计采集器取消标志
    pub stats_cancel_flag: Option<Arc<AtomicBool>>,
    /// 通道自动编号（从 0 开始）
    pub channel_index: u32,
    /// 发送字节计数（网络调试对端统计用）
    pub tx_bytes: Arc<AtomicU64>,
    /// 接收字节计数（网络调试对端统计用）
    pub rx_bytes: Arc<AtomicU64>,
    /// 通信抽象句柄（网络调试对端拥有各自实例，使自动应答/脚本按对端生效）
    pub comm_handle: Option<Arc<dyn CommHandle>>,
    /// 脚本引擎线程的命令发送端（对端级脚本/自动应答）
    pub script_tx: Option<mpsc::SyncSender<ScriptCmd>>,
    /// 脚本引擎线程句柄
    pub script_thread: Option<std::thread::JoinHandle<()>>,
    /// 脚本线程的协作式关闭标志
    pub script_shutdown: Option<Arc<AtomicBool>>,
    /// 是否为独立标签页子连接（SSH 通道 = true）；false = 会话内对端（网络调试，不占标签页）
    pub tabbed: bool,
    /// 对端地址描述（网络调试：`ip:port`；SSH：无）
    pub peer_addr: Option<String>,
    /// 本端地址（网络调试：TCP client 的连接本地地址 / server 的监听地址）
    pub local_addr: Option<String>,
}

/// 单个会话句柄（协议无关）
pub struct ActiveSessionHandle {
    pub id: TabId,
    pub name: String,
    /// 写入通道（None = 容器会话，不可直接写入；I/O 必须通过子连接）
    pub write_tx: Option<mpsc::SyncSender<IoLoopCmd>>,
    pub io_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub cancel_transfer_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub io_thread: Option<IoTaskHandle>,
    pub state: SessionState,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    /// 传输完成后归还 Channel 给 I/O 线程的发送端
    pub channel_return_tx: Option<mpsc::SyncSender<Box<dyn Channel>>>,
    pub tx_bytes: Arc<AtomicU64>,
    pub rx_bytes: Arc<AtomicU64>,
    pub connected_at: Option<u64>,
    pub stats_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// 统计采集器的取消标志（用于无 tokio 的 std thread 轮询）
    pub stats_cancel_flag: Option<Arc<AtomicBool>>,
    /// 是否启用文件传输子系统（默认 true）
    pub transfer_enabled: bool,
    /// 文件传输协议（ymodem / xmodem / zmodem）
    pub transfer_protocol: Option<String>,
    /// 是否启用发送栏（默认 true）
    pub send_bar_enabled: bool,
    /// 虚拟端口桥接线程（None = 未启用或未创建）
    pub virtual_port_bridge: Option<VirtualPortBridge>,
    /// 当前会话的虚拟端口对列表
    pub virtual_port_pairs: Vec<PortPair>,
    /// 通信抽象句柄（供脚本引擎使用）
    pub comm_handle: Option<Arc<dyn CommHandle>>,
    /// 脚本引擎线程的命令发送端
    pub script_tx: Option<mpsc::SyncSender<ScriptCmd>>,
    /// 脚本引擎线程句柄
    pub script_thread: Option<std::thread::JoinHandle<()>>,
    /// 脚本线程的协作式关闭标志（停止时置位，使 Lua sleep 分片中断，join 不长时阻塞）
    pub script_shutdown: Option<Arc<AtomicBool>>,
    /// 协议侧通道资源（如 SSH Session 供文件传输复用）。
    /// 由 `ProtocolConnection::side_channel` 提供，None 表示无辅助资源。
    /// 使用 `Arc<dyn SideChannel>` 以允许多个命令并发访问同一资源。
    pub side_channel: Option<Arc<dyn SideChannel>>,
    /// 侧通道传输取消标志（传输进行中置位，传输循环每块检查）。
    /// None 表示当前无传输进行。由传输命令在传输前设置，传输结束后置 None。
    pub transfer_cancel: Option<Arc<AtomicBool>>,
    /// 侧通道异步传输任务的 JoinHandle 集合。
    /// 关闭会话时 join 所有 handle，确保传输 task 的 Drop 清理逻辑执行完毕，
    /// 避免残留半成品文件（上传残留远端，下载残留本地）。
    pub transfer_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// 会话关闭后、资源完全释放前所需的额外等待时间（由协议适配器提供）。
    /// `close_session()` 在 join I/O 线程后据此睡眠，避免内核硬编码协议特定逻辑。
    pub teardown_delay: Duration,
    /// 子连接列表（SSH 多连接：每个 PTY channel 一个 SubConnection）
    pub sub_connections: Vec<SubConnection>,
}

impl SubConnection {
    /// 创建一个空的子连接（由 SessionStore 填充字段）
    pub fn new(
        id: TabId,
        name: String,
        write_tx: mpsc::SyncSender<IoLoopCmd>,
        io_thread: IoTaskHandle,
        channel_index: u32,
        io_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Self {
        Self {
            id,
            name,
            write_tx,
            io_cancel_tx,
            io_thread: Some(io_thread),
            state: SessionState::Connected,
            connected_at: None, // 由调用方设置（SubConnection 自身不感知真实连接时刻）
            stats_cancel_flag: None,
            channel_index,
            tx_bytes: Arc::new(AtomicU64::new(0)),
            rx_bytes: Arc::new(AtomicU64::new(0)),
            comm_handle: None,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            tabbed: true, // SSH 通道默认为独立标签页
            peer_addr: None,
            local_addr: None,
        }
    }
}

impl ActiveSessionHandle {
    pub fn virtual_port_enabled(&self) -> bool {
        self.params.get("virtual_port_enabled")
            .and_then(|v| v.as_bool()).unwrap_or(false)
    }
    pub fn virtual_port_count(&self) -> u32 {
        self.params.get("virtual_port_count")
            .and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(0)
    }
}

impl Drop for ActiveSessionHandle {
    fn drop(&mut self) {
        // 安全网：如果 close_session() 未被正确调用，确保桥接线程
        // 和 I/O 线程收到取消信号。
        // 注意：不在此处调用 bridge.shutdown() — 它会阻塞 join 最多 5 秒，
        // 可能在 panic unwind 中触发 double-panic，或在持有 SessionStore Mutex
        // 时阻塞调用线程。改为在独立线程中关闭，close_session() 正常路径
        // 中已正确调用 shutdown()。
        if let Some(bridge) = self.virtual_port_bridge.take() {
            log::warn!(
                "ActiveSessionHandle '{}' dropped without proper close_session — \
                 shutting down bridge in detached thread",
                self.id
            );
            std::thread::spawn(move || { bridge.shutdown(); });
        }
        if !self.virtual_port_pairs.is_empty() {
            log::warn!(
                "ActiveSessionHandle '{}' dropped with {} virtual port pair(s) still registered \
                 — these may be cleaned up on next TauTerm startup",
                self.id,
                self.virtual_port_pairs.len()
            );
        }
        if let Some(tx) = self.io_cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        if let Some(ref flag) = self.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        // 脚本引擎线程清理（先置协作式关闭标志，使长睡眠及时中断）
        if let Some(ref flag) = self.script_shutdown {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = self.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(thread) = self.script_thread.take() {
            let _ = thread.join();
        }
        // ── 子连接清理 ──
        // 发送取消信号和关闭命令，但不 join I/O task（在 Drop 中 join 可能死锁）。
        for sub in self.sub_connections.iter_mut() {
            if let Some(ref flag) = sub.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = sub.io_cancel_tx.take() {
                let _ = tx.send(());
            }
            let _ = sub.write_tx.send(IoLoopCmd::Shutdown);
        }
        if !self.sub_connections.is_empty() {
            log::warn!(
                "ActiveSessionHandle '{}' dropped with {} sub-connection(s) still active — \
                 I/O tasks will exit on Shutdown signal",
                self.id,
                self.sub_connections.len()
            );
        }
    }
}

/// 会话存储
pub struct SessionStore {
    sessions: HashMap<TabId, ActiveSessionHandle>,
    active_id: Option<TabId>,
    tab_order: Vec<TabId>,
    max_sessions: usize,
    /// 持久化会话名称映射，会话从 HashMap 移除后仍保留，
    /// 用于在错误消息中显示用户友好的名称而非原始 UUID。
    /// 通过 `removed_order` 队列进行 LRU 淘汰，防止无限增长。
    session_names: HashMap<TabId, String>,
    /// 关闭顺序队列，用于淘汰 `session_names` 中最旧的已删除会话条目
    removed_order: VecDeque<TabId>,
}

/// 持久化会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub name: String,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub timestamp: u64,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
    pub virtual_port_enabled: bool,
    pub virtual_port_count: u32,
}

/// 全局文件锁 — 保护 sessions.json 的 read-modify-write 操作。
/// 目前 Tauri 命令串行执行，但该锁为未来的并行调用（如批量删除）提供安全保证。
static SESSIONS_FILE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl SessionStore {
    /// 保留最近关闭会话名称的数量上限（LRU 淘汰）
    const MAX_REMOVED_SESSION_NAMES: usize = 50;

    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            active_id: None,
            tab_order: Vec::new(),
            max_sessions: 10,
            session_names: HashMap::new(),
            removed_order: VecDeque::new(),
        }
    }

    /// 创建新会话（使用协议适配器返回的 `ProtocolConnection`）
    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &mut self,
        name: &str,
        plugin_id: &str,
        endpoint: &str,
        params: serde_json::Value,
        conn: ProtocolConnection,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
        transfer_enabled: bool,
        transfer_protocol: Option<String>,
        send_bar_enabled: bool,
        // 可选：传入已有的 session_id 以原地重连（保留 UUID）
        id_override: Option<String>,
    ) -> Result<TabId, String> {
        // 若以已有 ID 重连，先清理上一个 Disconnected 僵尸句柄
        if let Some(ref raw) = id_override {
            if let Some(zombie) = self.sessions.get(raw) {
                if zombie.state == SessionState::Disconnected {
                    self.sessions.remove(raw);
                }
            }
        }

        // 清理所有僵尸句柄，以免占用 max_sessions 名额
        self.purge_zombies();

        if self.sessions.len() >= self.max_sessions {
            return Err(format!("已达到最大会话数限制 ({})", self.max_sessions));
        }

        // 验证 id_override 为合法 UUID，防止任意字符串导致 HashMap 键冲突与资源泄漏
        let id = if let Some(ref raw) = id_override {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            raw.clone()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let tab_name = if name.is_empty() {
            format!("{} @ {}", plugin_id, endpoint)
        } else {
            name.to_string()
        };

        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let (write_tx, write_rx) = mpsc::sync_channel::<IoLoopCmd>(256);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        // 通信抽象句柄：协议可自带（如未来 SSH 专用实现），否则统一使用 SerialCommHandle。
        // 当前所有协议的 CommHandle 均仅包装 write_tx，功能等价，故统一降级。
        // 传入会话编码：使 Lua `send_text` 文本路径按会话编码转码（与前端一致）。
        let comm_handle: Arc<dyn CommHandle> = conn.comm_handle
            .unwrap_or_else(|| {
                let encoding = params
                    .get("encoding")
                    .and_then(|v| v.as_str())
                    .unwrap_or("utf-8")
                    .to_string();
                Arc::new(crate::channel::serial_comm::SerialCommHandle::new(write_tx.clone(), encoding))
            });

        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        let tx_clone = tx_bytes.clone();
        let rx_clone = rx_bytes.clone();

        let sid = id.clone();

        // 包装 on_data 闭包，使每条接收数据通过 CommHandle 扇出
        // 脚本引擎等消费者通过 CommHandle::on_receive() 注册回调，
        // 无需直接调用 SessionStore::feed_script_data()
        let comm_for_fanout = comm_handle.clone();
        let wrapped_on_data = Box::new(move |session_id: String, data: Vec<u8>| {
            // 先借用扇出给脚本引擎等消费者，再把所有权移交终端/日志的 on_data，
            // 省去每包一次 data.clone()（即便无脚本运行也在拷贝）
            comm_for_fanout.notify_receive(&data);
            on_data(session_id, data);
        });

        let io_handle = match conn.channel {
            Some(ChannelKind::Sync(sync_channel)) => {
                IoTaskHandle::Sync(spawn_sync_io_loop(
                    sync_channel, sid.clone(), wrapped_on_data, on_disconnect, write_rx, cancel_rx,
                    tx_clone, rx_clone,
                ))
            }
            Some(ChannelKind::Async(async_channel)) => {
                IoTaskHandle::Async(spawn_async_io_loop(
                    async_channel, sid.clone(), wrapped_on_data, on_disconnect, write_rx, cancel_rx,
                    tx_clone, rx_clone,
                ))
            }
            None => {
                return Err(
                    "create_session requires an I/O channel; \
                     use create_container_session for headless (no terminal I/O) protocols"
                        .into(),
                );
            }
        };

        // 启动 StatsCollector（使用 std thread + AtomicBool 取消，无需 tokio runtime）
        let stats_cancel_flag = Arc::new(AtomicBool::new(false));
        Self::start_stats_collector(
            app_handle.clone(),
            id.clone(),
            tx_bytes.clone(),
            rx_bytes.clone(),
            connected_at,
            stats_cancel_flag.clone(),
        );

        // 保存名称副本，后续用于错误消息（handle 会消耗 tab_name）
        let session_name_for_map = tab_name.clone();

        let handle = ActiveSessionHandle {
            id: id.clone(),
            name: tab_name,
            write_tx: Some(write_tx),
            io_cancel_tx: Some(cancel_tx),
            cancel_transfer_tx: None,
            io_thread: Some(io_handle),
            state: SessionState::Connected,
            plugin_id: plugin_id.to_string(),
            endpoint: endpoint.to_string(),
            params,
            channel_return_tx: None,
            tx_bytes,
            rx_bytes,
            connected_at,
            stats_cancel_tx: None,
            stats_cancel_flag: Some(stats_cancel_flag),
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            virtual_port_bridge: None,
            virtual_port_pairs: Vec::new(),
            comm_handle: Some(comm_handle),
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            side_channel: conn.side_channel,
            transfer_cancel: None,
            transfer_tasks: Vec::new(),
            teardown_delay: conn.teardown_delay,
            sub_connections: Vec::new(),
        };

        // 防御性检查：若 id_override 指向的会话已存在且未被正确关闭，
        // 先清理旧会话，防止静默覆盖导致 I/O 线程、串口句柄、定时器等资源泄漏。
        // 显式 drop() 确保 SessionHandle 的 Drop 实现（关闭 I/O 线程/句柄）
        // 在新 session 插入前执行，避免新旧会话并发持有同一硬件资源。
        if let Some(mut old_handle) = self.sessions.remove(&id) {
            // 先关闭虚拟端口桥接再 drop，防止 JoinHandle detach 泄漏线程
            if let Some(bridge) = old_handle.virtual_port_bridge.take() {
                bridge.shutdown();
                log::warn!(
                    "create_session 中关闭了残留桥接线程 (session: {}) — 调用方应预先调用 close_session",
                    id
                );
            }
            drop(old_handle);
        }
        // 若 tab_order 中已有此 ID（例如前端未正确同步），移除旧条目
        self.tab_order.retain(|tid| tid != &id);

        self.sessions.insert(id.clone(), handle);
        self.tab_order.push(id.clone());
        self.session_names.insert(id.clone(), session_name_for_map);
        self.active_id = Some(id.clone());

        Ok(id)
    }

    /// 清理所有 Disconnected 状态的僵尸会话，以免占用 max_sessions 名额。
    fn purge_zombies(&mut self) {
        let zombie_ids: Vec<String> = self.sessions.iter()
            .filter(|(_, h)| h.state == SessionState::Disconnected)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &zombie_ids {
            self.sessions.remove(id);
        }
        if !zombie_ids.is_empty() {
            log::info!("已清理 {} 个僵尸会话", zombie_ids.len());
        }
    }

    /// 创建容器会话（SSH 父容器，无 I/O loop）。
    ///
    /// 仅持有 SSH side_channel 和元数据，不创建 I/O 线程。
    /// 实际终端通过 `add_sub_connection` 添加。
    pub fn create_container_session(
        &mut self,
        name: &str,
        plugin_id: &str,
        endpoint: &str,
        params: serde_json::Value,
        side_channel: Option<Arc<dyn SideChannel>>,
        comm_handle: Option<Arc<dyn CommHandle>>,
        transfer_enabled: bool,
        transfer_protocol: Option<String>,
        send_bar_enabled: bool,
        id_override: Option<String>,
    ) -> Result<TabId, String> {
        let id = if let Some(ref raw) = id_override {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            raw.clone()
        } else {
            uuid::Uuid::new_v4().to_string()
        };

        // 若以已有 ID 重连，先清理上一个 Disconnected 僵尸容器会话
        if let Some(ref raw) = id_override {
            if let Some(zombie) = self.sessions.get(raw) {
                if zombie.state == SessionState::Disconnected {
                    self.sessions.remove(raw);
                }
            }
        }

        // 清理所有僵尸句柄，以免占用 max_sessions 名额
        self.purge_zombies();

        if self.sessions.len() >= self.max_sessions {
            return Err(format!("已达到最大会话数限制 ({})", self.max_sessions));
        }

        // 容器会话不直接接受 I/O 数据 — write_tx 为 None，
        // 所有读写必须通过子连接路由。
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));

        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let session_name = if name.is_empty() {
            format!("{} @ {}", plugin_id, endpoint)
        } else {
            name.to_string()
        };

        let handle = ActiveSessionHandle {
            id: id.clone(),
            name: session_name.clone(),
            write_tx: None,
            io_cancel_tx: None,
            cancel_transfer_tx: None,
            io_thread: None,
            state: SessionState::Connected,
            plugin_id: plugin_id.to_string(),
            endpoint: endpoint.to_string(),
            params,
            channel_return_tx: None,
            tx_bytes,
            rx_bytes,
            connected_at,
            stats_cancel_tx: None,
            stats_cancel_flag: None,
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            virtual_port_bridge: None,
            virtual_port_pairs: Vec::new(),
            comm_handle,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            side_channel,
            transfer_cancel: None,
            transfer_tasks: Vec::new(),
            teardown_delay: Duration::ZERO,
            sub_connections: Vec::new(),
        };

        self.sessions.insert(id.clone(), handle);
        self.tab_order.push(id.clone());
        self.session_names.insert(id.clone(), session_name);
        self.active_id = Some(id.clone());
        Ok(id)
    }

    /// # 调用方约定
    ///
    /// **调用方必须在此调用之前 clone `virtual_port_pairs`**（如需访问），
    /// 因为此方法一开始就 `sessions.remove(session_id)`，句柄随后被 drop。
    /// 参考 `disconnect_session` 在 `commands.rs` 中的用法。
    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
        // 临时取出句柄以解除借用，关闭后再以 Disconnected 状态放回
        let mut handle = self.sessions.remove(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;

        // ── 先关闭所有子连接 ──
        // 采用 deferred-join 模式：发送取消信号后不在锁内 join I/O task，
        // 避免子连接 on_disconnect 回调尝试获取 session_store 锁时形成死锁。
        // Async I/O task 随 SubConnection drop 自然释放（tokio JoinHandle detach），
        // I/O task 会在处理完 Shutdown 后自行退出。
        {
            let mut subs = std::mem::take(&mut handle.sub_connections);
            for mut sub in subs.drain(..) {
                if let Some(ref flag) = sub.stats_cancel_flag {
                    flag.store(true, Ordering::SeqCst);
                }
                if let Some(tx) = sub.io_cancel_tx.take() {
                    let _ = tx.send(());
                }
                let _ = sub.write_tx.send(IoLoopCmd::Shutdown);
                // Async I/O task 的 JoinHandle 随 sub drop 自然释放（detach）。
                // tokio JoinHandle drop 不会 abort 任务，任务继续运行至完成。
                // I/O task 会在处理完 Shutdown 后自行退出。
                // on_disconnect 回调由 mark_disconnected 安全处理（不 join）。
            }
        }
        // 保存名称（create_session 中已保存，此处作为保障）
        self.session_names.insert(session_id.to_string(), handle.name.clone());
        // LRU 淘汰：推入关闭队列，超出上限时移除最旧条目
        self.removed_order.push_back(session_id.to_string());
        while self.removed_order.len() > Self::MAX_REMOVED_SESSION_NAMES {
            if let Some(old_id) = self.removed_order.pop_front() {
                self.session_names.remove(&old_id);
            }
        }

        // ── 侧通道传输取消 ──
        // 若会话有进行中的文件传输，置位取消标志。传输线程在下次块检查时退出，
        // 其 RAII guard 的 drop 会调用 transfer_done
        // (对已移除的 session 是 no-op)。side_channel 通过 Arc clone 保持 SSH
        // Session 存活，直到传输线程退出，避免 use-after-free。
        if let Some(flag) = handle.transfer_cancel.take() {
            flag.store(true, Ordering::SeqCst);
            log::info!("已请求取消会话 {} 的进行中传输", session_id);
        }

        // 取消 journald 实时追踪（若已启动）
        crate::plugins::ssh::journald::stop_journald_stream(session_id);

        // 取消 journald 日志导出（若已启动，幂等操作）
        crate::plugins::ssh::journald::stop_journald_export(session_id);

        // 关闭脚本引擎（必须在 IoLoop 关闭前执行）
        if let Some(ref flag) = handle.script_shutdown {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = handle.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(thread) = handle.script_thread.take() {
            let _ = thread.join();
        }
        // 清理脚本引擎注册的接收回调，与 stop_script() 保持一致
        if let Some(comm) = &handle.comm_handle {
            comm.clear_receivers();
        }

        // 关闭虚拟端口桥接线程
        if let Some(bridge) = handle.virtual_port_bridge.take() {
            bridge.shutdown();
            log::info!("虚拟端口桥接已关闭 (session: {})", session_id);
        }

        // 取消正在进行的传输
        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        // 取消 StatsCollector（通过 AtomicBool 标志）
        if let Some(ref flag) = handle.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        // 释放 Channel 归还通道
        handle.channel_return_tx = None;

        // 发送取消信号
        if let Some(tx) = handle.io_cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(ref tx) = handle.write_tx {
            let _ = tx.send(IoLoopCmd::Shutdown);
        }
        match handle.io_thread.take() {
            Some(IoTaskHandle::Sync(thread)) => {
                let _ = thread.join();
            }
            Some(IoTaskHandle::Async(task)) => {
                // Join async I/O task。需在两种场景下均可工作：
                // 1. Tauri async 命令（已在 tokio runtime 中）→ block_in_place + block_on
                // 2. RunEvent::Exit / Drop 清理（不在 runtime 中，如 main 线程）→ 临时 runtime
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        tokio::task::block_in_place(|| {
                            let _ = handle.block_on(task);
                        });
                    }
                    Err(_) => {
                        // 不在 tokio runtime 中（如 main 线程 Drop 清理），
                        // 尝试创建临时 runtime 来 join async task。
                        // 资源耗尽时创建 runtime 可能失败，此时仅记录警告，
                        // task JoinHandle 随 drop 自然释放（best-effort 清理）。
                        match tokio::runtime::Runtime::new() {
                            Ok(rt) => {
                                let _ = rt.block_on(task);
                            }
                            Err(e) => {
                                log::warn!(
                                    "无法创建临时 tokio runtime 清理异步 I/O task: {}. \
                                     task handle 将被 drop（可能导致 SSH 会话未完全关闭）",
                                    e
                                );
                            }
                        }
                    }
                }
            }
            None => {}
        }

        // ── 等待进行中的侧通道传输完成 ──
        // 采用 mark_disconnected 中已验证的模式：drain handles 后在独立 task 中
        // 以超时方式 join，避免持锁阻塞（传输 task 完成时需要 session_store 锁来
        // 调用 transfer_done，若此处持锁 block_on 会形成循环死锁）。
        //
        // 需处理两种场景：
        // 1. Tauri async 命令（已在 tokio runtime 中）→ tokio::spawn fire-and-forget
        // 2. RunEvent::Exit / Drop 清理（不在 runtime 中）→ 跳过 join，task handle
        //    随 drop 自然释放（best-effort 清理，因为此时传输 cancel flag 已置位）。
        for task in handle.transfer_tasks.drain(..) {
            let sid = session_id.to_string();
            match tokio::runtime::Handle::try_current() {
                Ok(_) => {
                    tokio::spawn(async move {
                        match tokio::time::timeout(Duration::from_secs(5), task).await {
                            Ok(_) => {
                                log::debug!("传输 task 已清理 (session: {})", sid);
                            }
                            Err(_) => {
                                log::warn!("传输 task join 超时 (session: {})", sid);
                            }
                        }
                    });
                }
                Err(_) => {
                    log::warn!(
                        "无法 join 传输 task（无 tokio runtime），task handle 将被 drop (session: {})",
                        sid
                    );
                    // transfer cancel flag 已在上面置位，task handle 随 drop 自然释放
                }
            }
        }

        // 协议适配器声明的关闭后等待时间（如串口驱动释放端口），避免硬编码协议判断
        if !handle.teardown_delay.is_zero() {
            std::thread::sleep(handle.teardown_delay);
        }

        self.tab_order.retain(|id| id != session_id);
        if self.active_id.as_deref() == Some(session_id) {
            self.active_id = self.tab_order.first().cloned();
        }

        // 通知侧通道即将释放，执行清理（如中止 TFTP 后台线程）
        if let Some(ref sc) = handle.side_channel {
            sc.shutdown();
        }

        // 断开连接后释放侧通道资源（如 TFTP 的 UDP socket）
        handle.side_channel = None;
        // 以 Disconnected 状态放回 HashMap，使并发传输命令可获取到句柄并返回明确的"已断开"错误
        handle.state = SessionState::Disconnected;
        self.sessions.insert(session_id.to_string(), handle);

        Ok(())
    }

    /// 格式化"会话不存在"错误消息，优先使用已保存的会话名称
    pub(crate) fn session_not_found(&self, session_id: &str) -> String {
        let display_name = self.session_names
            .get(session_id)
            .map(|n| n.as_str())
            .unwrap_or(session_id);
        format!("会话 {} 不存在", display_name)
    }

    /// 切换到指定会话（支持子连接 ID）
    pub fn switch_active(&mut self, session_id: &str) -> Result<(), String> {
        // 检查是否是已知会话
        if let Some(h) = self.sessions.get(session_id) {
            if h.state == SessionState::Disconnected {
                return Err(self.session_not_found(session_id));
            }
            self.active_id = Some(session_id.to_string());
            return Ok(());
        }
        // 搜索子连接
        for handle in self.sessions.values() {
            if handle.sub_connections.iter().any(|s| s.id == session_id) {
                self.active_id = Some(session_id.to_string());
                return Ok(());
            }
        }
        Err(self.session_not_found(session_id))
    }

    /// 重命名会话
    pub fn rename_session(&mut self, session_id: &str, new_name: &str) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;
        handle.name = new_name.to_string();
        self.session_names.insert(session_id.to_string(), new_name.to_string());
        Ok(())
    }

    /// 标签页重排序
    pub fn reorder_tabs(&mut self, new_order: Vec<TabId>) -> Result<(), String> {
        for id in &new_order {
            if !self.sessions.contains_key(id) {
                return Err(self.session_not_found(id));
            }
        }
        self.tab_order = new_order;
        Ok(())
    }

    /// 向指定会话写入数据（支持子连接路由）
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        // 先尝试直接匹配
        if let Some(handle) = self.sessions.get(session_id) {
            // 普通会话（serial 等有 I/O loop 的会话）
            if let Some(ref tx) = handle.write_tx {
                return tx.send(IoLoopCmd::Write(data.to_vec()))
                    .map_err(|e| format!("写入通道错误: {}", e));
            }
            // 容器会话（SSH 父容器，无 I/O loop）— 不自动路由，调用方应传子连接 ID
            return Err(format!("容器会话 {} 不可直接写入，请指定子连接 ID", session_id));
        }
        // 搜索子连接
        for handle in self.sessions.values() {
            for sub in &handle.sub_connections {
                if sub.id == session_id {
                    return sub.write_tx.send(IoLoopCmd::Write(data.to_vec()))
                        .map_err(|e| format!("子连接写入通道错误: {}", e));
                }
            }
        }
        Err(self.session_not_found(session_id))
    }

    /// 添加子连接到父会话
    pub fn add_sub_connection(
        &mut self,
        parent_id: &str,
        sub: SubConnection,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(parent_id);
        let handle = self.sessions.get_mut(parent_id).ok_or(not_found)?;
        handle.sub_connections.push(sub);
        Ok(())
    }

    /// 注册一个"对端通道"到容器会话（网络调试等协议使用）。
    ///
    /// 与 `commands::create_ssh_sub_channel` 的通道创建流程等价，但面向会话内
    /// 多对端模型：对端不占独立标签页（`tabbed = false`），拥有独立的 I/O loop、
    /// 统计采集、CommHandle（自动应答/脚本按对端生效）与日志路由。
    ///
    /// 对端 I/O loop 断开时广播 `netdbg-peer-left` 事件；本方法广播
    /// `netdbg-peer-joined` 事件，供前端对端列表刷新。
    #[allow(clippy::too_many_arguments)]
    pub fn register_peer_channel(
        &mut self,
        app: &tauri::AppHandle,
        log_tx: mpsc::SyncSender<LogEntry>,
        parent_id: &str,
        peer_name: &str,
        peer_addr: &str,
        local_addr: &str,
        channel: ChannelKind,
        encoding: &str,
        data_mode: &str,
        peer_writers: Arc<Mutex<HashMap<String, mpsc::SyncSender<IoLoopCmd>>>>,
        container_receivers: Arc<Mutex<Vec<DataCallback>>>,
    ) -> Result<String, String> {
        let not_found = self.session_not_found(parent_id);
        if let Some(h) = self.sessions.get(parent_id) {
            if h.state != SessionState::Connected {
                return Err("父会话已断开，无法注册对端".to_string());
            }
        } else {
            return Err(not_found);
        }

        let channel_id = uuid::Uuid::new_v4().to_string();
        let (write_tx, write_rx) = mpsc::sync_channel::<IoLoopCmd>(256);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        let tx_clone = tx_bytes.clone();
        let rx_clone = rx_bytes.clone();

        // 登记对端写通道：容器级脚本引擎按「当前目标」路由/群发用
        if let Ok(mut writers) = peer_writers.lock() {
            writers.insert(channel_id.clone(), write_tx.clone());
        }

        // 对端级 CommHandle：文本路径按对端编码转码；同时是对端脚本引擎的扇出目标
        let comm_handle: Arc<dyn CommHandle> = Arc::new(
            crate::channel::serial_comm::SerialCommHandle::new(
                write_tx.clone(),
                encoding.to_string(),
            ),
        );

        let app_clone = app.clone();
        let batcher = DataBatcher::new(move |batched| {
            let _ = app_clone.emit("session-data", serde_json::json!({
                "session_id": batched.session_id,
                "data_b64": batched.data_b64,
            }));
        });
        let comm_for_fanout = comm_handle.clone();
        let encoding_owned = encoding.to_string();
        let data_mode_owned = data_mode.to_string();
        let on_data = Box::new(move |session_id: String, data: Vec<u8>| {
            comm_for_fanout.notify_receive(&data);
            // 容器级脚本引擎（TCP 目标路由/群发）也感知该对端数据
            if let Ok(rx) = container_receivers.lock() {
                for cb in rx.iter() {
                    cb(&data);
                }
            }
            let data_for_log = data.clone();
            batcher.push(session_id.clone(), data);
            let _ = log_tx.try_send(LogEntry::SessionData(DataLogEntry {
                session_id,
                direction: DataDirection::RX,
                data_mode: data_mode_owned.clone(),
                encoding: encoding_owned.clone(),
                payload: data_for_log,
                timestamp: chrono::Local::now(),
            }));
        });

        let app_disconnect = app.clone();
        let pid = parent_id.to_string();
        let ch_id = channel_id.clone();
        let ch_id_for_evt = ch_id.clone();
        let on_disconnect: Box<dyn Fn(String) + Send> = Box::new(move |_channel_id| {
            // 对端退出：从容器写通道注册表移除，避免群发写入失效通道
            if let Ok(mut writers) = peer_writers.lock() {
                writers.remove(&ch_id_for_evt);
            }
            // I/O 线程退出路径：先持锁落状态（Disconnected + 停 stats collector），
            // 再发事件。锁内不 join 任何线程（mark_sub_disconnected 保证）。
            let mut final_tx: Option<u64> = None;
            let mut final_rx: Option<u64> = None;
            if let Ok(mut store) = app_disconnect
                .state::<crate::AppState>()
                .session_store
                .lock()
            {
                store.mark_sub_disconnected(&pid, &ch_id_for_evt);
                // R4: 附带最终统计，前端据此更新对端末值（stats collector 已停）
                if let Some(h) = store.get_session(&pid) {
                    if let Some(sub) = h.sub_connections.iter().find(|s| s.id == ch_id_for_evt) {
                        final_tx = Some(sub.tx_bytes.load(Ordering::Relaxed));
                        final_rx = Some(sub.rx_bytes.load(Ordering::Relaxed));
                    }
                }
            }
            let _ = app_disconnect.emit("netdbg-peer-left", serde_json::json!({
                "session_id": pid,
                "peer_id": ch_id_for_evt,
                "tx_bytes": final_tx,
                "rx_bytes": final_rx,
            }));
        });

        let io_handle = match channel {
            ChannelKind::Sync(sync_channel) => IoTaskHandle::Sync(spawn_sync_io_loop(
                sync_channel, ch_id.clone(), on_data, on_disconnect, write_rx, cancel_rx,
                tx_clone, rx_clone,
            )),
            ChannelKind::Async(async_channel) => IoTaskHandle::Async(spawn_async_io_loop(
                async_channel, ch_id.clone(), on_data, on_disconnect, write_rx, cancel_rx,
                tx_clone, rx_clone,
            )),
        };

        let stats_cancel_flag = Arc::new(AtomicBool::new(false));
        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        Self::spawn_stats_collector(
            app.clone(),
            channel_id.clone(),
            tx_bytes.clone(),
            rx_bytes.clone(),
            connected_at,
            stats_cancel_flag.clone(),
        );

        // 对端序号仅统计非标签页子连接（网络对端），SSH 通道不占号
        let (actual_index, actual_name) = {
            let handle = self.sessions.get_mut(parent_id).ok_or(not_found.clone())?;
            let idx = handle.sub_connections.iter()
                .filter(|s| !s.tabbed)
                .count() as u32;
            let name = if peer_name.is_empty() {
                format!("Peer {}", idx + 1)
            } else {
                peer_name.to_string()
            };
            (idx, name)
        };

        let sub = SubConnection {
            id: channel_id.clone(),
            name: actual_name.clone(),
            write_tx,
            io_cancel_tx: Some(cancel_tx),
            io_thread: Some(io_handle),
            state: SessionState::Connected,
            connected_at,
            stats_cancel_flag: Some(stats_cancel_flag),
            channel_index: actual_index,
            tx_bytes,
            rx_bytes,
            comm_handle: Some(comm_handle),
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            tabbed: false,
            peer_addr: Some(peer_addr.to_string()),
            local_addr: Some(local_addr.to_string()),
        };

        {
            let handle = self.sessions.get_mut(parent_id).ok_or(not_found)?;
            handle.sub_connections.push(sub);
        }

        let _ = app.emit("netdbg-peer-joined", serde_json::json!({
            "session_id": parent_id,
            "peer_id": channel_id,
            "peer_name": actual_name,
            "peer_addr": peer_addr,
            "local_addr": local_addr,
        }));
        Ok(channel_id)
    }

    /// 查找子连接（对端）所属的父会话与下标
    fn find_sub_connection_index(&self, channel_id: &str) -> Option<(TabId, usize)> {
        for (pid, handle) in self.sessions.iter() {
            for (i, sub) in handle.sub_connections.iter().enumerate() {
                if sub.id == channel_id {
                    return Some((pid.clone(), i));
                }
            }
        }
        None
    }

    /// 列出容器会话内所有对端（网络调试视图初始化用）
    pub fn list_peers(&self, parent_id: &str) -> Vec<PeerInfo> {
        let mut out = Vec::new();
        if let Some(handle) = self.sessions.get(parent_id) {
            for sub in &handle.sub_connections {
                if sub.tabbed {
                    continue;
                }
                out.push(PeerInfo {
                    peer_id: sub.id.clone(),
                    name: sub.name.clone(),
                    addr: sub.peer_addr.clone().unwrap_or_default(),
                    local_addr: sub.local_addr.clone().unwrap_or_default(),
                    state: match sub.state {
                        SessionState::Connected => "connected".into(),
                        SessionState::Connecting => "connecting".into(),
                        SessionState::Disconnected => "disconnected".into(),
                        SessionState::Transferring => "transferring".into(),
                    },
                    tx_bytes: sub.tx_bytes.load(Ordering::Relaxed),
                    rx_bytes: sub.rx_bytes.load(Ordering::Relaxed),
                    connected_at: sub.connected_at,
                });
            }
        }
        out
    }

    /// 获取通信句柄（支持对端路由）。
    ///
    /// 先尝试匹配会话；否则搜索对端（网络调试）。对端拥有各自的 CommHandle，
    /// 使文本转码、脚本、自动应答按对端生效。
    pub fn get_comm_handle_for(&self, session_id: &str) -> Option<Arc<dyn CommHandle>> {
        if let Some(handle) = self.sessions.get(session_id) {
            return handle.comm_handle.clone();
        }
        for handle in self.sessions.values() {
            for sub in &handle.sub_connections {
                if sub.id == session_id {
                    return sub.comm_handle.clone();
                }
            }
        }
        None
    }

    /// 关闭单个子连接（两段式）。
    ///
    /// **阶段 1（本方法，持 store 锁）**：向脚本引擎 / 统计采集器 / I/O loop
    /// 发送全部关闭信号，并从 `sub_connections` 移除，返回待 join 的句柄。
    /// **不在锁内 join** —— I/O 线程退出路径可能触发 on_disconnect 回调，
    /// 回调需获取本 store 锁，持锁 join 会形成循环死锁。
    ///
    /// **阶段 2（调用方，锁外）**：调用 [`SubConnectionCleanup::join`]。
    ///
    /// 返回 `(是否为最后一个子连接, 清理句柄)`；若为最后一个子连接，
    /// 调用方应级联关闭父会话。
    pub fn close_sub_connection(
        &mut self,
        parent_id: &str,
        channel_id: &str,
    ) -> Result<(bool, SubConnectionCleanup), String> {
        let not_found = self.session_not_found(parent_id);
        let handle = self.sessions.get_mut(parent_id).ok_or(not_found)?;

        // 找到并移除目标子连接
        let idx = handle.sub_connections.iter()
            .position(|s| s.id == channel_id)
            .ok_or_else(|| format!("子连接 {} 在会话 {} 中不存在", channel_id, parent_id))?;

        let mut sub = handle.sub_connections.remove(idx);

        // ── 阶段 1a：对端级脚本引擎信号（网络调试对端；SSH 通道此处为 None 空操作）──
        // 锁内仅置协作式关闭标志 + 发 Shutdown 命令，线程句柄留待锁外 join
        if let Some(flag) = sub.script_shutdown.take() {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = sub.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        let script_thread = sub.script_thread.take();
        if let Some(comm) = &sub.comm_handle {
            comm.clear_receivers();
        }

        // ── 阶段 1b：统计采集器 + I/O loop 关闭信号 ──
        if let Some(ref flag) = sub.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = sub.io_cancel_tx.take() {
            let _ = tx.send(());
        }
        let _ = sub.write_tx.send(IoLoopCmd::Shutdown);
        let io_thread = sub.io_thread.take();

        // 是否最后一个子连接（由调用方决定是否级联断开父会话；网络调试不级联）
        let is_last = handle.sub_connections.is_empty();
        if is_last {
            log::info!("最后一个子连接已关闭 (parent: {})", parent_id);
        }
        Ok((
            is_last,
            SubConnectionCleanup {
                channel_id: channel_id.to_string(),
                io_thread,
                script_thread,
            },
        ))
    }

    /// 获取会话的 side_channel（用于 SSH 多连接复用）
    pub fn get_side_channel(&self, session_id: &str) -> Option<Arc<dyn SideChannel>> {
        self.sessions.get(session_id)
            .and_then(|h| h.side_channel.clone())
    }

    /// 解析 session_id，返回实际的顶层会话 ID。
    ///
    /// 如果 session_id 是子连接，返回其父会话的 ID；否则返回自身。
    pub fn resolve_parent_id(&self, session_id: &str) -> Option<String> {
        if self.sessions.contains_key(session_id) {
            return Some(session_id.to_string());
        }
        self.find_parent_of_channel(session_id)
    }

    /// 获取 side_channel（支持子连接路由）。
    ///
    /// 先尝试直接查找 session_id 的 side_channel；若未找到，
    /// 则通过子连接的父会话查找。SSH 父会话持有 `russh::client::Handle`，
    /// 供 SFTP 文件服务和 journald 日志查看器复用。
    pub fn get_side_channel_for(&self, session_id: &str) -> Option<Arc<dyn SideChannel>> {
        if let Some(sc) = self.sessions.get(session_id).and_then(|h| h.side_channel.clone()) {
            return Some(sc);
        }
        let parent_id = self.find_parent_of_channel(session_id)?;
        self.sessions.get(&parent_id).and_then(|h| h.side_channel.clone())
    }

    /// 获取 write_tx（支持子连接路由）。
    ///
    /// 先尝试直接查找；若未找到则搜索子连接。
    /// 用于 `resize_pty` 等需要将命令写入正确 I/O loop 的场景。
    pub fn get_write_tx(&self, session_id: &str) -> Option<&mpsc::SyncSender<IoLoopCmd>> {
        if let Some(handle) = self.sessions.get(session_id) {
            return handle.write_tx.as_ref();
        }
        for handle in self.sessions.values() {
            for sub in &handle.sub_connections {
                if sub.id == session_id {
                    return Some(&sub.write_tx);
                }
            }
        }
        None
    }

    /// 查找子连接所属的父会话 ID
    pub fn find_parent_of_channel(&self, channel_id: &str) -> Option<String> {
        for (pid, handle) in self.sessions.iter() {
            if handle.sub_connections.iter().any(|s| s.id == channel_id) {
                return Some(pid.clone());
            }
        }
        None
    }

    /// 获取所有活跃会话的 ID 列表（含子连接）
    pub fn all_session_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sessions.keys().cloned().collect();
        for handle in self.sessions.values() {
            for sub in &handle.sub_connections {
                ids.push(sub.id.clone());
            }
        }
        ids
    }

    /// 启动脚本引擎（首次启动创建线程，后续发送新脚本）
    pub fn start_script(
        &mut self,
        session_id: &str,
        code: &str,
        app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        // 主会话路径
        if let Some(handle) = self.sessions.get_mut(session_id) {
            let comm = handle.comm_handle.clone()
                .ok_or("通信句柄不可用".to_string())?;

            match &handle.script_tx {
                Some(tx) => {
                    // 已在运行，发送新脚本
                    tx.send(ScriptCmd::LoadScript(code.to_string()))
                        .map_err(|e| format!("发送脚本失败: {}", e))?;
                }
                None => {
                    // 首次启动：通过 CommHandle 注册数据接收回调（替代 feed_script_data 直传）
                    // 此后所有接收数据经 CommHandle::notify_receive() 扇出时自动送达
                    let (tx, rx) = mpsc::sync_channel::<ScriptCmd>(4096);
                    let tx_for_callback = tx.clone();
                    comm.on_receive(Box::new(move |data: &[u8]| {
                        // bounded channel (4096)：缓冲区满时丢弃旧数据。
                        // 若脚本引擎处理速度持续落后，丢包比 OOM 更安全。
                        let _ = tx_for_callback.try_send(ScriptCmd::FeedData(data.to_vec()));
                    }));
                    let shutdown = Arc::new(AtomicBool::new(false));
                    let thread = spawn_script_thread(
                        comm,
                        app_handle,
                        rx,
                        session_id.to_string(),
                        shutdown.clone(),
                    );
                    tx.send(ScriptCmd::LoadScript(code.to_string()))
                        .map_err(|e| format!("发送脚本失败: {}", e))?;
                    handle.script_tx = Some(tx);
                    handle.script_thread = Some(thread);
                    handle.script_shutdown = Some(shutdown);
                }
            }
            return Ok(());
        }

        // 对端（网络调试子连接）路径：对端拥有各自的 CommHandle 与脚本状态
        let not_found = self.session_not_found(session_id);
        let (parent_id, sub_idx) = self
            .find_sub_connection_index(session_id)
            .ok_or(not_found)?;
        let parent_not_found = self.session_not_found(&parent_id);
        let handle = self.sessions.get_mut(&parent_id)
            .ok_or(parent_not_found)?;
        let sub = &mut handle.sub_connections[sub_idx];
        let comm = sub.comm_handle.clone()
            .ok_or("通信句柄不可用".to_string())?;

        match &sub.script_tx {
            Some(tx) => {
                tx.send(ScriptCmd::LoadScript(code.to_string()))
                    .map_err(|e| format!("发送脚本失败: {}", e))?;
            }
            None => {
                let (tx, rx) = mpsc::sync_channel::<ScriptCmd>(4096);
                let tx_for_callback = tx.clone();
                comm.on_receive(Box::new(move |data: &[u8]| {
                    let _ = tx_for_callback.try_send(ScriptCmd::FeedData(data.to_vec()));
                }));
                let shutdown = Arc::new(AtomicBool::new(false));
                let thread = spawn_script_thread(
                    comm,
                    app_handle,
                    rx,
                    session_id.to_string(),
                    shutdown.clone(),
                );
                tx.send(ScriptCmd::LoadScript(code.to_string()))
                    .map_err(|e| format!("发送脚本失败: {}", e))?;
                sub.script_tx = Some(tx);
                sub.script_thread = Some(thread);
                sub.script_shutdown = Some(shutdown);
            }
        }
        Ok(())
    }

    /// 停止脚本引擎
    pub fn stop_script(&mut self, session_id: &str) -> Result<(), String> {
        // 对端（网络调试子连接）路径
        if !self.sessions.contains_key(session_id) {
            let not_found = self.session_not_found(session_id);
            let (parent_id, sub_idx) = self
                .find_sub_connection_index(session_id)
                .ok_or(not_found)?;
            let parent_not_found = self.session_not_found(&parent_id);
            let handle = self.sessions.get_mut(&parent_id)
                .ok_or(parent_not_found)?;
            let sub = &mut handle.sub_connections[sub_idx];

            // 先置协作式关闭标志，使 Lua sleep 分片及时中断，join 不长时阻塞全局锁
            if let Some(flag) = sub.script_shutdown.take() {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = sub.script_tx.take() {
                let _ = tx.send(ScriptCmd::Shutdown);
            }
            if let Some(thread) = sub.script_thread.take() {
                let _ = thread.join();
            }
            // 清理脚本引擎注册的接收回调，避免 stop→start 循环累积持废弃 channel 的死回调
            if let Some(comm) = &sub.comm_handle {
                comm.clear_receivers();
            }
            return Ok(());
        }

        // 主会话路径
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;

        // 先置协作式关闭标志，使 Lua sleep 分片及时中断，join 不长时阻塞全局锁
        if let Some(flag) = handle.script_shutdown.take() {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = handle.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(thread) = handle.script_thread.take() {
            let _ = thread.join();
        }
        // 清理脚本引擎注册的接收回调，避免 stop→start 循环累积持废弃 channel 的死回调
        if let Some(comm) = &handle.comm_handle {
            comm.clear_receivers();
        }
        Ok(())
    }

    /// 获取活跃会话 ID
    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    /// 获取所有标签页 ID
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tab_order.clone()
    }

    /// 获取会话句柄引用
    pub fn get_session(&self, session_id: &str) -> Option<&ActiveSessionHandle> {
        self.sessions.get(session_id)
    }

    /// 获取会话句柄可变引用
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut ActiveSessionHandle> {
        self.sessions.get_mut(session_id)
    }

    /// 获取持久化会话列表
    pub fn get_saved_sessions(&self) -> Vec<SavedSession> {
        let mut result: Vec<SavedSession> = Vec::new();
        for h in self.sessions.values() {
            // 父会话
            result.push(SavedSession {
                id: h.id.clone(),
                name: h.name.clone(),
                plugin_id: h.plugin_id.clone(),
                endpoint: h.endpoint.clone(),
                params: h.params.clone(),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                transfer_enabled: h.transfer_enabled,
                transfer_protocol: h.transfer_protocol.clone(),
                send_bar_enabled: h.send_bar_enabled,
                virtual_port_enabled: h.virtual_port_enabled(),
                virtual_port_count: h.virtual_port_count(),
            });
            // 子连接不持久化：通道是运行时概念，断开即清理
        }
        result
    }

    /// 重连指定会话
    /// TODO: 暴露为 Tauri 命令并在前端 ConnectDialog 编辑模式中使用，
    /// 以保留 UUID 和 I/O 统计连续性（当前前端使用 delete+create 方式）。
    pub fn reconnect_session(
        &mut self,
        session_id: &str,
        channel: Box<dyn Channel>,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
    ) -> Result<serde_json::Value, String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;

        if let Some(ref flag) = handle.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }

        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let (write_tx, write_rx) = mpsc::sync_channel::<IoLoopCmd>(256);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));

        let sid = session_id.to_string();
        let io_handle = spawn_sync_io_loop(
            channel, sid, on_data, on_disconnect, write_rx, cancel_rx,
            tx_bytes.clone(), rx_bytes.clone(),
        );
        let io_handle = IoTaskHandle::Sync(io_handle);

        let new_stats_flag = Arc::new(AtomicBool::new(false));
        Self::start_stats_collector(
            app_handle.clone(),
            session_id.to_string(),
            tx_bytes.clone(),
            rx_bytes.clone(),
            connected_at,
            new_stats_flag.clone(),
        );

        let params = handle.params.clone();

        // 重建 comm_handle，确保重连后脚本引擎使用新的 write_tx 通道。
        // 旧 comm_handle 持有的 write_tx 副本已随旧 I/O 线程停止而失效，
        // 若不重建，脚本引擎 send() 会向死 channel 写入，错误被静默吞掉。
        // 会话编码在重连时可能已变更（编辑会话后重连），从新 params 重新解析。
        let new_comm: Arc<dyn CommHandle> = Arc::new(crate::channel::serial_comm::SerialCommHandle::new(
            write_tx.clone(),
            params
                .get("encoding")
                .and_then(|v| v.as_str())
                .unwrap_or("utf-8")
                .to_string(),
        ));
        // 清理旧 comm_handle 上可能残留的回调（防止 stop→reconnect→start 链路
        // 下旧回调堆积在已失效的 CommHandle 中）
        if let Some(old_comm) = &handle.comm_handle {
            old_comm.clear_receivers();
        }
        handle.comm_handle = Some(new_comm);

        // 重连后脚本引擎持有的旧 comm_handle 的 write_tx 已失效。
        // 若脚本引擎正在运行，通知前端需要手动重启。
        // script_shutdown 比 script_tx 语义更精确：前者表示"存在需协作关闭的后台线程"，
        // 后者仅表示命令通道存在（stop_script 后两者均为 None，此处等价，但语义不同）。
        if handle.script_shutdown.is_some() {
            let _ = app_handle.emit(
                "script-log",
                serde_json::json!({
                    "session_id": session_id,
                    "message": "[Engine] 会话已重连 — 请重新启动脚本引擎",
                }),
            );
        }

        handle.write_tx = Some(write_tx);
        handle.io_cancel_tx = Some(cancel_tx);
        handle.io_thread = Some(io_handle);
        handle.state = SessionState::Connected;
        handle.channel_return_tx = None;
        handle.tx_bytes = tx_bytes;
        handle.rx_bytes = rx_bytes;
        handle.connected_at = connected_at;
        handle.stats_cancel_tx = None;
        handle.stats_cancel_flag = Some(new_stats_flag);

        Ok(params)
    }

    /// 取消传输
    pub fn cancel_transfer(&mut self, session_id: &str) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;
        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    /// 为 SFTP/SCP 传输准备取消标志。
    ///
    /// 在传输开始前调用：在会话句柄上设置一个新的 `AtomicBool`（初值 false），
    /// 返回其 `Arc` 克隆供传输循环轮询。传输结束后应调用 `transfer_done` 清理。
    ///
    /// 设计决策：同一会话同一时刻只允许一个传输进行中。
    /// 若已有传输进行中（flag 已存在），返回错误以防止并发传输互相覆盖取消标志。
    pub fn transfer_start(&mut self, session_id: &str) -> Result<Arc<AtomicBool>, String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;
        if handle.transfer_cancel.is_some() {
            return Err("该会话已有传输进行中，请等待完成或取消后再试".to_string());
        }
        let flag = Arc::new(AtomicBool::new(false));
        handle.transfer_cancel = Some(flag.clone());
        Ok(flag)
    }

    /// 取消当前侧通道传输（置位取消标志，传输循环在下次块检查时退出）。
    pub fn cancel_transfer_op(&mut self, session_id: &str) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;
        if let Some(flag) = &handle.transfer_cancel {
            flag.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    /// 清理 SFTP/SCP 传输状态（传输结束后调用，无论成功/失败/取消）。
    pub fn transfer_done(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.transfer_cancel = None;
        }
    }

    /// 注册传输 task 的 JoinHandle，供 close_session 等待完成。
    ///
    /// 每次 `tokio::spawn` 启动传输后调用此方法，将 handle 存入会话句柄。
    /// `close_session()` 在 I/O 线程退出后 join 所有已注册的 handle，
    /// 确保传输 task 的 Drop 清理逻辑执行完毕。
    pub fn register_transfer_task(
        &mut self,
        session_id: &str,
        handle: tokio::task::JoinHandle<()>,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let h = self.sessions.get_mut(session_id).ok_or(not_found)?;
        // 清理已完成的 handle，防止长时间运行会话中 transfer_tasks 无限增长
        h.transfer_tasks.retain(|h| !h.is_finished());
        h.transfer_tasks.push(handle);
        Ok(())
    }

    /// 获取会话状态
    pub fn session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).map(|h| h.state.clone())
    }

    /// 标记会话为已断开（由 on_disconnect 回调调用）。
    ///
    /// 调用时机：I/O 循环检测到连接丢失。
    /// 注意：此时 I/O 线程正在退出，不应尝试 join（会死锁）。
    /// 但必须取消 SFTP、脚本引擎和统计采集器，避免资源泄漏。
    pub fn mark_disconnected(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.state = SessionState::Disconnected;

            // ── 清理所有子连接 ──
            for sub in handle.sub_connections.iter_mut() {
                sub.state = SessionState::Disconnected;
                if let Some(ref flag) = sub.stats_cancel_flag {
                    flag.store(true, Ordering::SeqCst);
                }
                if let Some(tx) = sub.io_cancel_tx.take() {
                    let _ = tx.send(());
                }
                // 不 join 子连接 I/O thread — 在 I/O callback 中调用会死锁
                // I/O thread 会在父 session close_session 时 join
            }

            // ── 侧通道传输取消 ──
            // 连接已断开，SFTP 传输不可能完成。置位取消标志使传输循环退出。
            if let Some(flag) = handle.transfer_cancel.take() {
                flag.store(true, Ordering::SeqCst);
                log::info!(
                    "已取消会话 {} 的进行中 SFTP 传输（连接已断开）",
                    session_id
                );
            }
            // 在独立 task 中 join SFTP handles，不阻塞 on_disconnect 回调
            // mark_disconnected 在 I/O task 回调中调用，通常有 tokio runtime，
            // 但仍做防护性检查以防边缘情况。
            for task in handle.transfer_tasks.drain(..) {
                let sid = session_id.to_string();
                match tokio::runtime::Handle::try_current() {
                    Ok(_) => {
                        tokio::spawn(async move {
                            match tokio::time::timeout(Duration::from_secs(5), task).await {
                                Ok(_) => {
                                    log::debug!("SFTP 传输 task 已清理 (session: {})", sid);
                                }
                                Err(_) => {
                                    log::warn!("SFTP 传输 task join 超时 (session: {})", sid);
                                }
                            }
                        });
                    }
                    Err(_) => {
                        log::warn!(
                            "无法 join SFTP 传输 task（无 tokio runtime），task handle 将被 drop (session: {})",
                            sid
                        );
                    }
                }
            }

            // ── 脚本引擎关闭 ──
            if let Some(ref flag) = handle.script_shutdown {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = handle.script_tx.take() {
                let _ = tx.send(ScriptCmd::Shutdown);
            }
            if let Some(thread) = handle.script_thread.take() {
                let _ = thread.join();
            }
            if let Some(comm) = &handle.comm_handle {
                comm.clear_receivers();
            }

            // ── 取消传输（X/Y/ZModem）──
            if let Some(tx) = handle.cancel_transfer_tx.take() {
                let _ = tx.send(());
            }

            // ── 统计采集器 ──
            if let Some(ref flag) = handle.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }

            // ── 虚拟端口桥接 ──
            if let Some(bridge) = handle.virtual_port_bridge.take() {
                bridge.shutdown();
                log::info!(
                    "虚拟端口桥接已关闭（设备意外断开，session: {}）",
                    session_id
                );
            }

            // ── I/O 线程 ──
            // io_cancel_tx 置位（触发 I/O 循环退出），但保留 io_thread
            // JoinHandle 供后续 close_session() join。
            handle.io_cancel_tx = None;
        }
    }

    /// 标记子连接为已断开（由子通道 I/O 回调调用）。
    ///
    /// 与 `mark_disconnected` 同理 — 调用时 I/O task 正在退出，不能 join 自己的线程。
    /// 仅标记状态、取消统计采集器。实际的 I/O task join 由后续的 `close_sub_connection` 或
    /// `close_session` 在安全上下文中执行。
    pub fn mark_sub_disconnected(&mut self, parent_id: &str, channel_id: &str) {
        if let Some(handle) = self.sessions.get_mut(parent_id) {
            for sub in handle.sub_connections.iter_mut() {
                if sub.id == channel_id {
                    sub.state = SessionState::Disconnected;
                    if let Some(ref flag) = sub.stats_cancel_flag {
                        flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    // 不 join I/O task — 在 I/O callback 上下文中调用，join 自己会死锁
                    sub.io_cancel_tx = None;
                    break;
                }
            }
            // 对端断开不级联父容器：TCP Server 监听器 / UDP bind 保持监听，
            // 父会话状态仅由显式 close_session 改变（见 network/mod.rs 模块注释）。
        }
    }

    /// 启动 I/O 统计采集器（使用 std::thread + AtomicBool 取消，无需 tokio runtime）。
    /// 子连接统计通过 [`Self::spawn_stats_collector`] 委托到此函数。
    fn start_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        tx_bytes: Arc<AtomicU64>,
        rx_bytes: Arc<AtomicU64>,
        connected_at: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        Self::spawn_stats_collector(app_handle, tab_id, tx_bytes, rx_bytes, connected_at, cancel_flag);
    }

    /// 启动 I/O 统计采集器（用于子连接，与 [`Self::start_stats_collector`] 共享实现）
    pub fn spawn_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        tx_bytes: Arc<AtomicU64>,
        rx_bytes: Arc<AtomicU64>,
        connected_at: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        std::thread::spawn(move || {
            let mut last_tx: u64 = 0;
            let mut last_rx: u64 = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if cancel_flag.load(Ordering::SeqCst) { break; }
                let tx = tx_bytes.load(Ordering::Relaxed);
                let rx = rx_bytes.load(Ordering::Relaxed);
                if tx != last_tx || rx != last_rx {
                    last_tx = tx; last_rx = rx;
                    let _ = app_handle.emit("session-stats", SessionStats {
                        tab_id: tab_id.clone(),
                        tx_bytes: tx,
                        rx_bytes: rx,
                        connected_at,
                    });
                }
            }
        });
    }

    /// 获取会话持久化文件路径
    pub fn sessions_file_path(app_handle: &tauri::AppHandle) -> std::path::PathBuf {
        use tauri::Manager;
        let mut path = app_handle.path().app_data_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        std::fs::create_dir_all(&path).ok();
        path.push("sessions.json");
        path
    }

    /// 保存会话到磁盘
    pub fn save_to_disk(&self, path: &std::path::Path) -> Result<(), String> {
        let _guard = SESSIONS_FILE_MUTEX.lock().map_err(|e| format!("获取文件锁失败: {}", e))?;
        let current: Vec<SavedSession> = self.get_saved_sessions();
        let existing = Self::load_from_disk(path).unwrap_or_default();

        if current.is_empty() {
            return Ok(());
        }

        let current_ids: HashSet<String> = current.iter().map(|s| s.id.clone()).collect();
        let mut merged: Vec<SavedSession> = existing
            .into_iter()
            .filter(|s| !current_ids.contains(&s.id))
            .collect();
        merged.extend(current);

        // 按 session id 去重（保留 current_ids 中的版本，它们是最新的）
        let mut dedup: HashMap<String, SavedSession> = HashMap::new();
        for s in merged {
            if current_ids.contains(&s.id) {
                dedup.insert(s.id.clone(), s);
            } else {
                dedup.entry(s.id.clone()).or_insert(s);
            }
        }
        let merged: Vec<SavedSession> = dedup.into_values().collect();

        let json = serde_json::to_string_pretty(&merged)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("写入文件失败: {}", e))
    }

    /// 从磁盘加载会话
    pub fn load_from_disk(path: &std::path::Path) -> Result<Vec<SavedSession>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }
        match serde_json::from_str::<Vec<SavedSession>>(&content) {
            Ok(sessions) => Ok(sessions),
            Err(e) => {
                let bak_path = path.with_extension("json.bak");
                let _ = std::fs::copy(path, &bak_path);
                log::warn!("会话文件损坏 ({}), 已备份到 {:?}", e, bak_path);
                Ok(Vec::new())
            }
        }
    }

    /// 保存单个会话配置到磁盘（合并写入，不依赖内存状态）
    pub fn save_config_to_disk(
        app_handle: &tauri::AppHandle,
        session: SavedSession,
    ) -> Result<(), String> {
        let _guard = SESSIONS_FILE_MUTEX.lock().map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle);
        let mut existing = Self::load_from_disk(&path).unwrap_or_default();
        // 用新配置覆盖同 ID 的旧记录
        existing.retain(|s| s.id != session.id);
        existing.push(session);
        let json = serde_json::to_string_pretty(&existing)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("写入文件失败: {}", e))
    }

    /// 从磁盘删除指定会话配置
    /// 从磁盘删除指定会话配置。
    pub fn delete_config_from_disk(
        app_handle: &tauri::AppHandle,
        session_id: &str,
    ) -> Result<(), String> {
        let _guard = SESSIONS_FILE_MUTEX.lock().map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle);
        let existing = Self::load_from_disk(&path).unwrap_or_default();
        let filtered: Vec<_> = existing.into_iter().filter(|s| s.id != session_id).collect();
        let json = serde_json::to_string_pretty(&filtered)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("写入文件失败: {}", e))
    }
}

impl Drop for SessionStore {
    fn drop(&mut self) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let _ = self.close_session(&id);
        }
    }
}
