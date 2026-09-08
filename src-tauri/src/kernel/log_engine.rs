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

use chrono::Local;
use log::{Log, Metadata, Record};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

// ── log crate 桥接器 ─────────────────────────────────

/// 全局日志发送器 — LogEngine 创建后设置，供 `LogBridge` 读取
static LOG_SENDER: Mutex<Option<mpsc::SyncSender<LogEntry>>> = Mutex::new(None);

/// 系统日志是否启用（可由前端设置页控制）
static SYSTEM_LOG_ENABLED: AtomicBool = AtomicBool::new(true);
static SESSION_LOG_ENABLED: AtomicBool = AtomicBool::new(true);
static DROPPED_SESSION_LOG_ENTRIES: AtomicU64 = AtomicU64::new(0);
static DROPPED_SYSTEM_LOG_ENTRIES: AtomicU64 = AtomicU64::new(0);

fn record_log_loss(counter: &AtomicU64, stream: &str, reason: &str) {
    let total = counter.fetch_add(1, Ordering::Relaxed) + 1;
    if total == 1 || total.is_power_of_two() {
        eprintln!(
            "TauTerm: {} log loss detected (count {}, latest: {})",
            stream, total, reason
        );
    }
}

fn record_session_log_loss(reason: &str) {
    record_log_loss(&DROPPED_SESSION_LOG_ENTRIES, "session", reason);
}

fn record_system_log_loss(reason: &str) {
    record_log_loss(&DROPPED_SYSTEM_LOG_ENTRIES, "system", reason);
}

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
        let min_level = SYSTEM_LOG_MIN_LEVEL
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
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
                if tx
                    .try_send(LogEntry::SystemEvent {
                        level: record.level().to_string(),
                        message: format!(
                            "[{}:{}] {}",
                            record.file().unwrap_or("?"),
                            record.line().unwrap_or(0),
                            record.args()
                        ),
                        timestamp: Local::now(),
                    })
                    .is_err()
                {
                    record_system_log_loss("queue or file write failure");
                }
            }
        }
    }

    fn flush(&self) {
        // 消费者线程定期 flush，无需在此处处理
    }
}

/// 更新系统日志配置（由前端设置页调用）
pub fn set_system_log_config(enabled: bool, level: &str) -> Result<(), String> {
    let mut guard = SYSTEM_LOG_MIN_LEVEL
        .lock()
        .map_err(|error| format!("system log level lock poisoned: {error}"))?;
    *guard = level.to_string();
    SYSTEM_LOG_ENABLED.store(enabled, Ordering::Relaxed);
    Ok(())
}

pub fn system_log_config_checked() -> Result<(bool, String), String> {
    let level = SYSTEM_LOG_MIN_LEVEL
        .lock()
        .map_err(|error| format!("system log level lock poisoned: {error}"))?;
    let level = if level.is_empty() {
        "info".to_string()
    } else {
        level.clone()
    };
    Ok((SYSTEM_LOG_ENABLED.load(Ordering::Relaxed), level))
}

fn try_send_session_log_when(
    sender: &mpsc::SyncSender<LogEntry>,
    entry: DataLogEntry,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    if sender.try_send(LogEntry::SessionData(entry)).is_err() {
        record_session_log_loss("queue or file write failure");
    }
}

pub fn try_send_session_log(sender: &mpsc::SyncSender<LogEntry>, entry: DataLogEntry) {
    try_send_session_log_when(sender, entry, SESSION_LOG_ENABLED.load(Ordering::Relaxed));
}

pub fn try_send_system_event(
    sender: &mpsc::SyncSender<LogEntry>,
    level: String,
    message: String,
    timestamp: chrono::DateTime<Local>,
) {
    if sender
        .try_send(LogEntry::SystemEvent {
            level,
            message,
            timestamp,
        })
        .is_err()
    {
        record_system_log_loss("queue or file write failure");
    }
}

use super::log_writer::LogWriter;
use crate::security::log_sanitizer::sanitize_log;

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
        response: mpsc::SyncSender<Result<LogStatus, String>>,
    },
    /// 停止会话日志
    StopSession { session_id: String },
    /// 全局关闭 Session Data Log 时结束所有活动 writer。
    StopAllSessions,
    /// 优雅关闭消费者线程
    Shutdown,
    /// 在消费者线程内关闭句柄、删除日志并恢复活动 Session writer。
    ClearAll {
        response: mpsc::SyncSender<Result<(), String>>,
    },
}

/// 数据日志条目（会话 TX/RX 数据）
#[derive(Debug, Clone)]
pub struct DataLogEntry {
    pub session_id: String,
    pub direction: DataDirection,
    pub data_mode: String,
    /// 会话字符编码：text 格式日志按此将 payload 解码回 UTF-8 再写入
    pub encoding: String,
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
    pub session_enabled: bool,
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
            session_enabled: true,
            log_dir: PathBuf::from("logs"),
            file_max_size: 10 * 1024 * 1024, // 10 MB
            buffer_size: 4096,               // 4 KB
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
    pub session_enabled: Option<bool>,
    pub file_max_size: Option<u64>,
    pub buffer_size: Option<usize>,
    pub flush_interval_ms: Option<u64>,
    pub retention_days: Option<u64>,
}

/// 日志配置响应（供前端查询，PathBuf 转为字符串）
#[derive(Debug, Clone, Serialize)]
pub struct LogConfigResponse {
    pub system_enabled: bool,
    pub system_level: String,
    pub session_enabled: bool,
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

#[derive(Debug, Clone, Serialize)]
pub struct LogHealth {
    pub dropped_session_entries: u64,
    pub dropped_system_entries: u64,
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
        SESSION_LOG_ENABLED.store(config.session_enabled, Ordering::Relaxed);
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
    pub fn set_log_dir(&self, dir: PathBuf) -> Result<(), String> {
        let mut cfg = self
            .config
            .lock()
            .map_err(|error| format!("log config lock poisoned: {error}"))?;
        cfg.log_dir = dir;
        Ok(())
    }

    /// 获取配置快照
    pub fn get_config(&self) -> Result<LogConfig, String> {
        self.config
            .lock()
            .map(|config| config.clone())
            .map_err(|error| format!("log config lock poisoned: {error}"))
    }

    /// 获取前端友好的配置响应（PathBuf → String）
    pub fn get_config_response(&self) -> Result<LogConfigResponse, String> {
        let cfg = self.get_config()?;
        let (system_enabled, system_level) = system_log_config_checked()?;
        Ok(LogConfigResponse {
            system_enabled,
            system_level,
            session_enabled: cfg.session_enabled,
            log_dir: cfg.log_dir.to_string_lossy().to_string(),
            file_max_size: cfg.file_max_size,
            buffer_size: cfg.buffer_size,
            flush_interval_ms: cfg.flush_interval_ms,
            retention_days: cfg.retention_days,
        })
    }

    /// 更新运行时配置（由前端设置页调用）
    ///
    /// 消费者线程每次循环自动读取最新配置，无需重启。
    pub fn update_config(&self, partial: LogConfigUpdate) -> Result<(), String> {
        let (previous, stop_all_sessions) = {
            let mut cfg = self
                .config
                .lock()
                .map_err(|error| format!("log config lock poisoned: {error}"))?;
            let previous = cfg.clone();
            let mut stop_all_sessions = false;
            if let Some(session_enabled) = partial.session_enabled {
                stop_all_sessions = cfg.session_enabled && !session_enabled;
                cfg.session_enabled = session_enabled;
                SESSION_LOG_ENABLED.store(session_enabled, Ordering::Relaxed);
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
            (previous, stop_all_sessions)
        };

        if stop_all_sessions
            && self
                .entry_tx
                .send(LogEntry::Command(LogCommand::StopAllSessions))
                .is_err()
        {
            if let Ok(mut cfg) = self.config.lock() {
                *cfg = previous.clone();
            }
            SESSION_LOG_ENABLED.store(previous.session_enabled, Ordering::Relaxed);
            return Err("log consumer is unavailable while disabling Session Data Log".to_string());
        }
        Ok(())
    }

    pub fn get_health(&self) -> LogHealth {
        LogHealth {
            dropped_session_entries: DROPPED_SESSION_LOG_ENTRIES.load(Ordering::Relaxed),
            dropped_system_entries: DROPPED_SYSTEM_LOG_ENTRIES.load(Ordering::Relaxed),
        }
    }

    /// 清理过期日志文件。
    ///
    /// 该函数在 consumer loop 正式接收消息前运行，因此不能通过 LogBridge 逐文件记录
    /// 清理结果，否则大量历史文件会把消息重新塞回尚未消费的同一有界队列。
    pub fn cleanup_old_logs(config: &LogConfig) {
        let retention_secs = config.retention_days * 86400;
        let cutoff = std::time::SystemTime::now().checked_sub(Duration::from_secs(retention_secs));
        let Some(cutoff) = cutoff else {
            return;
        };

        let entries = match std::fs::read_dir(&config.log_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                eprintln!(
                    "TauTerm: unable to inspect log retention directory {:?}: {}",
                    config.log_dir, error
                );
                return;
            }
        };

        let mut removed = 0_u64;
        let mut failed = 0_u64;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "log") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if modified >= cutoff {
                continue;
            }

            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) => {
                    failed += 1;
                    if failed == 1 || failed.is_power_of_two() {
                        eprintln!(
                            "TauTerm: log retention delete failures={} latest={:?}: {}",
                            failed, path, error
                        );
                    }
                }
            }
        }

        if removed > 0 || failed > 0 {
            eprintln!(
                "TauTerm: log retention cleanup complete (removed={}, failed={})",
                removed, failed
            );
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

        // 启动时清理过期日志。System Log 与 Session Log 启用状态彼此独立，
        // 清理策略不应被任意一个开关短路。
        Self::cleanup_old_logs(&initial_config);

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
            let current_timeout = config_arc
                .lock()
                .map(|c| get_timeout(&c))
                .unwrap_or(timeout);

            match rx.recv_timeout(current_timeout) {
                Ok(LogEntry::Command(cmd)) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    match cmd {
                        LogCommand::StartSession {
                            session_id,
                            session_name,
                            port_name,
                            data_mode,
                            response,
                        } => {
                            if !cfg.session_enabled {
                                let _ = response.send(Err(
                                    "Session Data Log is disabled in Settings".to_string(),
                                ));
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
                                        file_name,
                                        session_name,
                                        port_name
                                    );
                                    let status = LogStatus {
                                        session_id: session_id.clone(),
                                        file_name,
                                        bytes_written: 0,
                                    };
                                    if let Ok(mut map) = active_logs.lock() {
                                        map.insert(session_id.clone(), status.clone());
                                    }
                                    writers.insert(session_id.clone(), writer);
                                    if response.send(Ok(status)).is_err() {
                                        if let Some(mut writer) = writers.remove(&session_id) {
                                            if let Err(error) = writer.flush() {
                                                record_session_log_loss(&format!(
                                                    "session {} orphan-start cleanup failed: {}",
                                                    session_id, error
                                                ));
                                            }
                                        }
                                        if let Ok(mut map) = active_logs.lock() {
                                            map.remove(&session_id);
                                        }
                                    }
                                }
                                Err(e) => {
                                    let message = format!("无法创建日志文件: {}", e);
                                    let _ = response.send(Err(message.clone()));
                                    log::error!("{}", message);
                                }
                            }
                        }
                        LogCommand::StopSession { session_id } => {
                            if let Some(mut writer) = writers.remove(&session_id) {
                                let file_name = writer.file_name();
                                let bytes = writer.bytes_written();
                                if let Err(error) = writer.flush() {
                                    record_session_log_loss(&format!(
                                        "session {} final flush failed: {}",
                                        session_id, error
                                    ));
                                }
                                log::info!("日志记录已停止: {} (写入 {} 字节)", file_name, bytes);
                            }
                            if let Ok(mut map) = active_logs.lock() {
                                map.remove(&session_id);
                            }
                        }
                        LogCommand::StopAllSessions => {
                            let stopped = writers.len();
                            for (session_id, mut writer) in writers.drain() {
                                if let Err(error) = writer.flush() {
                                    record_session_log_loss(&format!(
                                        "session {} final flush failed: {}",
                                        session_id, error
                                    ));
                                }
                            }
                            if let Ok(mut map) = active_logs.lock() {
                                map.clear();
                            }
                            if stopped > 0 {
                                log::info!("全局会话日志已关闭，停止 {} 个活动日志", stopped);
                            }
                        }
                        LogCommand::Shutdown => {
                            Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                            return;
                        }
                        LogCommand::ClearAll { response } => {
                            let mut errors = Vec::new();

                            if let Some(mut writer) = system_writer.take() {
                                if let Err(error) = writer.flush() {
                                    record_system_log_loss(&format!(
                                        "system log flush before clear failed: {}",
                                        error
                                    ));
                                    errors.push(format!("system log flush failed: {}", error));
                                }
                            }
                            system_date = None;

                            for (session_id, writer) in writers.iter_mut() {
                                if let Err(error) = writer.close() {
                                    record_session_log_loss(&format!(
                                        "session {} close before clear failed: {}",
                                        session_id, error
                                    ));
                                    errors.push(format!(
                                        "session {} close failed: {}",
                                        session_id, error
                                    ));
                                }
                            }

                            match std::fs::read_dir(&cfg.log_dir) {
                                Ok(entries) => {
                                    for entry in entries.flatten() {
                                        let path = entry.path();
                                        if path.extension().is_none_or(|ext| ext != "log") {
                                            continue;
                                        }
                                        if let Err(error) = std::fs::remove_file(&path) {
                                            errors.push(format!(
                                                "delete {:?} failed: {}",
                                                path, error
                                            ));
                                        }
                                    }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                                Err(error) => errors.push(format!(
                                    "read log directory {:?} failed: {}",
                                    cfg.log_dir, error
                                )),
                            }

                            let mut failed_sessions = Vec::new();
                            for (session_id, writer) in writers.iter_mut() {
                                if let Err(error) = writer.reopen() {
                                    record_session_log_loss(&format!(
                                        "session {} reopen after clear failed: {}",
                                        session_id, error
                                    ));
                                    errors.push(format!(
                                        "session {} reopen failed: {}",
                                        session_id, error
                                    ));
                                    failed_sessions.push(session_id.clone());
                                } else if let Ok(mut map) = active_logs.lock() {
                                    if let Some(status) = map.get_mut(session_id) {
                                        status.file_name = writer.file_name();
                                        status.bytes_written = writer.bytes_written();
                                    }
                                }
                            }
                            for session_id in failed_sessions {
                                writers.remove(&session_id);
                                if let Ok(mut map) = active_logs.lock() {
                                    map.remove(&session_id);
                                }
                            }

                            if errors.is_empty() {
                                log::info!("所有日志文件已清除，活动 Session writer 已重新打开");
                                let _ = response.send(Ok(()));
                            } else {
                                let _ = response.send(Err(errors.join("; ")));
                            }
                        }
                    }
                }
                Ok(LogEntry::SessionData(entry)) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    if !cfg.session_enabled {
                        continue;
                    }
                    if let Some(writer) = writers.get_mut(&entry.session_id) {
                        if let Err(e) = writer.write_entry(&entry) {
                            record_session_log_loss(&format!(
                                "session {} write failed: {}",
                                entry.session_id, e
                            ));
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
                Ok(LogEntry::SystemEvent {
                    level,
                    message,
                    timestamp,
                }) => {
                    let cfg = config_arc.lock().map(|c| c.clone()).unwrap_or_default();
                    if !SYSTEM_LOG_ENABLED.load(Ordering::Relaxed) {
                        continue;
                    }

                    // 按日期轮转系统日志文件
                    let today = timestamp.format("%Y%m%d").to_string();
                    if system_date.as_deref() != Some(&today) {
                        // 关闭旧文件
                        if let Some(mut w) = system_writer.take() {
                            if let Err(error) = w.flush() {
                                record_system_log_loss(&format!(
                                    "system log rotation flush failed: {}",
                                    error
                                ));
                            }
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
                                record_system_log_loss(&format!(
                                    "cannot open system log file {:?}: {}",
                                    sys_path, e
                                ));
                                system_date = None;
                                continue;
                            }
                        }
                    }

                    let write_failed = if let Some(ref mut w) = system_writer {
                        let ts = timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
                        let sanitized_msg = sanitize_log(&message);
                        let line =
                            format!("[{}] [{}] {}\n", ts, level.to_uppercase(), sanitized_msg);
                        if let Err(error) = w.write_all(line.as_bytes()) {
                            record_system_log_loss(&format!("system log write failed: {}", error));
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if write_failed {
                        system_writer = None;
                        system_date = None;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // 超时：flush 所有活跃 writer 的非空缓冲区
                    for (session_id, writer) in writers.iter_mut() {
                        if let Err(error) = writer.flush() {
                            record_session_log_loss(&format!(
                                "session {} periodic flush failed: {}",
                                session_id, error
                            ));
                        }
                    }
                    if let Some(ref mut w) = system_writer {
                        if let Err(error) = w.flush() {
                            record_system_log_loss(&format!(
                                "system log periodic flush failed: {}",
                                error
                            ));
                        }
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
                record_session_log_loss(&format!(
                    "session {} final flush failed: {}",
                    session_id, e
                ));
            }
        }
        if let Some(ref mut w) = system_writer {
            if let Err(error) = w.flush() {
                record_system_log_loss(&format!("system log final flush failed: {}", error));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> DataLogEntry {
        DataLogEntry {
            session_id: "test-session".into(),
            direction: DataDirection::RX,
            data_mode: "text".into(),
            encoding: "utf-8".into(),
            payload: b"hello".to_vec(),
            timestamp: Local::now(),
        }
    }

    #[test]
    fn disabled_session_logging_does_not_fill_queue() {
        let (tx, rx) = mpsc::sync_channel(1);

        try_send_session_log_when(&tx, sample_entry(), false);
        try_send_session_log_when(&tx, sample_entry(), false);

        assert!(rx.try_recv().is_err());
    }
}
