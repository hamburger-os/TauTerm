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

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use crate::session::{ConnectionType, SessionState, TermSession};
use crate::session::serial::SerialSession;

/// 标签页/会话唯一标识符
pub type TabId = String;

/// I/O 线程命令
#[derive(Debug)]
pub enum IoCmd {
    Write(Vec<u8>),
    Shutdown,
}

/// 单个会话句柄
pub struct SessionHandle {
    pub id: TabId,
    pub name: String,
    pub connection: Box<dyn TermSession>,
    pub write_tx: mpsc::SyncSender<IoCmd>,
    pub io_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub io_thread: Option<std::thread::JoinHandle<()>>,
    pub state: SessionState,
    pub connection_type: ConnectionType,
    pub endpoint: String,
    pub params: serde_json::Value,
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
    /// 打开串口连接，启动 I/O 线程，返回会话 ID。
    pub fn create_session(
        &mut self,
        name: &str,
        connection_type: ConnectionType,
        endpoint: &str,
        params: serde_json::Value,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
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

        // 根据连接类型创建会话
        match connection_type {
            ConnectionType::Serial => {
                let (_session, write_tx, io_thread, io_cancel_tx, actual_params) =
                    SerialSession::create_session(
                        &id,
                        endpoint,
                        &params,
                        on_data,
                        on_disconnect,
                    )?;

                let handle = SessionHandle {
                    id: id.clone(),
                    name: tab_name,
                    connection: Box::new(_session),
                    write_tx,
                    io_cancel_tx: Some(io_cancel_tx),
                    io_thread: Some(io_thread),
                    state: SessionState::Connected,
                    connection_type,
                    endpoint: endpoint.to_string(),
                    params: actual_params,
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

        // 发送取消信号
        if let Some(tx) = handle.io_cancel_tx.take() {
            let _ = tx.send(());
        }
        // 发送关闭信号
        let _ = handle.write_tx.send(IoCmd::Shutdown);
        // 等待 I/O 线程退出
        if let Some(thread) = handle.io_thread.take() {
            let _ = thread.join();
        }

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

    /// 取消指定会话的传输
    pub fn cancel_transfer(&self, session_id: &str) -> Result<(), String> {
        let handle = self.sessions.get(session_id)
            .ok_or_else(|| format!("会话 {} 不存在", session_id))?;
        // 通过 SessionHandle 的 transfer_cancel 通道（如果需要）
        // 当前由 SerialSession 内部管理
        let _ = handle;
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

    /// 标记会话状态为已断开
    pub fn mark_disconnected(&mut self, session_id: &str) {
        if let Some(handle) = self.sessions.get_mut(session_id) {
            handle.state = SessionState::Disconnected;
            handle.io_cancel_tx = None;
            handle.io_thread = None;
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
    pub fn save_to_disk(&self, path: &std::path::Path) -> Result<(), String> {
        let saved: Vec<SavedSession> = self.get_saved_sessions();
        // 只保存非活跃连接的会话参数（已连接的会话在恢复时不自动连接）
        let json = serde_json::to_string_pretty(&saved)
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
pub fn spawn_io_thread<F, D>(
    mut port: Box<dyn serialport::SerialPort>,
    session_id: String,
    mut on_data: F,
    mut on_disconnect: D,
    write_rx: mpsc::Receiver<IoCmd>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
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
                        // 继续处理下一个排队写入
                    }
                    Ok(IoCmd::Shutdown) => return,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            // 4. 短暂休眠
            std::thread::sleep(tick);
        }
    })
}
