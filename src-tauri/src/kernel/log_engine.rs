//! 日志引擎核心
//!
//! 生产者-消费者异步日志系统。
//!
//! ## 架构
//!
//! - **生产者**: IoLoop（RX 数据）、commands（TX 数据、系统事件）、前端 log_event（用户操作）
//! - **通道**: `std::sync::mpsc::SyncChannel<LogEntry>`，容量 256
//! - **消费者**: 独立 `std::thread`，管理所有 LogWriter 实例
//! - **刷新策略**: 缓冲区满 4KB 或 500ms 超时双重触发
//! - **分卷**: 单文件超过设定阈值自动创建带序号的新文件
//!
//! ## 线程安全
//!
//! - `entry_tx` 可克隆，供多生产者共享
//! - 消费者线程互斥访问 LogWriter HashMap
//! - LogWriter 的 BufWriter 在 Drop 时自动 flush + 关闭文件句柄

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use chrono::Local;
use log::{Log, Metadata, Record};
use serde::{Deserialize, Serialize};

// ── log crate 桥接器 ─────────────────────────────────

/// 全局日志发送器 — LogEngine 创建后设置，供 `LogBridge` 读取
static LOG_SENDER: Mutex<Option<mpsc::SyncSender<LogEntry>>> = Mutex::new(None);

/// 系统日志是否启用（可由前端设置页控制）
static SYSTEM_LOG_ENABLED: AtomicBool = AtomicBool::new(true);

/// 系统日志最低级别过滤。
///
/// 存储为字符串以支持运行时通过前端设置页动态切换。
/// 空字符串等价于 "info"（默认级别，忽略 Debug/Trace）。
/// 有效值：`"error"` | `"warn"` | `"info"` | `"debug"` | `"trace"` | `""`（同 info）
static SYSTEM_LOG_MIN_LEVEL: Mutex<String> = Mutex::new(String::new());

/// `log` crate 桥接器
///
/// 将所有 `log::info!()` / `log::warn!()` / `log::error!()` 调用
/// 转发到 LogEngine 消费者线程，写入 `TauTerm_{date}.log`。
///
/// 初始化时机：`lib.rs` 的 `run()` 入口处调用 `log::set_logger(&LogBridge)`，
/// LogEngine 创建时自动设置 `LOG_SENDER`。
pub struct LogBridge;

impl Log for LogBridge {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if !SYSTEM_LOG_ENABLED.load(Ordering::Relaxed) {
            return false;
        }
        // 检查级别过滤
        let min_level = SYSTEM_LOG_MIN_LEVEL.lock().map(|s| s.clone()).unwrap_or_default();
        let min = match min_level.as_str() {
            "error" => log::Level::Error,
            "warn" => log::Level::Warn,
            "info" | "" => log::Level::Info,
            "debug" => log::Level::Debug,
            "trace" => log::Level::Trace,
            _ => log::Level::Info,
        };
        metadata.level() <= min
    }

    fn log(&self, record: &Record) {
        if let Ok(guard) = LOG_SENDER.lock() {
            if let Some(ref tx) = *guard {
                let _ = tx.try_send(LogEntry::SystemEvent {
                    level: record.level().to_string(),
                    message: format!(
                        "[{}:{}] {}",
                        record.file().unwrap_or("?"),
                        record.line().unwrap_or(0),
                        record.args()
                    ),
                    timestamp: Local::now(),
                });
            }
        }
    }

    fn flush(&self) {
        // 消费者线程定期 flush，无需在此处处理
    }
}

/// 更新系统日志配置（由前端设置页调用）
pub fn set_system_log_config(enabled: bool, level: &str) {
    SYSTEM_LOG_ENABLED.store(enabled, Ordering::Relaxed);
    if let Ok(mut guard) = SYSTEM_LOG_MIN_LEVEL.lock() {
        *guard = level.to_string();
    }
}

use super::log_writer::LogWriter;

// ── 数据结构 ────────────────────────────────────────

/// 数据方向
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataDirection {
    TX,
    RX,
}

/// 日志控制命令（消费者线程内部使用）
#[derive(Debug)]
pub enum LogCommand {
    /// 启动会话日志
    StartSession {
        session_id: String,
        session_name: String,
        port_name: String,
        data_mode: String,
    },
    /// 停止会话日志
    StopSession {
        session_id: String,
    },
    /// 优雅关闭消费者线程
    Shutdown,
    /// 清除日志文件后重新打开所有写入器（系统日志 + 会话日志）
    ReopenAfterClear,
}

/// 数据日志条目（会话 TX/RX 数据）
#[derive(Debug, Clone)]
pub struct DataLogEntry {
    pub session_id: String,
    pub direction: DataDirection,
    pub data_mode: String,
    pub payload: Vec<u8>,
    pub timestamp: chrono::DateTime<Local>,
}

/// 发送到消费者线程的日志条目
#[derive(Debug)]
pub enum LogEntry {
    /// 控制命令
    Command(LogCommand),
    /// 会话数据日志
    SessionData(DataLogEntry),
    /// 系统/用户操作事件
    SystemEvent {
        level: String,
        message: String,
        timestamp: chrono::DateTime<Local>,
    },
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub enabled: bool,
    pub log_dir: PathBuf,
    /// 单文件最大大小（字节），默认 10MB
    pub file_max_size: u64,
    /// 消费者线程缓冲区大小（字节），默认 4096
    pub buffer_size: usize,
    /// 缓冲区刷新间隔（毫秒），默认 500ms
    pub flush_interval_ms: u64,
    /// 日志文件保留天数，默认 7 天
    pub retention_days: u64,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_dir: PathBuf::from("logs"),
            file_max_size: 10 * 1024 * 1024, // 10 MB
            buffer_size: 4096,                // 4 KB
            flush_interval_ms: 500,
            retention_days: 7,
        }
    }
}

/// 部分配置更新（前端设置页传入）
///
/// 所有字段均为可选，仅更新提供的值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfigUpdate {
    pub enabled: Option<bool>,
    pub file_max_size: Option<u64>,
    pub buffer_size: Option<usize>,
    pub flush_interval_ms: Option<u64>,
    pub retention_days: Option<u64>,
}

/// 日志配置响应（供前端查询，PathBuf 转为字符串）
#[derive(Debug, Clone, Serialize)]
pub struct LogConfigResponse {
    pub enabled: bool,
    pub log_dir: String,
    pub file_max_size: u64,
    pub buffer_size: usize,
    pub flush_interval_ms: u64,
    pub retention_days: u64,
}

/// 日志状态快照（供前端查询）
#[derive(Debug, Clone, Serialize)]
pub struct LogStatus {
    pub session_id: String,
    pub file_name: String,
    pub bytes_written: u64,
}

/// 日志引擎
///
/// 全局单例，管理所有日志写入器。
/// 通过 `entry_tx` (SyncSender) 接收日志条目。
pub struct LogEngine {
    /// 日志条目发送端（可克隆给生产者）
    entry_tx: mpsc::SyncSender<LogEntry>,
    /// 消费者线程句柄
    consumer_handle: Option<std::thread::JoinHandle<()>>,
    /// 消费者线程取消标志
    cancel_flag: Arc<AtomicBool>,
    /// 活跃的日志状态（用于前端查询）
    active_logs: Arc<Mutex<HashMap<String, LogStatus>>>,
    /// 当前配置（线程安全共享）
    config: Arc<Mutex<LogConfig>>,
}

impl LogEngine {
    /// 创建日志引擎并启动消费者线程
    pub fn new(config: LogConfig) -> Self {
        let (entry_tx, entry_rx) = mpsc::sync_channel::<LogEntry>(256);

        // 将 sender 注册到全局桥接器，使 log::info!/warn!/error! 自动写入系统日志
        if let Ok(mut guard) = LOG_SENDER.lock() {
            *guard = Some(entry_tx.clone());
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let active_logs = Arc::new(Mutex::new(HashMap::new()));
        let active_logs_clone = active_logs.clone();
        let config_arc = Arc::new(Mutex::new(config));
        let config_clone = config_arc.clone();

        let handle = std::thread::spawn(move || {
            Self::consumer_loop(entry_rx, cancel_flag_clone, config_clone, active_logs_clone);
        });

        LogEngine {
            entry_tx,
            consumer_handle: Some(handle),
            cancel_flag,
            active_logs,
            config: config_arc,
        }
    }

    /// 获取日志条目发送端的克隆（给生产者使用）
    pub fn sender(&self) -> mpsc::SyncSender<LogEntry> {
        self.entry_tx.clone()
    }

    /// 获取当前活跃日志状态快照
    pub fn get_active_logs(&self) -> Vec<LogStatus> {
        if let Ok(map) = self.active_logs.lock() {
            map.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// 更新日志目录（应用启动时由 setup 回调调用）
    pub fn set_log_dir(&self, dir: PathBuf) {
        if let Ok(mut cfg) = self.config.lock() {
            cfg.log_dir = dir;
        }
    }

    /// 获取配置快照
    pub fn get_config(&self) -> LogConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// 获取前端友好的配置响应（PathBuf → String）
    pub fn get_config_response(&self) -> LogConfigResponse {
        let cfg = self.get_config();
        LogConfigResponse {
            enabled: cfg.enabled,
            log_dir: cfg.log_dir.to_string_lossy().to_string(),
            file_max_size: cfg.file_max_size,
            buffer_size: cfg.buffer_size,
            flush_interval_ms: cfg.flush_interval_ms,
            retention_days: cfg.retention_days,
        }
    }

    /// 更新运行时配置（由前端设置页调用）
    ///
    /// 消费者线程每次循环自动读取最新配置，无需重启。
    pub fn update_config(&self, partial: LogConfigUpdate) {
        if let Ok(mut cfg) = self.config.lock() {
            if let Some(enabled) = partial.enabled {
                cfg.enabled = enabled;
            }
            if let Some(file_max_size) = partial.file_max_size {
                cfg.file_max_size = file_max_size;
            }
            if let Some(buffer_size) = partial.buffer_size {
                cfg.buffer_size = buffer_size;
            }
            if let Some(flush_interval_ms) = partial.flush_interval_ms {
                cfg.flush_interval_ms = flush_interval_ms;
            }
            if let Some(retention_days) = partial.retention_days {
                cfg.retention_days = retention_days;
            }
        }
    }

    /// 清理过期日志文件
    pub fn cleanup_old_logs(config: &LogConfig) {
        let retention_secs = config.retention_days * 86400;
        let cutoff = std::time::SystemTime::now()
            .checked_sub(Duration::from_secs(retention_secs));

        if let Some(cutoff) = cutoff {
            if let Ok(entries) = std::fs::read_dir(&config.log_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "log") {
                        continue;
                    }
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if modified < cutoff {
                                let _ = std::fs::remove_file(&path);
                                log::info!("已删除过期日志: {:?}", path);
                            }
                        }
                    }
                }
            }
        }
    }

    // ── 消费者线程 ──

    fn consumer_loop(
        rx: mpsc::Receiver<LogEntry>,
        cancel_flag: Arc<AtomicBool>,
        config_arc: Arc<Mutex<LogConfig>>,
        active_logs: Arc<Mutex<HashMap<String, LogStatus>>>,
    ) {
        let initial_config = config_arc.lock().map(|c| c.clone()).unwrap_or_default();

        // 启动时清理过期日志
        if initial_config.enabled {
            Self::cleanup_old_logs(&initial_config);
        }

        let mut writers: HashMap<String, LogWriter> = HashMap::new();
        // 系统日志独立写入（使用简单的 BufWriter<File>）
        let mut system_writer: Option<std::io::BufWriter<std::fs::File>> = None;
        let mut system_date: Option<String> = None;
        // active_logs 状态更新计数器：每 ~10 条会话数据才更新一次，减少锁竞争
        let mut status_update_counter: u32 = 0;

        // 从配置获取超时
        let get_timeout = |cfg: &LogConfig| Duration::from_millis(cfg.flush_interval_ms);
        let timeout = get_timeout(&initial_config);

        loop {
            // 检查取消信号
            if cancel_flag.load(Ordering::SeqCst) {
                Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                break;
            }

            // 动态读取配置获取最新超时
            let current_timeout = config_arc.lock()
                .map(|c| get_timeout(&c))
                .unwrap_or(timeout);

            match rx.recv_timeout(current_timeout) {
                Ok(LogEntry::Command(cmd)) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    match cmd {
                        LogCommand::StartSession { session_id, session_name, port_name, data_mode } => {
                            if !cfg.enabled {
                                continue;
                            }
                            match LogWriter::new(
                                &cfg.log_dir,
                                cfg.file_max_size,
                                cfg.buffer_size,
                                &session_name,
                                &port_name,
                                &data_mode,
                            ) {
                                Ok(writer) => {
                                    let file_name = writer.file_name();
                                    log::info!(
                                        "日志记录已启动: {} (会话: {}, 端口: {})",
                                        file_name, session_name, port_name
                                    );
                                    if let Ok(mut map) = active_logs.lock() {
                                        map.insert(session_id.clone(), LogStatus {
                                            session_id: session_id.clone(),
                                            file_name,
                                            bytes_written: 0,
                                        });
                                    }
                                    writers.insert(session_id, writer);
                                }
                                Err(e) => {
                                    log::error!("无法创建日志文件: {}", e);
                                }
                            }
                        }
                        LogCommand::StopSession { session_id } => {
                            if let Some(mut writer) = writers.remove(&session_id) {
                                let file_name = writer.file_name();
                                let bytes = writer.bytes_written();
                                let _ = writer.flush();
                                log::info!(
                                    "日志记录已停止: {} (写入 {} 字节)",
                                    file_name, bytes
                                );
                            }
                            if let Ok(mut map) = active_logs.lock() {
                                map.remove(&session_id);
                            }
                        }
                        LogCommand::Shutdown => {
                            Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                            return;
                        }
                        LogCommand::ReopenAfterClear => {
                            // 关闭系统日志句柄，下次 SystemEvent 自动按日期重建
                            if let Some(mut w) = system_writer.take() {
                                let _ = w.flush();
                            }
                            system_date = None;
                            // 每个会话日志 writer 分卷到新文件
                            for (sid, writer) in writers.iter_mut() {
                                if let Err(e) = writer.reopen() {
                                    log::error!("日志重新打开失败 (会话 {}): {}", sid, e);
                                }
                            }
                            log::info!("日志文件已清除，所有写入器已重新打开");
                        }
                    }
                }
                Ok(LogEntry::SessionData(entry)) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    if !cfg.enabled {
                        continue;
                    }
                    if let Some(writer) = writers.get_mut(&entry.session_id) {
                        if let Err(e) = writer.write_entry(&entry) {
                            log::error!("日志写入失败 (会话 {}): {}", entry.session_id, e);
                        }
                        // 更新活跃日志状态（每 ~10 条更新一次，减少锁竞争）
                        status_update_counter += 1;
                        if status_update_counter.is_multiple_of(10) {
                            if let Ok(mut map) = active_logs.lock() {
                                if let Some(status) = map.get_mut(&entry.session_id) {
                                    status.bytes_written = writer.bytes_written();
                                }
                            }
                        }
                    }
                }
                Ok(LogEntry::SystemEvent { level, message, timestamp }) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    if !cfg.enabled {
                        continue;
                    }

                    // 按日期轮转系统日志文件
                    let today = timestamp.format("%Y%m%d").to_string();
                    if system_date.as_deref() != Some(&today) {
                        // 关闭旧文件
                        if let Some(mut w) = system_writer.take() {
                            let _ = w.flush();
                        }
                        // 打开新文件
                        let sys_filename = format!("TauTerm_{}.log", today);
                        let sys_path = cfg.log_dir.join(&sys_filename);
                        let _ = std::fs::create_dir_all(&cfg.log_dir);
                        match std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&sys_path)
                        {
                            Ok(file) => {
                                system_writer = Some(std::io::BufWriter::with_capacity(8192, file));
                                system_date = Some(today);
                            }
                            Err(e) => {
                                log::error!("无法打开系统日志文件 {:?}: {}", sys_path, e);
                                system_date = None;
                                continue;
                            }
                        }
                    }

                    if let Some(ref mut w) = system_writer {
                        let ts = timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
                        let line = format!("[{}] [{}] {}\n", ts, level.to_uppercase(), message);
                        let _ = w.write_all(line.as_bytes());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // 超时：flush 所有活跃 writer 的非空缓冲区
                    for writer in writers.values_mut() {
                        let _ = writer.flush();
                    }
                    if let Some(ref mut w) = system_writer {
                        let _ = w.flush();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                    break;
                }
            }
        }
    }

    /// 刷新并清理所有 writer
    fn flush_all(
        writers: &mut HashMap<String, LogWriter>,
        active_logs: &Arc<Mutex<HashMap<String, LogStatus>>>,
        system_writer: &mut Option<std::io::BufWriter<std::fs::File>>,
    ) {
        for (session_id, writer) in writers.iter_mut() {
            if let Err(e) = writer.flush() {
                log::error!("日志最终 flush 失败 (会话 {}): {}", session_id, e);
            }
        }
        if let Some(ref mut w) = system_writer {
            let _ = w.flush();
        }
        if let Ok(mut map) = active_logs.lock() {
            map.clear();
        }
    }
}

impl Drop for LogEngine {
    fn drop(&mut self) {
        // 设置取消标志
        self.cancel_flag.store(true, Ordering::SeqCst);
        // 发送关闭信号（如果通道还开着）
        let _ = self.entry_tx.send(LogEntry::Command(LogCommand::Shutdown));
        // 等待消费者线程结束
        if let Some(handle) = self.consumer_handle.take() {
            let _ = handle.join();
        }
    }
}
