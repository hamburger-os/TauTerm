//! 会话存储
//!
//! SessionStore owns user-visible session lifecycle. Physical I/O resources are owned by
//! `SessionDataPlane`; callers interact through `SessionIo` capabilities.

use crate::kernel::file_transfer::FileTransfer;
use crate::kernel::persistence::atomic_write;
use crate::kernel::plugin_adapter::{
    ProtocolConnection, SessionAttach, SessionChannelFactory, SessionService,
};
use crate::kernel::script_engine::{spawn_script_thread, ScriptCmd};
use crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};
use crate::transfer::scheduler::TransferScheduler;
use crate::transport::DataPlaneRuntime;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::{Emitter, Manager};

pub type TabId = String;

/// 会话状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Transferring,
}

/// Deferred child cleanup. The DataPlane owner is moved out while SessionStore is locked and
/// joined only after the caller releases that lock.
pub struct SubConnectionCleanup {
    channel_id: String,
    data_plane: Option<SessionDataPlane>,
    script_thread: Option<std::thread::JoinHandle<()>>,
}

impl SubConnectionCleanup {
    pub fn join(mut self) {
        if let Some(mut data_plane) = self.data_plane.take() {
            data_plane.shutdown();
            log::debug!("子连接 DataPlane 已清理: {}", self.channel_id);
        }
        if let Some(thread) = self.script_thread.take() {
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

/// 通用子连接句柄。
///
/// 每个子连接有独立的 DataPlane、SessionIo 和统计信息。协议可以决定它是否作为
/// Workspace 中的独立可见项，但协议地址、peer 元数据等领域信息由插件运行时自行持有。
pub struct SubConnection {
    pub id: TabId,
    pub name: String,
    pub data_plane: Option<SessionDataPlane>,
    pub io: Arc<SessionIo>,
    pub state: SessionState,
    pub connected_at: Option<u64>,
    pub stats_cancel_flag: Option<Arc<AtomicBool>>,
    pub channel_index: u32,
    pub elevated: bool,
    pub retain_terminal: bool,
    pub script_tx: Option<mpsc::SyncSender<ScriptCmd>>,
    pub script_thread: Option<std::thread::JoinHandle<()>>,
    pub script_shutdown: Option<Arc<AtomicBool>>,
    pub visible_in_workspace: bool,
}

pub struct ActiveSessionHandle {
    pub id: TabId,
    pub name: String,
    pub data_plane: Option<SessionDataPlane>,
    pub io: Option<Arc<SessionIo>>,
    pub state: SessionState,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub connected_at: Option<u64>,
    pub stats_cancel_flag: Option<Arc<AtomicBool>>,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
    pub script_tx: Option<mpsc::SyncSender<ScriptCmd>>,
    pub script_thread: Option<std::thread::JoinHandle<()>>,
    pub script_shutdown: Option<Arc<AtomicBool>>,
    pub service: Option<Arc<dyn SessionService>>,
    pub file_transfer: Option<Arc<dyn FileTransfer>>,
    pub attachment: Option<Arc<dyn SessionAttach>>,
    pub channel_factory: Option<Arc<dyn SessionChannelFactory>>,
    pub transfer_scheduler: TransferScheduler,
    pub transfer_tasks: Vec<tokio::task::JoinHandle<()>>,
    pub teardown_delay: Duration,
    pub sub_connections: Vec<SubConnection>,
    pub next_child_index: u32,
}

impl SubConnection {
    pub fn new(
        id: TabId,
        name: String,
        data_plane: SessionDataPlane,
        io: Arc<SessionIo>,
        channel_index: u32,
        elevated: bool,
    ) -> Self {
        Self {
            id,
            name,
            data_plane: Some(data_plane),
            io,
            state: SessionState::Connected,
            connected_at: None,
            stats_cancel_flag: None,
            channel_index,
            elevated,
            retain_terminal: false,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            visible_in_workspace: true,
        }
    }

    /// 创建不直接显示为 Workspace tab 的通用后台子连接。
    pub fn background(
        id: TabId,
        name: String,
        data_plane: SessionDataPlane,
        io: Arc<SessionIo>,
        channel_index: u32,
    ) -> Self {
        let mut child = Self::new(id, name, data_plane, io, channel_index, false);
        child.visible_in_workspace = false;
        child
    }
}

impl Drop for ActiveSessionHandle {
    fn drop(&mut self) {
        if let Some(attachment) = self.attachment.take() {
            attachment.on_detached(&self.id);
        }
        if let Some(service) = self.service.take() {
            std::thread::spawn(move || service.shutdown());
        }
        self.file_transfer = None;
        self.channel_factory = None;
        self.transfer_scheduler.cancel_for_shutdown();
        if let Some(flag) = &self.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(flag) = &self.script_shutdown {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = self.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(data_plane) = &self.data_plane {
            data_plane.request_shutdown();
        }
        for sub in &mut self.sub_connections {
            if let Some(flag) = &sub.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(data_plane) = &sub.data_plane {
                data_plane.request_shutdown();
            }
            if let Some(flag) = &sub.script_shutdown {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = sub.script_tx.take() {
                let _ = tx.send(ScriptCmd::Shutdown);
            }
        }
    }
}

/// 会话存储
pub struct SessionStore {
    sessions: HashMap<TabId, ActiveSessionHandle>,
    active_id: Option<TabId>,
    tab_order: Vec<TabId>,
    /// 只限制运行中的根 Session；Saved Session Library 不受这个运行时资源预算约束。
    max_active_root_sessions: usize,
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
}

const SESSION_LIBRARY_VERSION: u32 = 2;
const DEFAULT_MAX_ACTIVE_ROOT_SESSIONS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionLibraryFile {
    version: u32,
    sessions: Vec<SavedSession>,
}

/// 全局文件锁 — 保护 sessions.json 的 read-modify-write 操作。
/// 目前 Tauri 命令串行执行，但该锁为未来的并行调用（如批量删除）提供安全保证。
static SESSIONS_FILE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 普通会话创建参数。
pub struct SessionCreateOptions {
    pub name: String,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
    pub id_override: Option<String>,
}

/// 容器会话创建参数。
#[derive(Default)]
pub struct ContainerSessionRuntime {
    pub service: Option<Arc<dyn SessionService>>,
    pub file_transfer: Option<Arc<dyn FileTransfer>>,
    pub channel_factory: Option<Arc<dyn SessionChannelFactory>>,
    pub io: Option<Arc<SessionIo>>,
    pub attachment: Option<Arc<dyn SessionAttach>>,
    pub teardown_delay: Duration,
}

pub struct ContainerSessionCreateOptions {
    pub name: String,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
    pub id_override: Option<String>,
}

impl SessionStore {
    /// 保留最近关闭会话名称的数量上限（LRU 淘汰）
    const MAX_REMOVED_SESSION_NAMES: usize = 50;

    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            active_id: None,
            tab_order: Vec::new(),
            max_active_root_sessions: DEFAULT_MAX_ACTIVE_ROOT_SESSIONS,
            session_names: HashMap::new(),
            removed_order: VecDeque::new(),
        }
    }

    /// 创建新会话（使用协议适配器返回的 `ProtocolConnection`）
    pub fn create_session(
        &mut self,
        options: SessionCreateOptions,
        mut conn: ProtocolConnection,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
        app_handle: tauri::AppHandle,
    ) -> Result<TabId, String> {
        let SessionCreateOptions {
            name,
            plugin_id,
            endpoint,
            params,
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            id_override,
        } = options;
        if let Some(raw) = id_override.as_ref() {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            match self.sessions.get(raw).map(|handle| handle.state.clone()) {
                Some(SessionState::Disconnected) => {
                    self.sessions.remove(raw);
                }
                Some(_) => {
                    return Err(format!("会话 {} 已存在且未断开，拒绝覆盖活动运行时", raw));
                }
                None => {}
            }
        }
        self.purge_zombies();
        if self.sessions.len() >= self.max_active_root_sessions {
            return Err(format!(
                "已达到最大活动根会话数限制 ({})",
                self.max_active_root_sessions
            ));
        }
        let id = id_override.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let tab_name = if name.is_empty() {
            format!("{} @ {}", plugin_id, endpoint)
        } else {
            name
        };
        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        let runtime = conn.data_plane.take().ok_or_else(|| {
            "create_session requires a DataPlane; use create_container_session for headless protocols".to_string()
        })?;
        let encoding = params
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8");
        let io = Arc::new(SessionIo::new(Some(runtime.handle.clone()), None, encoding));
        let data_plane = SessionDataPlane::attach(runtime, id.clone(), on_data, on_disconnect)
            .map_err(|e| e.to_string())?;
        let stats_cancel_flag = Arc::new(AtomicBool::new(false));
        Self::start_stats_collector(
            app_handle,
            id.clone(),
            io.clone(),
            connected_at,
            stats_cancel_flag.clone(),
        );
        if let Some(attach) = conn.on_attached.as_ref() {
            attach.on_attached(&id);
        }
        let handle = ActiveSessionHandle {
            id: id.clone(),
            name: tab_name.clone(),
            data_plane: Some(data_plane),
            io: Some(io),
            state: SessionState::Connected,
            plugin_id,
            endpoint,
            params,
            connected_at,
            stats_cancel_flag: Some(stats_cancel_flag),
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            service: conn.service,
            file_transfer: conn.file_transfer,
            attachment: conn.on_attached,
            channel_factory: conn.channel_factory,
            transfer_scheduler: TransferScheduler::default(),
            transfer_tasks: Vec::new(),
            teardown_delay: conn.teardown_delay,
            sub_connections: Vec::new(),
            next_child_index: 0,
        };
        debug_assert!(!self.sessions.contains_key(&id));
        self.tab_order.retain(|tid| tid != &id);
        self.sessions.insert(id.clone(), handle);
        self.tab_order.push(id.clone());
        self.session_names.insert(id.clone(), tab_name);
        self.active_id = Some(id.clone());
        Ok(id)
    }

    /// 清理所有 Disconnected 状态的僵尸会话，以免占用 max_sessions 名额。
    fn purge_zombies(&mut self) {
        let zombie_ids: Vec<String> = self
            .sessions
            .iter()
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

    /// 创建终端父容器（无 I/O loop）。
    ///
    /// 仅持有可选协议 service、文件传输 capability、channel factory 和元数据，不创建 I/O 线程。
    /// 实际终端通过 `add_sub_connection` 添加。
    pub fn create_container_session(
        &mut self,
        options: ContainerSessionCreateOptions,
        runtime: ContainerSessionRuntime,
    ) -> Result<TabId, String> {
        let ContainerSessionRuntime {
            service,
            file_transfer,
            channel_factory,
            io,
            attachment,
            teardown_delay,
        } = runtime;
        let ContainerSessionCreateOptions {
            name,
            plugin_id,
            endpoint,
            params,
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            id_override,
        } = options;
        let id = if let Some(raw) = id_override.as_ref() {
            if uuid::Uuid::parse_str(raw).is_err() {
                return Err(format!("无效的 session_id 格式: {}", raw));
            }
            raw.clone()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let preserved_next_child_index = self
            .sessions
            .get(&id)
            .filter(|h| h.state == SessionState::Disconnected)
            .map(|h| h.next_child_index)
            .unwrap_or(0);
        match self.sessions.get(&id).map(|handle| handle.state.clone()) {
            Some(SessionState::Disconnected) => {
                self.sessions.remove(&id);
            }
            Some(_) => {
                return Err(format!("会话 {} 已存在且未断开，拒绝覆盖活动运行时", id));
            }
            None => {}
        }
        self.purge_zombies();
        if self.sessions.len() >= self.max_active_root_sessions {
            return Err(format!(
                "已达到最大活动根会话数限制 ({})",
                self.max_active_root_sessions
            ));
        }
        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        let session_name = if name.is_empty() {
            format!("{} @ {}", plugin_id, endpoint)
        } else {
            name
        };
        let handle = ActiveSessionHandle {
            id: id.clone(),
            name: session_name.clone(),
            data_plane: None,
            io,
            state: SessionState::Connected,
            plugin_id,
            endpoint,
            params,
            connected_at,
            stats_cancel_flag: None,
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            script_tx: None,
            script_thread: None,
            script_shutdown: None,
            service,
            file_transfer,
            attachment: attachment.clone(),
            channel_factory,
            transfer_scheduler: TransferScheduler::default(),
            transfer_tasks: Vec::new(),
            teardown_delay,
            sub_connections: Vec::new(),
            next_child_index: preserved_next_child_index,
        };
        if let Some(attach) = attachment.as_ref() {
            attach.on_attached(&id);
        }
        self.sessions.insert(id.clone(), handle);
        self.tab_order.retain(|tid| tid != &id);
        self.tab_order.push(id.clone());
        self.session_names.insert(id.clone(), session_name);
        self.active_id = Some(id.clone());
        Ok(id)
    }

    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
        let mut handle = self
            .sessions
            .remove(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;
        for mut sub in std::mem::take(&mut handle.sub_connections) {
            if let Some(flag) = &sub.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(flag) = &sub.script_shutdown {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = sub.script_tx.take() {
                let _ = tx.send(ScriptCmd::Shutdown);
            }
            if let Some(data_plane) = &sub.data_plane {
                data_plane.request_shutdown();
            }
        }
        self.session_names
            .insert(session_id.to_string(), handle.name.clone());
        self.removed_order.push_back(session_id.to_string());
        while self.removed_order.len() > Self::MAX_REMOVED_SESSION_NAMES {
            if let Some(old_id) = self.removed_order.pop_front() {
                self.session_names.remove(&old_id);
            }
        }
        handle.transfer_scheduler.cancel_for_shutdown();
        if let Some(flag) = &handle.script_shutdown {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = handle.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(thread) = handle.script_thread.take() {
            let _ = thread.join();
        }
        if let Some(flag) = &handle.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(data_plane) = &handle.data_plane {
            data_plane.request_shutdown();
        }
        handle.data_plane = None;
        for task in handle.transfer_tasks.drain(..) {
            let sid = session_id.to_string();
            if tokio::runtime::Handle::try_current().is_ok() {
                tokio::spawn(async move {
                    if tokio::time::timeout(Duration::from_secs(5), task)
                        .await
                        .is_err()
                    {
                        log::warn!("传输 task join 超时 (session: {})", sid);
                    }
                });
            }
        }
        if !handle.teardown_delay.is_zero() {
            std::thread::sleep(handle.teardown_delay);
        }
        self.tab_order.retain(|id| id != session_id);
        if self.active_id.as_deref() == Some(session_id) {
            self.active_id = self.tab_order.first().cloned();
        }
        if let Some(service) = &handle.service {
            service.shutdown();
        }
        if let Some(attachment) = &handle.attachment {
            attachment.on_detached(session_id);
        }
        handle.service = None;
        handle.file_transfer = None;
        handle.attachment = None;
        handle.channel_factory = None;
        handle.io = None;
        handle.state = SessionState::Disconnected;
        self.sessions.insert(session_id.to_string(), handle);
        Ok(())
    }

    /// 格式化"会话不存在"错误消息，优先使用已保存的会话名称
    pub(crate) fn session_not_found(&self, session_id: &str) -> String {
        let display_name = self
            .session_names
            .get(session_id)
            .map(|n| n.as_str())
            .unwrap_or(session_id);
        format!("会话 {} 不存在", display_name)
    }

    /// 切换到指定会话（支持子连接 ID）
    pub fn switch_active(&mut self, session_id: &str) -> Result<(), String> {
        if let Some(h) = self.sessions.get(session_id) {
            if h.state == SessionState::Disconnected {
                return Err(self.session_not_found(session_id));
            }
            self.active_id = Some(session_id.to_string());
            return Ok(());
        }
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
        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;
        handle.name = new_name.to_string();
        self.session_names
            .insert(session_id.to_string(), new_name.to_string());
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

    /// 向指定会话写入数据（支持通用子连接路由）
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        self.get_io_for(session_id)
            .ok_or_else(|| format!("会话 {} 不可直接写入", session_id))?
            .send(data)
            .map_err(|e| e.to_string())
    }

    /// 添加子连接到父会话
    pub fn add_sub_connection(
        &mut self,
        parent_id: &str,
        sub: SubConnection,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(parent_id);
        let handle = self.sessions.get_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            return Err(not_found);
        }
        handle.sub_connections.push(sub);
        Ok(())
    }

    /// 查找子连接所属的父会话与下标。
    fn find_sub_connection_index(&self, channel_id: &str) -> Option<(TabId, usize)> {
        for (pid, handle) in &self.sessions {
            for (index, sub) in handle.sub_connections.iter().enumerate() {
                if sub.id == channel_id {
                    return Some((pid.clone(), index));
                }
            }
        }
        None
    }

    /// 获取通信句柄（支持通用子连接路由）。
    pub fn get_io_for(&self, session_id: &str) -> Option<Arc<SessionIo>> {
        if let Some(handle) = self.sessions.get(session_id) {
            return handle.io.clone();
        }
        self.sessions.values().find_map(|handle| {
            handle
                .sub_connections
                .iter()
                .find(|sub| sub.id == session_id)
                .map(|sub| sub.io.clone())
        })
    }

    /// 关闭单个子连接（两段式）。
    ///
    /// **阶段 1（本方法，持 store 锁）**：向脚本引擎 / 统计采集器 / I/O loop
    /// 发送全部关闭信号，并从 `sub_connections` 移除，返回待 join 的句柄。
    /// **阶段 2（调用方，锁外）**：调用 [`SubConnectionCleanup::join`]。
    pub fn close_sub_connection(
        &mut self,
        parent_id: &str,
        channel_id: &str,
    ) -> Result<(bool, SubConnectionCleanup), String> {
        let not_found = self.session_not_found(parent_id);
        let handle = self.sessions.get_mut(parent_id).ok_or(not_found)?;
        let idx = handle
            .sub_connections
            .iter()
            .position(|s| s.id == channel_id)
            .ok_or_else(|| format!("子连接 {} 在会话 {} 中不存在", channel_id, parent_id))?;
        let mut sub = handle.sub_connections.remove(idx);
        if let Some(flag) = sub.script_shutdown.take() {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = sub.script_tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        let script_thread = sub.script_thread.take();
        if let Some(flag) = &sub.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(data_plane) = &sub.data_plane {
            data_plane.request_shutdown();
        }
        let data_plane = sub.data_plane.take();
        let is_last = handle
            .sub_connections
            .iter()
            .all(|child| child.state == SessionState::Disconnected);
        Ok((
            is_last,
            SubConnectionCleanup {
                channel_id: channel_id.to_string(),
                data_plane,
                script_thread,
            },
        ))
    }

    /// 解析 session_id，返回实际的顶层会话 ID。
    pub fn resolve_parent_id(&self, session_id: &str) -> Option<String> {
        if self.sessions.contains_key(session_id) {
            return Some(session_id.to_string());
        }
        self.find_parent_of_channel(session_id)
    }

    /// 查找子连接所属的父会话 ID
    pub fn find_parent_of_channel(&self, channel_id: &str) -> Option<String> {
        for (pid, handle) in &self.sessions {
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
        if let Some(handle) = self.sessions.get_mut(session_id) {
            let io = handle.io.clone().ok_or("通信能力不可用")?;
            if let Some(tx) = &handle.script_tx {
                return tx
                    .send(ScriptCmd::LoadScript(code.to_string()))
                    .map_err(|e| format!("发送脚本失败: {}", e));
            }
            let subscription = io
                .primary()
                .map(|_| io.subscribe())
                .transpose()
                .map_err(|e| e.to_string())?;
            let (tx, rx) = mpsc::sync_channel::<ScriptCmd>(4096);
            let shutdown = Arc::new(AtomicBool::new(false));
            let thread = spawn_script_thread(
                io,
                subscription,
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
            return Ok(());
        }
        let not_found = self.session_not_found(session_id);
        let (parent_id, sub_idx) = self
            .find_sub_connection_index(session_id)
            .ok_or(not_found)?;
        let handle = self.sessions.get_mut(&parent_id).ok_or("父会话不存在")?;
        let sub = &mut handle.sub_connections[sub_idx];
        if let Some(tx) = &sub.script_tx {
            return tx
                .send(ScriptCmd::LoadScript(code.to_string()))
                .map_err(|e| format!("发送脚本失败: {}", e));
        }
        let io = sub.io.clone();
        let subscription = io.subscribe().map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::sync_channel::<ScriptCmd>(4096);
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread = spawn_script_thread(
            io,
            Some(subscription),
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
        Ok(())
    }

    /// 停止脚本引擎
    pub fn stop_script(&mut self, session_id: &str) -> Result<(), String> {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            Self::stop_script_fields(
                &mut handle.script_shutdown,
                &mut handle.script_tx,
                &mut handle.script_thread,
            );
            return Ok(());
        }
        let not_found = self.session_not_found(session_id);
        let (parent_id, sub_idx) = self
            .find_sub_connection_index(session_id)
            .ok_or(not_found)?;
        let handle = self.sessions.get_mut(&parent_id).ok_or("父会话不存在")?;
        let sub = &mut handle.sub_connections[sub_idx];
        Self::stop_script_fields(
            &mut sub.script_shutdown,
            &mut sub.script_tx,
            &mut sub.script_thread,
        );
        Ok(())
    }

    fn stop_script_fields(
        shutdown: &mut Option<Arc<AtomicBool>>,
        tx: &mut Option<mpsc::SyncSender<ScriptCmd>>,
        thread: &mut Option<std::thread::JoinHandle<()>>,
    ) {
        if let Some(flag) = shutdown.take() {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(tx) = tx.take() {
            let _ = tx.send(ScriptCmd::Shutdown);
        }
        if let Some(thread) = thread.take() {
            let _ = thread.join();
        }
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

    pub fn reset_child_counter(&mut self, parent_id: &str) {
        if let Some(handle) = self.sessions.get_mut(parent_id) {
            handle.next_child_index = 0;
        }
    }

    /// 重连指定会话。
    pub fn reconnect_session(
        &mut self,
        session_id: &str,
        runtime: DataPlaneRuntime,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send + 'static>,
        app_handle: tauri::AppHandle,
    ) -> Result<serde_json::Value, String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;
        if let Some(flag) = &handle.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(old) = &handle.data_plane {
            old.request_shutdown();
        }
        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        let encoding = handle
            .params
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("utf-8");
        let io = Arc::new(SessionIo::new(Some(runtime.handle.clone()), None, encoding));
        let data_plane =
            SessionDataPlane::attach(runtime, session_id.to_string(), on_data, on_disconnect)
                .map_err(|e| e.to_string())?;
        let stats_flag = Arc::new(AtomicBool::new(false));
        Self::start_stats_collector(
            app_handle.clone(),
            session_id.to_string(),
            io.clone(),
            connected_at,
            stats_flag.clone(),
        );
        if handle.script_shutdown.is_some() {
            let _ = app_handle.emit(
                "script-log",
                serde_json::json!({
                    "session_id": session_id,
                    "message": "[Engine] 会话已重连 — 请重新启动脚本引擎",
                }),
            );
            Self::stop_script_fields(
                &mut handle.script_shutdown,
                &mut handle.script_tx,
                &mut handle.script_thread,
            );
        }
        handle.data_plane = Some(data_plane);
        handle.io = Some(io);
        handle.state = SessionState::Connected;
        handle.connected_at = connected_at;
        handle.stats_cancel_flag = Some(stats_flag);
        Ok(handle.params.clone())
    }

    /// 为 Inline 传输预留当前 Session 的唯一活动槽。
    pub fn reserve_inline_transfer(
        &mut self,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>, String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;
        handle.transfer_scheduler.reserve_inline(transfer_id)
    }

    /// 为 Auxiliary 传输预留活动槽，并返回协议循环使用的取消标志。
    pub fn transfer_start(
        &mut self,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<Arc<AtomicBool>, String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;
        handle.transfer_scheduler.reserve_auxiliary(transfer_id)
    }

    /// 精确取消当前 Session 的活动传输，调度器内部区分 Inline / Auxiliary。
    pub fn cancel_scheduled_transfer(
        &mut self,
        session_id: &str,
        transfer_id: Option<&str>,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;
        handle.transfer_scheduler.cancel(transfer_id)
    }

    /// 清理当前传输占用。Auxiliary PanicGuard 与 Inline 归还端口共用此入口。
    pub fn transfer_done(&mut self, session_id: &str, transfer_id: Option<&str>) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            let _ = handle.transfer_scheduler.finish(transfer_id);
        }
    }

    /// 注册传输 task 的 JoinHandle，供 close_session 等待完成。
    pub fn register_transfer_task(
        &mut self,
        session_id: &str,
        handle: tokio::task::JoinHandle<()>,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let h = self.sessions.get_mut(session_id).ok_or(not_found)?;
        h.transfer_tasks.retain(|h| !h.is_finished());
        h.transfer_tasks.push(handle);
        Ok(())
    }

    /// 获取会话状态
    pub fn session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).map(|h| h.state.clone())
    }

    /// 标记会话为已断开（由 on_disconnect 回调调用）。
    pub fn mark_disconnected(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.state = SessionState::Disconnected;
            for sub in &mut handle.sub_connections {
                sub.state = SessionState::Disconnected;
                if let Some(flag) = &sub.stats_cancel_flag {
                    flag.store(true, Ordering::SeqCst);
                }
                if let Some(data_plane) = &sub.data_plane {
                    data_plane.request_shutdown();
                }
            }
            handle.transfer_scheduler.cancel_for_shutdown();
            if let Some(flag) = &handle.script_shutdown {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = handle.script_tx.take() {
                let _ = tx.send(ScriptCmd::Shutdown);
            }
            if let Some(flag) = &handle.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(service) = &handle.service {
                service.shutdown();
            }
        }
    }

    /// 标记子连接为已断开（由子通道 I/O 回调调用）。
    pub fn mark_sub_disconnected(
        &mut self,
        parent_id: &str,
        channel_id: &str,
        retain_terminal: bool,
    ) {
        if let Some(handle) = self.sessions.get_mut(parent_id) {
            if let Some(sub) = handle
                .sub_connections
                .iter_mut()
                .find(|sub| sub.id == channel_id)
            {
                sub.state = SessionState::Disconnected;
                sub.retain_terminal = retain_terminal;
                if let Some(flag) = &sub.stats_cancel_flag {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    /// 启动 I/O 统计采集器（使用 std::thread + AtomicBool 取消，无需 tokio runtime）。
    fn start_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        io: Arc<SessionIo>,
        connected_at: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        Self::spawn_stats_collector(app_handle, tab_id, io, connected_at, cancel_flag);
    }

    pub fn spawn_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        io: Arc<SessionIo>,
        connected_at: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        std::thread::spawn(move || {
            let mut last_tx = 0;
            let mut last_rx = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
                let tx = io.tx_bytes();
                let rx = io.rx_bytes();
                if tx != last_tx || rx != last_rx {
                    last_tx = tx;
                    last_rx = rx;
                    let _ = app_handle.emit(
                        "session-stats",
                        SessionStats {
                            tab_id: tab_id.clone(),
                            tx_bytes: tx,
                            rx_bytes: rx,
                            connected_at,
                        },
                    );
                }
            }
        });
    }

    /// 获取会话持久化文件路径
    pub fn sessions_file_path(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
        let path = app_handle
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法解析应用数据目录: {}", error))?;
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("无法创建应用数据目录 {:?}: {}", path, error))?;
        Ok(path.join("sessions.json"))
    }

    fn write_library(path: &std::path::Path, sessions: Vec<SavedSession>) -> Result<(), String> {
        let snapshot = SessionLibraryFile {
            version: SESSION_LIBRARY_VERSION,
            sessions,
        };
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| format!("序列化会话库失败: {}", e))?;
        atomic_write(path, json.as_bytes()).map_err(|e| format!("写入会话库失败: {}", e))
    }

    fn backup_invalid_library(path: &std::path::Path) -> Result<(), String> {
        let backup = path.with_extension("json.invalid.bak");
        std::fs::copy(path, &backup)
            .map(|_| ())
            .map_err(|e| format!("备份无效会话库失败: {}", e))
    }

    /// 从磁盘加载当前 Session Library schema。
    ///
    /// 研发阶段不迁移旧的裸数组格式：版本不匹配或格式损坏都会备份并从空 Library 开始。
    pub fn load_from_disk(path: &std::path::Path) -> Result<Vec<SavedSession>, String> {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        Self::load_from_disk_unlocked(path)
    }

    fn load_from_disk_unlocked(path: &std::path::Path) -> Result<Vec<SavedSession>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("读取会话库失败: {}", e))?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        match serde_json::from_str::<SessionLibraryFile>(&content) {
            Ok(library) if library.version == SESSION_LIBRARY_VERSION => Ok(library.sessions),
            Ok(library) => {
                Self::backup_invalid_library(path)?;
                log::warn!(
                    "会话库版本 {} 不受支持（expected {}），已备份并从空 Library 启动",
                    library.version,
                    SESSION_LIBRARY_VERSION
                );
                Ok(Vec::new())
            }
            Err(error) => {
                Self::backup_invalid_library(path)?;
                log::warn!("会话库格式无效 ({})，已备份；开发阶段不迁移旧格式", error);
                Ok(Vec::new())
            }
        }
    }

    /// 以给定安全快照完整覆盖 Session Library。
    pub fn replace_saved_sessions(
        path: &std::path::Path,
        sessions: &[SavedSession],
    ) -> Result<(), String> {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        Self::write_library(path, sessions.to_vec())
    }

    /// 保存单个 Session 配置到磁盘 Library（合并写入，不依赖运行时状态）。
    pub fn save_config_to_disk(
        app_handle: &tauri::AppHandle,
        session: SavedSession,
    ) -> Result<(), String> {
        Self::save_config_to_disk_transactional(app_handle, session, || Ok(()))
    }

    /// 保存 Session Library 后执行一个外部提交步骤；外部步骤失败时恢复原 Library。
    pub fn save_config_to_disk_transactional<F>(
        app_handle: &tauri::AppHandle,
        session: SavedSession,
        post_commit: F,
    ) -> Result<(), String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle)?;
        let existing = Self::load_from_disk_unlocked(&path)?;
        let mut next = existing.clone();
        next.retain(|entry| entry.id != session.id);
        next.push(session);
        next.sort_by_key(|entry| entry.timestamp);
        Self::write_library(&path, next)?;

        if let Err(commit_error) = post_commit() {
            return match Self::write_library(&path, existing) {
                Ok(()) => Err(commit_error),
                Err(rollback_error) => Err(format!(
                    "{}；Session Library 回滚失败: {}",
                    commit_error, rollback_error
                )),
            };
        }
        Ok(())
    }

    /// 重命名一个 Saved Session，并在外部运行态更新失败时恢复原 Library。
    pub fn rename_config_on_disk_transactional<F>(
        app_handle: &tauri::AppHandle,
        session_id: &str,
        new_name: &str,
        post_commit: F,
    ) -> Result<(), String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle)?;
        let existing = Self::load_from_disk_unlocked(&path)?;
        let mut next = existing.clone();
        let target = next
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| format!("Saved Session 不存在: {}", session_id))?;
        target.name = new_name.to_string();
        Self::write_library(&path, next)?;

        if let Err(commit_error) = post_commit() {
            return match Self::write_library(&path, existing) {
                Ok(()) => Err(commit_error),
                Err(rollback_error) => Err(format!(
                    "{}；Session Library 回滚失败: {}",
                    commit_error, rollback_error
                )),
            };
        }
        Ok(())
    }

    /// 更新 Saved Session 的单个非敏感参数，并在运行态发布失败时恢复旧 Library。
    pub fn set_config_param_on_disk_transactional<F>(
        app_handle: &tauri::AppHandle,
        session_id: &str,
        key: &str,
        value: serde_json::Value,
        post_commit: F,
    ) -> Result<(), String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle)?;
        let existing = Self::load_from_disk_unlocked(&path)?;
        let mut next = existing.clone();
        let target = next
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| format!("Saved Session 不存在: {}", session_id))?;
        let params = target
            .params
            .as_object_mut()
            .ok_or_else(|| format!("Saved Session {} 参数不是 JSON object", session_id))?;
        params.insert(key.to_string(), value);
        Self::write_library(&path, next)?;

        if let Err(commit_error) = post_commit() {
            return match Self::write_library(&path, existing) {
                Ok(()) => Err(commit_error),
                Err(rollback_error) => Err(format!(
                    "{}；Session Library 回滚失败: {}",
                    commit_error, rollback_error
                )),
            };
        }
        Ok(())
    }

    /// 从磁盘 Session Library 删除指定配置。
    pub fn delete_config_from_disk(
        app_handle: &tauri::AppHandle,
        session_id: &str,
    ) -> Result<(), String> {
        Self::delete_config_from_disk_transactional(app_handle, session_id, |_| Ok(()))
    }

    /// 删除 Session Library 条目后执行外部清理；外部步骤失败时恢复原 Library。
    pub fn delete_config_from_disk_transactional<F>(
        app_handle: &tauri::AppHandle,
        session_id: &str,
        post_commit: F,
    ) -> Result<(), String>
    where
        F: FnOnce(Option<&SavedSession>) -> Result<(), String>,
    {
        let _guard = SESSIONS_FILE_MUTEX
            .lock()
            .map_err(|e| format!("获取文件锁失败: {}", e))?;
        let path = Self::sessions_file_path(app_handle)?;
        let existing = Self::load_from_disk_unlocked(&path)?;
        let target = existing
            .iter()
            .find(|session| session.id == session_id)
            .cloned();
        let filtered: Vec<_> = existing
            .iter()
            .filter(|session| session.id != session_id)
            .cloned()
            .collect();
        Self::write_library(&path, filtered)?;

        if let Err(commit_error) = post_commit(target.as_ref()) {
            return match Self::write_library(&path, existing) {
                Ok(()) => Err(commit_error),
                Err(rollback_error) => Err(format!(
                    "{}；Session Library 回滚失败: {}",
                    commit_error, rollback_error
                )),
            };
        }
        Ok(())
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

#[cfg(test)]
mod persistence_tests {
    use super::*;

    fn sample_saved_session(id: &str, timestamp: u64) -> SavedSession {
        SavedSession {
            id: id.to_string(),
            name: format!("session-{id}"),
            plugin_id: "dummy".into(),
            endpoint: "loopback".into(),
            params: serde_json::json!({
                "encoding": "utf-8",
                "data_mode": "text",
                "option": 42
            }),
            timestamp,
            transfer_enabled: false,
            transfer_protocol: None,
            send_bar_enabled: true,
        }
    }

    #[test]
    fn saved_session_library_round_trips_versioned_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let expected = vec![sample_saved_session("a", 10), sample_saved_session("b", 20)];

        SessionStore::replace_saved_sessions(&path, &expected).unwrap();
        let loaded = SessionStore::load_from_disk(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "a");
        assert_eq!(loaded[1].id, "b");
        assert_eq!(loaded[0].params["option"], 42);
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(raw.contains("\"version\": 2"));
        assert!(raw.contains("\"sessions\""));
    }

    #[test]
    fn malformed_library_is_backed_up_before_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        std::fs::write(&path, "{ definitely not json").unwrap();

        let loaded = SessionStore::load_from_disk(&path).unwrap();

        assert!(loaded.is_empty());
        assert!(path.with_extension("json.invalid.bak").exists());
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.invalid.bak")).unwrap(),
            "{ definitely not json"
        );
    }

    #[test]
    fn unsupported_library_version_is_backed_up_and_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        std::fs::write(&path, r#"{"version":99,"sessions":[]}"#).unwrap();

        let loaded = SessionStore::load_from_disk(&path).unwrap();

        assert!(loaded.is_empty());
        assert!(path.with_extension("json.invalid.bak").exists());
    }
}
