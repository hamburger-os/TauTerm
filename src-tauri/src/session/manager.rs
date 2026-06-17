//! 会话管理器
//!
//! `SessionManager` 管理所有活跃终端会话的生命周期。
//! 每个会话拥有独立的 I/O 线程和缓冲写入通道。
//! 支持多标签页（最多 10 个并发会话）。
//!
//! ## 架构
//!
//! SessionManager
//! ├── sessions: HashMap<TabId, SessionHandle>
//! ├── active_id: Option<TabId>
//! └── tab_order: Vec<TabId>
//!
//! SessionHandle
//! ├── id: TabId (uuid v4)
//! ├── name: String (用户可编辑标签)
//! ├── connection: Box<dyn TermSession>
//! ├── write_tx: SyncSender<IoCmd> (缓冲通道, capacity=32)
//! ├── io_cancel_tx: Option<oneshot::Sender>
//! ├── io_thread: Option<JoinHandle>
//! └── state: SessionState

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use crate::session::{ConnectionType, SessionImpl, SessionState, SessionStats};
use crate::session::serial::SerialSession;

/// 标签页/会话唯一标识符
pub type TabId = String;

/// I/O 线程命令
#[derive(Debug)]
pub enum IoCmd {
    Write(Vec<u8>),
    Shutdown,
    /// 将串口所有权移交给传输代码。
    /// `give_tx` 用于交出端口，`return_rx` 用于阻塞等待端口归还。
    HandoffPort {
        give_tx: std::sync::mpsc::SyncSender<Box<dyn serialport::SerialPort>>,
        return_rx: std::sync::mpsc::Receiver<Box<dyn serialport::SerialPort>>,
    },
}

/// 单个会话句柄
pub struct SessionHandle {
    pub id: TabId,
    pub name: String,
    pub session: SessionImpl,
    pub write_tx: mpsc::SyncSender<IoCmd>,
    pub io_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub cancel_transfer_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub io_thread: Option<std::thread::JoinHandle<()>>,
    pub state: SessionState,
    pub connection_type: ConnectionType,
    pub endpoint: String,
    pub params: serde_json::Value,
    /// 传输完成后用于归还串口端口给 I/O 线程的发送端
    pub port_return_tx: Option<mpsc::SyncSender<Box<dyn serialport::SerialPort>>>,
    /// I/O 统计原子计数器（I/O 线程写入，StatsCollector 读取）
    pub tx_bytes: Arc<AtomicU64>,
    pub rx_bytes: Arc<AtomicU64>,
    /// 会话建立连接时的时间戳（毫秒）
    pub connected_at: Option<u64>,
    /// 取消 StatsCollector 任务的发送端
    pub stats_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// 会话管理器
pub struct SessionManager {
    sessions: HashMap<TabId, SessionHandle>,
    active_id: Option<TabId>,
    tab_order: Vec<TabId>,
    max_sessions: usize,
}

/// 持久化的会话配置（用于保存/恢复）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub endpoint: String,
    pub params: serde_json::Value,
    pub timestamp: u64,
}

impl SessionManager {
    /// 创建新的会话管理器
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            active_id: None,
            tab_order: Vec::new(),
            max_sessions: 10,
        }
    }

    /// 创建新会话
    ///
    /// 打开串口连接，启动 I/O 线程和 StatsCollector，返回会话 ID。
    pub fn create_session(
        &mut self,
        name: &str,
        connection_type: ConnectionType,
        endpoint: &str,
        params: serde_json::Value,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
    ) -> Result<TabId, String> {
        if self.sessions.len() >= self.max_sessions {
            return Err(format!("已达到最大会话数限制 ({})", self.max_sessions));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let tab_name = if name.is_empty() {
            format!("{} @ {}", connection_type.label(), endpoint)
        } else {
            name.to_string()
        };

        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        // 根据连接类型创建会话
        match connection_type {
            ConnectionType::Serial => {
                let (session, write_tx, io_thread, io_cancel_tx, actual_params) =
                    SerialSession::create_session(
                        &id,
                        endpoint,
                        &params,
                        on_data,
                        on_disconnect,
                    )?;

                // 从 SerialSession 提取原子计数器（在移入 SessionImpl 之前）
                let tx_bytes = session.tx_counter();
                let rx_bytes = session.rx_counter();

                // 启动 StatsCollector（每秒采集并推送至前端）
                let (stats_cancel_tx, stats_cancel_rx) = tokio::sync::oneshot::channel::<()>();
                Self::start_stats_collector(
                    app_handle.clone(),
                    id.clone(),
                    tx_bytes.clone(),
                    rx_bytes.clone(),
                    connected_at,
                    stats_cancel_rx,
                );

                let handle = SessionHandle {
                    id: id.clone(),
                    name: tab_name,
                    session: SessionImpl::Serial(session),
                    write_tx,
                    io_cancel_tx: Some(io_cancel_tx),
                    cancel_transfer_tx: None,
                    io_thread: Some(io_thread),
                    state: SessionState::Connected,
                    connection_type,
                    endpoint: endpoint.to_string(),
                    params: actual_params,
                    port_return_tx: None,
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
            ConnectionType::Ssh | ConnectionType::Telnet => {
                Err(format!("{} 连接类型尚未实现", connection_type.label()))
            }
        }
    }

    /// 关闭指定会话
    pub fn close_session(&mut self, session_id: &str) -> Result<(), String> {
        let mut handle = self.sessions.remove(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

        // 取消正在进行的传输
        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        // 取消 StatsCollector 任务
        if let Some(tx) = handle.stats_cancel_tx.take() {
            let _ = tx.send(());
        }
        // 释放端口归还通道：若 I/O 线程正在 HandoffPort 等待中，
        // drop sender 会导致 return_rx.recv_timeout 收到 Disconnected 并退出
        handle.port_return_tx = None;

        // 发送取消信号（正常模式下的 I/O 线程）
        if let Some(tx) = handle.io_cancel_tx.take() {
            let _ = tx.send(());
        }
        // 发送关闭信号
        let _ = handle.write_tx.send(IoCmd::Shutdown);
        // 等待 I/O 线程退出
        if let Some(thread) = handle.io_thread.take() {
            let _ = thread.join();
        }
        // Windows: 短暂等待 COM 端口句柄释放
        #[cfg(target_os = "windows")]
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 从 tab_order 中移除
        self.tab_order.retain(|id| id != session_id);

        // 如果关闭的是活跃标签页，切换到下一个
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
        // 验证所有 ID 都存在
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
        handle.write_tx.send(IoCmd::Write(data.to_vec()))
            .map_err(|e| format!("写入通道错误: {}", e))
    }

    /// 获取活跃会话 ID
    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    /// 获取所有标签页 ID（按 tab_order 排列）
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tab_order.clone()
    }

    /// 获取会话的数量
    #[allow(dead_code)]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 获取指定会话句柄的引用
    pub fn get_session(&self, session_id: &str) -> Option<&SessionHandle> {
        self.sessions.get(session_id)
    }

    /// 获取指定会话句柄的可变引用
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut SessionHandle> {
        self.sessions.get_mut(session_id)
    }

    /// 获取用于持久化的会话列表
    pub fn get_saved_sessions(&self) -> Vec<SavedSession> {
        self.sessions.values().map(|h| SavedSession {
            id: h.id.clone(),
            name: h.name.clone(),
            connection_type: serde_json::to_string(&h.connection_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string(),
            endpoint: h.endpoint.clone(),
            params: h.params.clone(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }).collect()
    }

    /// 重连指定会话
    ///
    /// 重新打开串口并启动 I/O 线程，恢复已断开的会话到 Connected 状态。
    /// 返回更新后的连接参数。
    pub fn reconnect_session(
        &mut self,
        session_id: &str,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
        app_handle: tauri::AppHandle,
    ) -> Result<serde_json::Value, String> {
        let handle = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;

        // 停止旧的 StatsCollector
        if let Some(tx) = handle.stats_cancel_tx.take() {
            let _ = tx.send(());
        }

        let connected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        match handle.connection_type {
            ConnectionType::Serial => {
                let (session, write_tx, io_thread, io_cancel_tx, actual_params) =
                    SerialSession::create_session(
                        session_id,
                        &handle.endpoint,
                        &handle.params,
                        on_data,
                        on_disconnect,
                    )?;

                // 提取新的原子计数器
                let tx_bytes = session.tx_counter();
                let rx_bytes = session.rx_counter();

                // 启动新的 StatsCollector
                let (stats_cancel_tx, stats_cancel_rx) = tokio::sync::oneshot::channel::<()>();
                SessionManager::start_stats_collector(
                    app_handle.clone(),
                    session_id.to_string(),
                    tx_bytes.clone(),
                    rx_bytes.clone(),
                    connected_at,
                    stats_cancel_rx,
                );

                handle.session = SessionImpl::Serial(session);
                handle.write_tx = write_tx;
                handle.io_cancel_tx = Some(io_cancel_tx);
                handle.io_thread = Some(io_thread);
                handle.state = SessionState::Connected;
                handle.params = actual_params.clone();
                handle.port_return_tx = None;
                handle.tx_bytes = tx_bytes;
                handle.rx_bytes = rx_bytes;
                handle.connected_at = connected_at;
                handle.stats_cancel_tx = Some(stats_cancel_tx);

                Ok(actual_params)
            }
            _ => Err(format!("{} 连接类型不支持重连", handle.connection_type.label())),
        }
    }

    /// 取消指定会话的传输
    pub fn cancel_transfer(&mut self, session_id: &str) -> Result<(), String> {
        let handle = self.sessions.get_mut(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    /// 获取会话的连接类型
    pub fn session_connection_type(&self, session_id: &str) -> Option<ConnectionType> {
        self.sessions.get(session_id).map(|h| h.connection_type.clone())
    }

    /// 获取会话状态
    pub fn session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).map(|h| h.state.clone())
    }

    /// 启动 I/O 统计采集器
    ///
    /// 每 1 秒读取原子计数器的值并通过 Tauri 事件 `session-stats` 推送至前端。
    /// 任务由 `cancel_rx` 控制生命周期，会话关闭时自动停止。
    fn start_stats_collector(
        app_handle: tauri::AppHandle,
        tab_id: String,
        tx_bytes: Arc<AtomicU64>,
        rx_bytes: Arc<AtomicU64>,
        connected_at: Option<u64>,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        // 使用独立 std::thread + tokio runtime，避免依赖外部 tokio context
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

    /// 标记会话状态为已断开
    pub fn mark_disconnected(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.state = SessionState::Disconnected;
            handle.io_cancel_tx = None;
            handle.io_thread = None;
            // 停止 StatsCollector
            if let Some(tx) = handle.stats_cancel_tx.take() {
                let _ = tx.send(());
            }
        }
    }

    /// 获取 Tauri 应用数据目录下的会话文件路径
    pub fn sessions_file_path(app_handle: &tauri::AppHandle) -> std::path::PathBuf {
        let mut path = app_handle.path().app_data_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        std::fs::create_dir_all(&path).ok();
        path.push("sessions.json");
        path
    }

    /// 保存会话到磁盘
    ///
    /// 合并当前内存中的会话与文件中已有的会话配置：
    /// - 内存中存在的会话按 ID 覆盖/追加
    /// - 已存在于文件中但不在内存中的条目（已断开的会话）予以保留
    /// - 若内存中无任何会话且文件已有数据，跳过写入以保护已有数据
    pub fn save_to_disk(&self, path: &std::path::Path) -> Result<(), String> {
        let current: Vec<SavedSession> = self.get_saved_sessions();

        // 加载已有持久化会话（文件不存在或损坏视为空）
        let existing = Self::load_from_disk(path).unwrap_or_default();

        // 若当前内存中无任何会话，保留已有数据不覆盖
        if current.is_empty() {
            return Ok(());
        }

        // 收集当前内存中会话的 ID，用于去重（使用 owned String 避免 borrow 冲突）
        let current_ids: HashSet<String> = current.iter().map(|s| s.id.clone()).collect();

        // 保留已有文件中不在当前内存中的条目（已断开但未删除的会话）
        let mut merged: Vec<SavedSession> = existing
            .into_iter()
            .filter(|s| !current_ids.contains(&s.id))
            .collect();

        // 追加当前内存中的会话
        merged.extend(current);

        // 按 (endpoint, connection_type) 二次去重：
        // 重连时相同端口的旧已断开记录与新活跃记录可能拥有不同 ID，
        // 须以当前内存中的条目替换文件中同端口的旧条目，避免累积重复标签页
        let mut dedup: HashMap<(String, String), SavedSession> = HashMap::new();
        for s in merged {
            let key = (s.endpoint.clone(), s.connection_type.clone());
            if current_ids.contains(&s.id) {
                // 当前内存中的条目始终覆盖
                dedup.insert(key, s);
            } else {
                // 已有文件的条目仅在 key 不存在时插入
                dedup.entry(key).or_insert(s);
            }
        }
        let merged: Vec<SavedSession> = dedup.into_values().collect();

        let json = serde_json::to_string_pretty(&merged)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("写入文件失败: {}", e))
    }

    /// 从磁盘加载会话配置
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
                // 损坏文件处理：备份并返回空列表
                let bak_path = path.with_extension("json.bak");
                let _ = std::fs::copy(path, &bak_path);
                log::warn!("会话文件损坏 ({}), 已备份到 {:?}", e, bak_path);
                Ok(Vec::new())
            }
        }
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let _ = self.close_session(&id);
        }
    }
}

/// 公共 I/O 线程启动函数
///
/// 使用缓冲通道和公平读写调度。
/// 供 SerialSession 使用，未来 SshSession/TelnetSession 也可复用。
///
/// `tx_bytes` / `rx_bytes` 原子计数器由 I/O 线程实时更新，
/// 供 `StatsCollector` 定期采集并推送至前端。
pub fn spawn_io_thread<F, D>(
    mut port: Box<dyn serialport::SerialPort>,
    session_id: String,
    mut on_data: F,
    mut on_disconnect: D,
    write_rx: mpsc::Receiver<IoCmd>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()>
where
    F: FnMut(String, Vec<u8>) + Send + 'static,
    D: FnMut(String) + Send + 'static,
{
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag = cancel_flag.clone();

    // 取消监听线程
    std::thread::spawn(move || {
        let _ = cancel_rx.blocking_recv();
        flag.store(true, Ordering::SeqCst);
    });

    let tick = Duration::from_millis(1);

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            // 1. 检查取消信号
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }

            // 2. 尝试读取（非阻塞，50ms 超时）
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    on_data(session_id.clone(), buf[..n].to_vec());
                    // 注意：不再 continue，继续处理写入
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => {
                    on_disconnect(session_id.clone());
                    break;
                }
            }

            // 3. 处理所有排队的写操作（公平调度：读写均有机会）
            loop {
                match write_rx.try_recv() {
                    Ok(IoCmd::Write(data)) => {
                        if port.write_all(&data).is_err() || port.flush().is_err() {
                            on_disconnect(session_id.clone());
                            return; // 退出线程
                        }
                        tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                        // 继续处理下一个排队写入
                    }
                    Ok(IoCmd::Shutdown) => return,
                    Ok(IoCmd::HandoffPort { give_tx, return_rx }) => {
                        // 将串口所有权移交给传输代码
                        let _ = give_tx.send(port);
                        // 阻塞等待端口归还，每 100ms 检查取消标志
                        loop {
                            if cancel_flag.load(Ordering::SeqCst) {
                                return; // 会话关闭，退出
                            }
                            match return_rx.recv_timeout(Duration::from_millis(100)) {
                                Ok(returned_port) => {
                                    port = returned_port;
                                    // 清空传输期间残留的缓冲区数据
                                    let _ = port.clear(serialport::ClearBuffer::All);
                                    break; // 端口已归还，恢复正常的读写循环
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    // 继续等待，再次检查取消标志
                                    continue;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    // return_tx 被 drop（会话关闭），I/O 线程退出
                                    return;
                                }
                            }
                        }
                        // 跳出内层写循环，回到外层主循环继续正常读写
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            // 4. 短暂休眠
            std::thread::sleep(tick);
        }
    })
}
