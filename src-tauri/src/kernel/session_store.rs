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

pub type TabId = String;

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
    pub io_thread: Option<std::thread::JoinHandle<()>>,
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

    /// 创建新会话（使用已打开的 Channel）
    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &mut self,
        name: &str,
        plugin_id: &str,
        endpoint: &str,
        params: serde_json::Value,
        channel: Box<dyn Channel>,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
        transfer_enabled: bool,
        transfer_protocol: Option<String>,
        send_bar_enabled: bool,
        // 可选：传入已有的 session_id 以原地重连（保留 UUID）
        id_override: Option<String>,
    ) -> Result<TabId, String> {
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

        let (write_tx, write_rx) = mpsc::sync_channel::<IoLoopCmd>(32);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        let tx_clone = tx_bytes.clone();
        let rx_clone = rx_bytes.clone();

        let sid = id.clone();
        let io_handle = spawn_sync_io_loop(
            channel, sid, on_data, on_disconnect, write_rx, cancel_rx,
            tx_clone, rx_clone,
        );

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
        };

        // 防御性检查：若 id_override 指向的会话已存在且未被正确关闭，
        // 先清理旧会话，防止静默覆盖导致 I/O 线程、串口句柄、定时器等资源泄漏。
        // 显式 drop() 确保 SessionHandle 的 Drop 实现（关闭 I/O 线程/句柄）
        // 在新 session 插入前执行，避免新旧会话并发持有同一硬件资源。
        if let Some(old_handle) = self.sessions.remove(&id) {
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

    /// 关闭指定会话
    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
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
        if let Some(thread) = handle.io_thread.take() {
            let _ = thread.join();
        }

        #[cfg(target_os = "windows")]
        std::thread::sleep(std::time::Duration::from_millis(100));

        self.tab_order.retain(|id| id != session_id);
        if self.active_id.as_deref() == Some(session_id) {
            self.active_id = self.tab_order.first().cloned();
        }

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

        let (write_tx, write_rx) = mpsc::sync_channel::<IoLoopCmd>(32);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));

        let sid = session_id.to_string();
        let io_handle = spawn_sync_io_loop(
            channel, sid, on_data, on_disconnect, write_rx, cancel_rx,
            tx_bytes.clone(), rx_bytes.clone(),
        );

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

    /// 获取会话状态
    pub fn session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).map(|h| h.state.clone())
    }

    /// 标记会话为已断开
    pub fn mark_disconnected(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.state = SessionState::Disconnected;
            handle.io_cancel_tx = None;
            handle.io_thread = None;
            if let Some(ref flag) = handle.stats_cancel_flag {
                flag.store(true, Ordering::SeqCst);
            }
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
