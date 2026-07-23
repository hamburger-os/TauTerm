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
use std::sync::{mpsc, Arc};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use crate::channel::Channel;
use crate::channel::io_loop::{IoLoopCmd, spawn_sync_io_loop};
use crate::channel::async_io_loop::spawn_async_io_loop;
use crate::kernel::comm_handle::CommHandle;
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

/// I/O 统计快照
#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub tab_id: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connected_at: Option<u64>,
}

/// 单个会话句柄（协议无关）
pub struct ActiveSessionHandle {
    pub id: TabId,
    pub name: String,
    pub write_tx: mpsc::SyncSender<IoLoopCmd>,
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
        let zombie_ids: Vec<String> = self.sessions.iter()
            .filter(|(_, h)| h.state == SessionState::Disconnected)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &zombie_ids {
            self.sessions.remove(id);
        }

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
        let comm_handle: Arc<dyn CommHandle> = conn.comm_handle
            .unwrap_or_else(|| {
                Arc::new(crate::channel::serial_comm::SerialCommHandle::new(write_tx.clone()))
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
            ChannelKind::Sync(sync_channel) => {
                IoTaskHandle::Sync(spawn_sync_io_loop(
                    sync_channel, sid.clone(), wrapped_on_data, on_disconnect, write_rx, cancel_rx,
                    tx_clone, rx_clone,
                ))
            }
            ChannelKind::Async(async_channel) => {
                IoTaskHandle::Async(spawn_async_io_loop(
                    async_channel, sid.clone(), wrapped_on_data, on_disconnect, write_rx, cancel_rx,
                    tx_clone, rx_clone,
                ))
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
            write_tx,
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

    /// 关闭指定会话。
    ///
    /// 立即从 HashMap 中移除会话句柄，然后关闭桥接线程、取消传输、
    /// 发送 I/O 取消信号并 join I/O 线程。
    ///
    /// # 调用方约定
    ///
    /// **调用方必须在此调用之前 clone `virtual_port_pairs`**（如需访问），
    /// 因为此方法一开始就 `sessions.remove(session_id)`，句柄随后被 drop。
    /// 参考 `disconnect_session` 在 `commands.rs` 中的用法。
    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
        // 临时取出句柄以解除借用，关闭后再以 Disconnected 状态放回，
        // 使并发到达的传输命令可返回"会话已断开"而非"会话不存在"。
        let mut handle = self.sessions.remove(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;
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
        let _ = handle.write_tx.send(IoLoopCmd::Shutdown);
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

    /// 切换到指定会话
    pub fn switch_active(&mut self, session_id: &str) -> Result<(), String> {
        if !self.sessions.contains_key(session_id) {
            return Err(self.session_not_found(session_id));
        }
        // 拒绝切换到已断开（僵尸）句柄
        if let Some(h) = self.sessions.get(session_id) {
            if h.state == SessionState::Disconnected {
                return Err(self.session_not_found(session_id));
            }
        }
        self.active_id = Some(session_id.to_string());
        Ok(())
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

    /// 向指定会话写入数据
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let handle = self.sessions.get(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;
        handle.write_tx.send(IoLoopCmd::Write(data.to_vec()))
            .map_err(|e| format!("写入通道错误: {}", e))
    }

    /// 启动脚本引擎（首次启动创建线程，后续发送新脚本）
    pub fn start_script(
        &mut self,
        session_id: &str,
        code: &str,
        app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id)
            .ok_or(not_found)?;

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
                // 此后所有串口接收数据经 CommHandle::notify_receive() 扇出时自动送达
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
        Ok(())
    }

    /// 停止脚本引擎
    pub fn stop_script(&mut self, session_id: &str) -> Result<(), String> {
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
        self.sessions.values().map(|h| SavedSession {
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
        }).collect()
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
        let new_comm: Arc<dyn CommHandle> = Arc::new(crate::channel::serial_comm::SerialCommHandle::new(write_tx.clone()));
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

        handle.write_tx = write_tx;
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

    /// 启动 I/O 统计采集器（使用 std::thread + AtomicBool 取消，无需 tokio runtime）
    fn start_stats_collector(
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
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
                let tx = tx_bytes.load(Ordering::Relaxed);
                let rx = rx_bytes.load(Ordering::Relaxed);
                if tx != last_tx || rx != last_rx {
                    last_tx = tx;
                    last_rx = rx;
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
