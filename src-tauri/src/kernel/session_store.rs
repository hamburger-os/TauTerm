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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
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
}

/// 会话存储
pub struct SessionStore {
    sessions: HashMap<TabId, ActiveSessionHandle>,
    active_id: Option<TabId>,
    tab_order: Vec<TabId>,
    max_sessions: usize,
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
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            active_id: None,
            tab_order: Vec::new(),
            max_sessions: 10,
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
    ) -> Result<TabId, String> {
        if self.sessions.len() >= self.max_sessions {
            return Err(format!("已达到最大会话数限制 ({})", self.max_sessions));
        }

        let id = uuid::Uuid::new_v4().to_string();
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

        // 启动 StatsCollector
        let (stats_cancel_tx, stats_cancel_rx) = tokio::sync::oneshot::channel::<()>();
        Self::start_stats_collector(
            app_handle.clone(),
            id.clone(),
            tx_bytes.clone(),
            rx_bytes.clone(),
            connected_at,
            stats_cancel_rx,
        );

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
            stats_cancel_tx: Some(stats_cancel_tx),
        };

        self.sessions.insert(id.clone(), handle);
        self.tab_order.push(id.clone());
        self.active_id = Some(id.clone());

        Ok(id)
    }

    /// 关闭指定会话
    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
        let mut handle = self.sessions.remove(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

        // 取消正在进行的传输
        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        // 取消 StatsCollector
        if let Some(tx) = handle.stats_cancel_tx.take() {
            let _ = tx.send(());
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

    /// 切换到指定会话
    pub fn switch_active(&mut self, session_id: &str) -> Result<(), String> {
        if !self.sessions.contains_key(session_id) {
            return Err(format!("会话 {} 不存在", session_id));
        }
        self.active_id = Some(session_id.to_string());
        Ok(())
    }

    /// 重命名会话
    pub fn rename_session(&mut self, session_id: &str, new_name: &str) -> Result<(), String> {
        let handle = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
        handle.name = new_name.to_string();
        Ok(())
    }

    /// 标签页重排序
    pub fn reorder_tabs(&mut self, new_order: Vec<TabId>) -> Result<(), String> {
        for id in &new_order {
            if !self.sessions.contains_key(id) {
                return Err(format!("会话 {} 不存在", id));
            }
        }
        self.tab_order = new_order;
        Ok(())
    }

    /// 向指定会话写入数据
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let handle = self.sessions.get(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
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
        }).collect()
    }

    /// 重连指定会话
    pub fn reconnect_session(
        &mut self,
        session_id: &str,
        channel: Box<dyn Channel>,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
    ) -> Result<serde_json::Value, String> {
        let handle = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

        if let Some(tx) = handle.stats_cancel_tx.take() {
            let _ = tx.send(());
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

        let (stats_cancel_tx, stats_cancel_rx) = tokio::sync::oneshot::channel::<()>();
        Self::start_stats_collector(
            app_handle.clone(),
            session_id.to_string(),
            tx_bytes.clone(),
            rx_bytes.clone(),
            connected_at,
            stats_cancel_rx,
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
        handle.stats_cancel_tx = Some(stats_cancel_tx);

        Ok(params)
    }

    /// 取消传输
    pub fn cancel_transfer(&mut self, session_id: &str) -> Result<(), String> {
        let handle = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
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
            if let Some(tx) = handle.stats_cancel_tx.take() {
                let _ = tx.send(());
            }
        }
    }

    /// 启动 I/O 统计采集器
    fn start_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        tx_bytes: Arc<AtomicU64>,
        rx_bytes: Arc<AtomicU64>,
        connected_at: Option<u64>,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("StatsCollector: 无法创建 tokio runtime: {}", e);
                    return;
                }
            };

            rt.block_on(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                tokio::pin!(let cancel = cancel_rx;);

                let mut last_tx: u64 = 0;
                let mut last_rx: u64 = 0;

                loop {
                    tokio::select! {
                        _ = &mut cancel => break,
                        _ = interval.tick() => {
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
                    }
                }
            });
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

        let mut dedup: HashMap<(String, String), SavedSession> = HashMap::new();
        for s in merged {
            let key = (s.endpoint.clone(), s.plugin_id.clone());
            if current_ids.contains(&s.id) {
                dedup.insert(key, s);
            } else {
                dedup.entry(key).or_insert(s);
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
}

impl Drop for SessionStore {
    fn drop(&mut self) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let _ = self.close_session(&id);
        }
    }
}
