//! 会话存储
//!
//! SessionStore owns user-visible session lifecycle. Physical I/O resources are owned by
//! `SessionDataPlane`; callers interact through `SessionIo` capabilities.

use crate::kernel::file_transfer::FileTransfer;
use crate::kernel::persistence::atomic_write;
use crate::kernel::plugin_adapter::{
    ProtocolConnection, SessionAttach, SessionChannelFactory, SessionService,
};
use crate::kernel::script_engine::{ScriptCleanup, ScriptRuntime};
use crate::session::{
    AutomationIo, DisconnectInfo, RuntimeId, RuntimeIdentity, SessionDataPlane, SessionIo,
};
use crate::transfer::scheduler::TransferScheduler;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub type TabId = String;

/// 会话状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
}

/// Deferred child cleanup. The DataPlane owner is moved out while SessionStore is locked and
/// joined only after the caller releases that lock.
pub struct SubConnectionCleanup {
    channel_id: String,
    data_plane: Option<SessionDataPlane>,
    script: Option<ScriptCleanup>,
}

impl SubConnectionCleanup {
    pub fn join(mut self) {
        if let Some(mut data_plane) = self.data_plane.take() {
            data_plane.shutdown();
            log::debug!("子连接 DataPlane 已清理: {}", self.channel_id);
        }
        if let Some(script) = self.script.take() {
            script.join();
        }
    }
}

/// Runtime resources detached atomically from the SessionStore. Shutdown and joins must run
/// after releasing its lock; protocol-owned resources remain opaque capabilities here.
#[must_use = "Session cleanup must be awaited or scheduled outside the SessionStore lock"]
pub struct SessionCleanup {
    identity: RuntimeIdentity,
    children: Vec<SubConnectionCleanup>,
    data_plane: Option<SessionDataPlane>,
    script: Option<ScriptCleanup>,
    transfers: Vec<tokio::task::JoinHandle<()>>,
    service: Option<Arc<dyn SessionService>>,
    attachment: Option<Arc<dyn SessionAttach>>,
    teardown_delay: Duration,
}

impl SessionCleanup {
    pub fn identity(&self) -> &RuntimeIdentity {
        &self.identity
    }
    fn join_resources(mut self) {
        for child in self.children.drain(..) {
            child.join();
        }
        if let Some(mut data_plane) = self.data_plane.take() {
            data_plane.shutdown();
        }
        if let Some(script) = self.script.take() {
            script.join();
        }
        if !self.teardown_delay.is_zero() {
            std::thread::sleep(self.teardown_delay);
        }
        if let Some(service) = self.service.take() {
            service.shutdown();
        }
        if let Some(attachment) = self.attachment.take() {
            attachment.on_detached(&self.identity.session_id);
        }
    }

    pub async fn finish(mut self) -> Result<(), String> {
        let transfers = std::mem::take(&mut self.transfers);
        let session_id = self.identity.session_id.clone();
        tauri::async_runtime::spawn_blocking(move || self.join_resources())
            .await
            .map_err(|error| format!("等待会话资源清理失败: {error}"))?;
        for task in transfers {
            if tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .is_err()
            {
                log::warn!("传输 task join 超时 (session: {session_id})");
            }
        }
        Ok(())
    }

    /// For synchronous lifecycle callbacks; the scheduled task owns all detached resources.
    pub fn schedule(self) {
        tauri::async_runtime::spawn(async move {
            if let Err(error) = self.finish().await {
                log::error!("{error}");
            }
        });
    }
}

/// I/O 统计快照
#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub runtime_id: RuntimeId,
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
    published: bool,
    pub runtime_id: RuntimeId,
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
    pub script: Option<ScriptRuntime>,
    pub visible_in_workspace: bool,
}

pub struct ActiveSessionHandle {
    published: bool,
    pub runtime_id: RuntimeId,
    pub id: TabId,
    pub name: String,
    pub data_plane: Option<SessionDataPlane>,
    pub io: Option<Arc<SessionIo>>,
    /// Protocol-neutral SendBar/script capability. For ordinary streams this is the same SessionIo;
    /// multiplexed container plugins may provide an independent implementation.
    pub automation_io: Option<Arc<dyn AutomationIo>>,
    pub state: SessionState,
    pub plugin_id: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub connected_at: Option<u64>,
    pub stats_cancel_flag: Option<Arc<AtomicBool>>,
    pub transfer_enabled: bool,
    pub transfer_protocol: Option<String>,
    pub send_bar_enabled: bool,
    pub script: Option<ScriptRuntime>,
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
            runtime_id: data_plane.identity().runtime_id,
            published: false,
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
            script: None,
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
        if let Some(script) = &self.script {
            script.cancel();
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
            if let Some(script) = &sub.script {
                script.cancel();
            }
        }
    }
}

/// 会话存储
pub struct SessionStore {
    next_runtime_id: u64,
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
    pub automation_io: Option<Arc<dyn AutomationIo>>,
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
            next_runtime_id: 1,
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
        on_data: Box<dyn Fn(RuntimeIdentity, Vec<u8>) + Send + 'static>,
        on_disconnect: Box<dyn Fn(RuntimeIdentity, DisconnectInfo) + Send + 'static>,
        app_handle: tauri::AppHandle,
    ) -> Result<RuntimeIdentity, String> {
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
        let identity = self.allocate_runtime_identity(id.clone())?;
        let data_plane =
            SessionDataPlane::attach_paused(runtime, identity.clone(), on_data, on_disconnect)
                .map_err(|e| e.to_string())?;
        let stats_cancel_flag = Arc::new(AtomicBool::new(false));
        Self::start_stats_collector(
            app_handle,
            identity.clone(),
            io.clone(),
            connected_at,
            stats_cancel_flag.clone(),
        );
        if let Some(attach) = conn.on_attached.as_ref() {
            attach.on_attached(&id);
        }
        let handle = ActiveSessionHandle {
            published: false,
            runtime_id: identity.runtime_id,
            id: id.clone(),
            name: tab_name.clone(),
            data_plane: Some(data_plane),
            io: Some(io.clone()),
            automation_io: Some(io as Arc<dyn AutomationIo>),
            state: SessionState::Connected,
            plugin_id,
            endpoint,
            params,
            connected_at,
            stats_cancel_flag: Some(stats_cancel_flag),
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            script: None,
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
        Ok(identity)
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
    ) -> Result<RuntimeIdentity, String> {
        let ContainerSessionRuntime {
            service,
            file_transfer,
            channel_factory,
            io,
            automation_io,
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
        let identity = self.allocate_runtime_identity(id.clone())?;
        let handle = ActiveSessionHandle {
            published: false,
            runtime_id: identity.runtime_id,
            id: id.clone(),
            name: session_name.clone(),
            data_plane: None,
            io,
            automation_io,
            state: SessionState::Connected,
            plugin_id,
            endpoint,
            params,
            connected_at,
            stats_cancel_flag: None,
            transfer_enabled,
            transfer_protocol,
            send_bar_enabled,
            script: None,
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
        Ok(identity)
    }

    pub fn close_session(&mut self, session_id: &str) -> Result<SessionCleanup, String> {
        let mut handle = self
            .sessions
            .remove(session_id)
            .ok_or_else(|| self.session_not_found(session_id))?;
        let mut children = Vec::new();
        for mut sub in std::mem::take(&mut handle.sub_connections) {
            if let Some(flag) = &sub.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
            children.push(SubConnectionCleanup {
                channel_id: sub.id,
                data_plane: sub.data_plane.take(),
                script: sub.script.take().map(ScriptRuntime::into_cleanup),
            });
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
        if let Some(flag) = &handle.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
        }
        let cleanup = SessionCleanup {
            identity: RuntimeIdentity {
                session_id: session_id.to_string(),
                runtime_id: handle.runtime_id,
            },
            children,
            data_plane: handle.data_plane.take(),
            script: handle.script.take().map(ScriptRuntime::into_cleanup),
            transfers: std::mem::take(&mut handle.transfer_tasks),
            service: handle.service.take(),
            attachment: handle.attachment.take(),
            teardown_delay: handle.teardown_delay,
        };
        self.tab_order.retain(|id| id != session_id);
        if self.active_id.as_deref() == Some(session_id) {
            self.active_id = self.tab_order.first().cloned();
        }
        handle.file_transfer = None;
        handle.channel_factory = None;
        handle.io = None;
        handle.automation_io = None;
        handle.state = SessionState::Disconnected;
        self.sessions.insert(session_id.to_string(), handle);
        Ok(cleanup)
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
        let handle = self
            .sessions
            .get_mut(parent_id)
            .ok_or_else(|| not_found.clone())?;
        if handle.state != SessionState::Connected || handle.runtime_id != sub.runtime_id {
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

    /// Capture I/O only for the connected instance named by the caller.
    pub fn io_for_runtime(&self, identity: &RuntimeIdentity) -> Result<Arc<SessionIo>, String> {
        if !self.runtime_is_connected(identity) {
            return Err("会话运行时已变化或断开".into());
        }
        self.get_io_for(&identity.session_id)
            .ok_or_else(|| "当前会话没有 I/O 能力".into())
    }

    /// Activate a previously registered root or child DataPlane.
    ///
    /// Registration-sensitive connectors publish SessionStore state and their connected event
    /// before releasing this barrier, so data/disconnect callbacks cannot race ahead of them.
    pub fn activate_data_plane(&mut self, identity: &RuntimeIdentity) -> Result<(), String> {
        if !self.matches_runtime(identity) {
            return Err("会话运行时已变化，拒绝激活旧连接".into());
        }
        if let Some(root) = self.sessions.get_mut(&identity.session_id) {
            root.published = true;
            if let Some(owner) = &root.data_plane {
                owner.activate();
            }
            for child in &root.sub_connections {
                if child.published {
                    if let Some(owner) = &child.data_plane {
                        owner.activate();
                    }
                }
            }
            return Ok(());
        }
        let (parent_id, index) = self
            .find_sub_connection_index(&identity.session_id)
            .ok_or("子会话不存在")?;
        let root = self.sessions.get_mut(&parent_id).ok_or("父会话不存在")?;
        let child = &mut root.sub_connections[index];
        child.published = true;
        if root.published {
            if let Some(owner) = &child.data_plane {
                owner.activate();
            }
        }
        Ok(())
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
        let script = sub.script.take().map(ScriptRuntime::into_cleanup);
        if let Some(flag) = &sub.stats_cancel_flag {
            flag.store(true, Ordering::SeqCst);
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
                script,
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

    /// 启动脚本引擎（首次启动创建线程，后续发送新脚本）。
    ///
    /// 脚本只依赖 AutomationIo。普通流式 Session 由 SessionIo 适配；RTT 等容器
    /// Session 可以提供自己的多路复用自动化能力，而不需要伪造根 DataPlane。
    pub fn start_script(
        &mut self,
        session_id: &str,
        code: &str,
        app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        let not_found = self.session_not_found(session_id);
        let (script, automation) = if let Some(handle) = self.sessions.get_mut(session_id) {
            let automation = handle
                .automation_io
                .clone()
                .ok_or("当前会话不提供自动化 I/O 能力")?;
            (&mut handle.script, automation)
        } else {
            let (parent_id, sub_idx) = self
                .find_sub_connection_index(session_id)
                .ok_or(not_found)?;
            let handle = self.sessions.get_mut(&parent_id).ok_or("父会话不存在")?;
            let sub = &mut handle.sub_connections[sub_idx];
            let automation: Arc<dyn AutomationIo> = sub.io.clone();
            (&mut sub.script, automation)
        };
        if let Some(runtime) = script {
            return runtime.load(code);
        }
        *script = Some(ScriptRuntime::start(
            automation, app_handle, session_id, code,
        )?);
        Ok(())
    }

    /// Cancel under the store lock; the caller joins the returned owner outside it.
    pub fn stop_script(&mut self, session_id: &str) -> Result<Option<ScriptCleanup>, String> {
        let not_found = self.session_not_found(session_id);
        let script = if let Some(handle) = self.sessions.get_mut(session_id) {
            &mut handle.script
        } else {
            let (parent_id, sub_idx) = self
                .find_sub_connection_index(session_id)
                .ok_or(not_found)?;
            let handle = self.sessions.get_mut(&parent_id).ok_or("父会话不存在")?;
            &mut handle.sub_connections[sub_idx].script
        };
        Ok(script.take().map(ScriptRuntime::into_cleanup))
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
    pub fn transfer_done(&mut self, identity: &RuntimeIdentity, transfer_id: Option<&str>) {
        if !self.matches_runtime(identity) {
            return;
        }
        let session_id = identity.session_id.as_str();
        if let Some(handle) = self.sessions.get_mut(session_id) {
            let _ = handle.transfer_scheduler.finish(transfer_id);
        }
    }

    /// 注册传输 task 的 JoinHandle，供 close_session 等待完成。
    pub fn register_transfer_task(
        &mut self,
        identity: &RuntimeIdentity,
        handle: tokio::task::JoinHandle<()>,
    ) -> Result<(), String> {
        if !self.runtime_is_connected(identity) {
            return Err("传输所属连接已关闭或变化".into());
        }
        let session_id = identity.session_id.as_str();
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

    /// Commit only to the originating connection and reuse the canonical cleanup owner.
    pub fn mark_disconnected(&mut self, identity: &RuntimeIdentity) -> Option<SessionCleanup> {
        let handle = self.sessions.get(&identity.session_id)?;
        if handle.runtime_id != identity.runtime_id || handle.state != SessionState::Connected {
            return None;
        }
        self.close_session(&identity.session_id).ok()
    }

    pub fn mark_sub_disconnected(
        &mut self,
        parent: &RuntimeIdentity,
        channel_id: &str,
        retain_terminal: bool,
    ) -> bool {
        let Some(handle) = self.sessions.get_mut(&parent.session_id) else {
            return false;
        };
        if handle.runtime_id != parent.runtime_id || handle.state != SessionState::Connected {
            return false;
        }
        let Some(sub) = handle
            .sub_connections
            .iter_mut()
            .find(|sub| sub.id == channel_id)
        else {
            return false;
        };
        if sub.state != SessionState::Connected {
            return false;
        }
        sub.state = SessionState::Disconnected;
        sub.retain_terminal = retain_terminal;
        if let Some(script) = &sub.script {
            script.cancel();
        }
        if let Some(flag) = &sub.stats_cancel_flag {
            flag.store(true, Ordering::Release);
        }
        true
    }

    fn allocate_runtime_identity(&mut self, session_id: String) -> Result<RuntimeIdentity, String> {
        let id = self.next_runtime_id;
        self.next_runtime_id = id
            .checked_add(1)
            .ok_or("Session runtime identity exhausted")?;
        Ok(RuntimeIdentity {
            session_id,
            runtime_id: RuntimeId(id),
        })
    }

    pub fn runtime_identity(&self, session_id: &str) -> Option<RuntimeIdentity> {
        if let Some(root) = self.sessions.get(session_id) {
            return Some(RuntimeIdentity {
                session_id: root.id.clone(),
                runtime_id: root.runtime_id,
            });
        }
        let (parent_id, index) = self.find_sub_connection_index(session_id)?;
        let child = self.sessions.get(&parent_id)?.sub_connections.get(index)?;
        Some(RuntimeIdentity {
            session_id: child.id.clone(),
            runtime_id: child.runtime_id,
        })
    }

    pub fn matches_runtime(&self, identity: &RuntimeIdentity) -> bool {
        self.runtime_identity(&identity.session_id).as_ref() == Some(identity)
    }

    pub fn runtime_is_connected(&self, identity: &RuntimeIdentity) -> bool {
        if let Some(root) = self.sessions.get(&identity.session_id) {
            return root.runtime_id == identity.runtime_id && root.state == SessionState::Connected;
        }
        let Some((parent_id, index)) = self.find_sub_connection_index(&identity.session_id) else {
            return false;
        };
        let Some(root) = self.sessions.get(&parent_id) else {
            return false;
        };
        root.runtime_id == identity.runtime_id
            && root.state == SessionState::Connected
            && root.sub_connections[index].state == SessionState::Connected
    }

    pub fn close_if_current(
        &mut self,
        identity: &RuntimeIdentity,
    ) -> Result<SessionCleanup, String> {
        if !self.matches_runtime(identity) {
            return Err("会话运行时已变化，拒绝关闭旧连接".into());
        }
        self.close_session(&identity.session_id)
    }

    /// 启动 I/O 统计采集器（使用 std::thread + AtomicBool 取消，无需 tokio runtime）。
    fn start_stats_collector(
        app_handle: tauri::AppHandle,
        identity: RuntimeIdentity,
        io: Arc<SessionIo>,
        connected_at: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        Self::spawn_stats_collector(app_handle, identity, io, connected_at, cancel_flag);
    }

    pub fn spawn_stats_collector(
        app_handle: tauri::AppHandle,
        identity: RuntimeIdentity,
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
                            tab_id: identity.session_id.clone(),
                            runtime_id: identity.runtime_id,
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
            let _ = self.close_session(&id).map(SessionCleanup::schedule);
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    fn create_test_container(store: &mut SessionStore, id: &str) -> RuntimeIdentity {
        store
            .create_container_session(
                ContainerSessionCreateOptions {
                    name: "test".into(),
                    plugin_id: "dummy".into(),
                    endpoint: "local".into(),
                    params: serde_json::json!({}),
                    transfer_enabled: false,
                    transfer_protocol: None,
                    send_bar_enabled: false,
                    id_override: Some(id.into()),
                },
                ContainerSessionRuntime {
                    service: None,
                    file_transfer: None,
                    channel_factory: None,
                    io: None,
                    automation_io: None,
                    attachment: None,
                    teardown_delay: Duration::ZERO,
                },
            )
            .unwrap()
    }

    struct TestStream(std::sync::mpsc::Receiver<Vec<u8>>);

    impl crate::transport::BlockingByteStream for TestStream {
        fn read(
            &mut self,
            buf: &mut [u8],
        ) -> Result<crate::transport::ReadStatus, crate::transport::TransportError> {
            match self.0.recv_timeout(Duration::from_millis(1)) {
                Ok(bytes) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(crate::transport::ReadStatus::Data(bytes.len()))
                }
                Err(_) => Ok(crate::transport::ReadStatus::Idle),
            }
        }
        fn write_all(&mut self, _: &[u8]) -> Result<(), crate::transport::TransportError> {
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::transport::TransportError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), crate::transport::TransportError> {
            Ok(())
        }
    }

    fn test_child(
        identity: &RuntimeIdentity,
    ) -> (
        SubConnection,
        std::sync::mpsc::Sender<Vec<u8>>,
        std::sync::mpsc::Receiver<RuntimeIdentity>,
    ) {
        let (input, rx) = std::sync::mpsc::channel();
        let runtime = crate::transport::DataPlaneRuntime::spawn(Box::new(TestStream(rx)));
        let io = Arc::new(SessionIo::new(Some(runtime.handle.clone()), None, "utf-8"));
        let (delivered, delivery) = std::sync::mpsc::channel();
        let plane = SessionDataPlane::attach_paused(
            runtime,
            identity.clone(),
            Box::new(move |identity, _| {
                let _ = delivered.send(identity);
            }),
            Box::new(|_, _| {}),
        )
        .unwrap();
        (
            SubConnection::new(
                identity.session_id.clone(),
                "child".into(),
                plane,
                io,
                0,
                false,
            ),
            input,
            delivery,
        )
    }

    #[test]
    fn stale_runtime_cannot_close_replace_or_register_children_of_a_new_connection() {
        let mut store = SessionStore::new();
        let id = uuid::Uuid::new_v4().to_string();
        let old = create_test_container(&mut store, &id);
        store.close_if_current(&old).unwrap().join_resources();
        let current = create_test_container(&mut store, &id);
        assert!(current.runtime_id.0 > old.runtime_id.0);
        assert!(store.close_if_current(&old).is_err());
        assert!(store.mark_disconnected(&old).is_none());
        assert!(store.activate_data_plane(&old).is_err());
        let (child, _, _) = test_child(&old.for_session("old-child".into()));
        assert!(store.add_sub_connection(&id, child).is_err());
        assert!(store.runtime_is_connected(&current));
        assert!(store.sessions[&id].sub_connections.is_empty());
        let child_id = "io-child".to_string();
        let (child, _, _) = test_child(&current.for_session(child_id.clone()));
        store.add_sub_connection(&id, child).unwrap();
        assert!(store
            .io_for_runtime(&old.for_session(child_id.clone()))
            .is_err());
        assert!(store
            .io_for_runtime(&current.for_session(child_id.clone()))
            .is_ok());
        store.close_if_current(&current).unwrap().join_resources();
        assert!(store
            .io_for_runtime(&current.for_session(child_id))
            .is_err());
        store.sessions.clear();
    }

    #[test]
    fn child_data_waits_for_both_parent_and_child_publication_in_either_order() {
        for parent_first in [false, true] {
            let mut store = SessionStore::new();
            let parent = create_test_container(&mut store, &uuid::Uuid::new_v4().to_string());
            let identity = parent.for_session("child".into());
            let (child, input, delivery) = test_child(&identity);
            store.add_sub_connection(&parent.session_id, child).unwrap();
            input.send(vec![42]).unwrap();
            let (first, second) = if parent_first {
                (&parent, &identity)
            } else {
                (&identity, &parent)
            };
            store.activate_data_plane(first).unwrap();
            assert!(delivery.recv_timeout(Duration::from_millis(30)).is_err());
            store.activate_data_plane(second).unwrap();
            assert_eq!(
                delivery.recv_timeout(Duration::from_secs(2)).unwrap(),
                identity
            );
            store.close_if_current(&parent).unwrap().join_resources();
            store.sessions.clear();
        }
    }

    #[test]
    fn closing_a_slow_service_releases_the_store_before_shutdown() {
        use std::sync::{mpsc, Mutex};
        struct SlowService {
            started: mpsc::Sender<()>,
            release: Mutex<mpsc::Receiver<()>>,
        }
        impl SessionService for SlowService {
            fn shutdown(&self) {
                self.started.send(()).expect("started");
                self.release
                    .lock()
                    .expect("release lock")
                    .recv_timeout(Duration::from_secs(2))
                    .expect("release cleanup");
            }
        }
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut store = SessionStore::new();
        let id = store
            .create_container_session(
                ContainerSessionCreateOptions {
                    name: "slow".into(),
                    plugin_id: "dummy".into(),
                    endpoint: "local".into(),
                    params: serde_json::json!({}),
                    transfer_enabled: false,
                    transfer_protocol: None,
                    send_bar_enabled: false,
                    id_override: None,
                },
                ContainerSessionRuntime {
                    service: Some(Arc::new(SlowService {
                        started: started_tx,
                        release: Mutex::new(release_rx),
                    })),
                    file_transfer: None,
                    channel_factory: None,
                    io: None,
                    automation_io: None,
                    attachment: None,
                    teardown_delay: Duration::ZERO,
                },
            )
            .expect("session");
        let store = Arc::new(Mutex::new(store));
        let cleanup = store
            .lock()
            .expect("store")
            .close_if_current(&id)
            .expect("detach resources");
        assert!(
            started_rx.try_recv().is_err(),
            "close_session must not call plugin shutdown under the lock"
        );
        let worker = std::thread::spawn(move || cleanup.join_resources());
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shutdown started");
        {
            let mut unlocked = store
                .try_lock()
                .expect("sibling Session operations remain available");
            assert_eq!(
                unlocked.session_state(&id.session_id),
                Some(SessionState::Disconnected)
            );
            unlocked.sessions.clear();
        }
        release_tx.send(()).expect("finish service");
        worker.join().expect("cleanup worker");
    }

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
