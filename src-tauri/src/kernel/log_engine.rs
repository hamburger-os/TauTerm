//! TauTerm logging runtime.
//!
//! The logger is deliberately best-effort and is not an evidence recorder. Producers never block
//! transport I/O. A bounded ingress queue protects the application, while soft reservations keep
//! capacity available for system/control traffic under sustained Session Data Log load.
//!
//! Storage uses append-only segments. Active segments are never rewritten to satisfy retention or
//! size limits; rotation closes one segment and creates the next. Retention maintenance runs at
//! startup and periodically while the process remains alive.

use chrono::{Local, SecondsFormat};
use log::{Log, Metadata, Record};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use super::log_writer::LogWriter;
use crate::security::log_sanitizer::sanitize_log;

const LOG_QUEUE_CAPACITY: usize = 256;
const SESSION_QUEUE_SOFT_LIMIT: usize = 160;
const SYSTEM_QUEUE_SOFT_LIMIT: usize = 64;
const STORAGE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60);
const SYSTEM_RETRY_BACKOFF: Duration = Duration::from_secs(5);

const MIN_FILE_SIZE: u64 = 1024 * 1024;
const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
const MIN_BUFFER_SIZE: usize = 1024;
const MAX_BUFFER_SIZE: usize = 64 * 1024;
const MIN_FLUSH_INTERVAL_MS: u64 = 100;
const MAX_FLUSH_INTERVAL_MS: u64 = 2_000;
const MIN_RETENTION_DAYS: u64 = 1;
const MAX_RETENTION_DAYS: u64 = 365;

static LOG_SENDER: Mutex<Option<mpsc::SyncSender<LogEntry>>> = Mutex::new(None);
static SYSTEM_LOG_ENABLED: AtomicBool = AtomicBool::new(true);
static SESSION_LOG_ENABLED: AtomicBool = AtomicBool::new(true);
static SYSTEM_LOG_MIN_LEVEL: Mutex<String> = Mutex::new(String::new());
static ACTIVE_SESSION_LOGS: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

static SESSION_PENDING: AtomicUsize = AtomicUsize::new(0);
static SYSTEM_PENDING: AtomicUsize = AtomicUsize::new(0);
static DROPPED_SESSION_LOG_ENTRIES: AtomicU64 = AtomicU64::new(0);
static DROPPED_SYSTEM_LOG_ENTRIES: AtomicU64 = AtomicU64::new(0);
static SESSION_QUEUE_DROPS: AtomicU64 = AtomicU64::new(0);
static SYSTEM_QUEUE_DROPS: AtomicU64 = AtomicU64::new(0);
static SESSION_WRITE_FAILURES: AtomicU64 = AtomicU64::new(0);
static SYSTEM_WRITE_FAILURES: AtomicU64 = AtomicU64::new(0);
static MAINTENANCE_FAILURES: AtomicU64 = AtomicU64::new(0);

fn record_log_loss(counter: &AtomicU64, stream: &str, reason: &str) {
    let total = counter.fetch_add(1, Ordering::Relaxed) + 1;
    if total == 1 || total.is_power_of_two() {
        eprintln!(
            "TauTerm: {stream} log loss detected (count {total}, latest: {reason})"
        );
    }
}

fn record_session_queue_loss(reason: &str) {
    SESSION_QUEUE_DROPS.fetch_add(1, Ordering::Relaxed);
    record_log_loss(&DROPPED_SESSION_LOG_ENTRIES, "session", reason);
}

fn record_system_queue_loss(reason: &str) {
    SYSTEM_QUEUE_DROPS.fetch_add(1, Ordering::Relaxed);
    record_log_loss(&DROPPED_SYSTEM_LOG_ENTRIES, "system", reason);
}

fn record_session_write_loss(reason: &str) {
    SESSION_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    record_log_loss(&DROPPED_SESSION_LOG_ENTRIES, "session", reason);
}

fn record_system_write_loss(reason: &str) {
    SYSTEM_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    record_log_loss(&DROPPED_SYSTEM_LOG_ENTRIES, "system", reason);
}

fn record_maintenance_failure(reason: &str) {
    let total = MAINTENANCE_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
    if total == 1 || total.is_power_of_two() {
        eprintln!(
            "TauTerm: log storage maintenance failure (count {total}, latest: {reason})"
        );
    }
}

fn try_reserve(counter: &AtomicUsize, limit: usize) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        if current >= limit {
            return false;
        }
        match counter.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(next) => current = next,
        }
    }
}

fn release_reservation(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
        Some(current.saturating_sub(1))
    });
}

fn parse_level(value: &str) -> Option<log::Level> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(log::Level::Error),
        "warn" | "warning" => Some(log::Level::Warn),
        "info" => Some(log::Level::Info),
        "debug" => Some(log::Level::Debug),
        "trace" => Some(log::Level::Trace),
        _ => None,
    }
}

fn configured_system_level() -> Result<log::Level, String> {
    let level = SYSTEM_LOG_MIN_LEVEL
        .lock()
        .map_err(|error| format!("system log level lock poisoned: {error}"))?;
    Ok(parse_level(if level.is_empty() { "info" } else { &level }).unwrap_or(log::Level::Info))
}

fn system_level_allows(level: log::Level) -> bool {
    configured_system_level()
        .map(|minimum| level <= minimum)
        .unwrap_or(false)
}

pub struct LogBridge;

impl Log for LogBridge {
    fn enabled(&self, metadata: &Metadata) -> bool {
        SYSTEM_LOG_ENABLED.load(Ordering::Relaxed) && system_level_allows(metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(sender) = LOG_SENDER.lock() {
            if let Some(sender) = sender.as_ref() {
                try_send_system_event(
                    sender,
                    record.level().to_string(),
                    format!(
                        "[{}:{}] {}",
                        record.file().unwrap_or("?"),
                        record.line().unwrap_or(0),
                        record.args()
                    ),
                    Local::now(),
                );
            }
        }
    }

    fn flush(&self) {}
}

pub fn set_system_log_config(enabled: bool, level: &str) -> Result<(), String> {
    let normalized = level.trim().to_ascii_lowercase();
    if parse_level(&normalized).is_none() {
        return Err(format!("unsupported system log level: {level}"));
    }
    let mut guard = SYSTEM_LOG_MIN_LEVEL
        .lock()
        .map_err(|error| format!("system log level lock poisoned: {error}"))?;
    *guard = normalized;
    SYSTEM_LOG_ENABLED.store(enabled, Ordering::Relaxed);

    // Runtime filtering belongs to LogBridge. Keeping the global facade ceiling at Trace makes the
    // Settings Debug/Trace levels real instead of losing those records before our logger sees them.
    log::set_max_level(log::LevelFilter::Trace);
    Ok(())
}

pub fn system_log_config_checked() -> Result<(bool, String), String> {
    let level = SYSTEM_LOG_MIN_LEVEL
        .lock()
        .map_err(|error| format!("system log level lock poisoned: {error}"))?;
    Ok((
        SYSTEM_LOG_ENABLED.load(Ordering::Relaxed),
        if level.is_empty() {
            "info".to_string()
        } else {
            level.clone()
        },
    ))
}

pub fn session_log_is_active(session_id: &str) -> bool {
    SESSION_LOG_ENABLED.load(Ordering::Relaxed)
        && ACTIVE_SESSION_LOGS
            .read()
            .map(|active| active.contains(session_id))
            .unwrap_or(false)
}

fn set_session_active(session_id: &str, active: bool) {
    if let Ok(mut sessions) = ACTIVE_SESSION_LOGS.write() {
        if active {
            sessions.insert(session_id.to_string());
        } else {
            sessions.remove(session_id);
        }
    }
}

fn clear_active_sessions() {
    if let Ok(mut sessions) = ACTIVE_SESSION_LOGS.write() {
        sessions.clear();
    }
}

pub fn try_send_session_log(sender: &mpsc::SyncSender<LogEntry>, entry: DataLogEntry) {
    if !session_log_is_active(&entry.session_id) {
        return;
    }
    if !try_reserve(&SESSION_PENDING, SESSION_QUEUE_SOFT_LIMIT) {
        record_session_queue_loss("session queue soft limit reached");
        return;
    }
    if sender.try_send(LogEntry::SessionData(entry)).is_err() {
        release_reservation(&SESSION_PENDING);
        record_session_queue_loss("shared logging queue unavailable");
    }
}

pub fn try_send_system_event(
    sender: &mpsc::SyncSender<LogEntry>,
    level: String,
    message: String,
    timestamp: chrono::DateTime<Local>,
) {
    if !SYSTEM_LOG_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(parsed_level) = parse_level(&level) else {
        record_system_queue_loss("invalid system event level");
        return;
    };
    if !system_level_allows(parsed_level) {
        return;
    }
    if !try_reserve(&SYSTEM_PENDING, SYSTEM_QUEUE_SOFT_LIMIT) {
        record_system_queue_loss("system queue soft limit reached");
        return;
    }
    if sender
        .try_send(LogEntry::SystemEvent {
            level: parsed_level.to_string(),
            message,
            timestamp,
        })
        .is_err()
    {
        release_reservation(&SYSTEM_PENDING);
        record_system_queue_loss("shared logging queue unavailable");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataDirection {
    TX,
    RX,
}

#[derive(Debug)]
pub enum LogCommand {
    StartSession {
        session_id: String,
        session_name: String,
        port_name: String,
        data_mode: String,
        response: mpsc::SyncSender<Result<LogStatus, String>>,
    },
    StopSession {
        session_id: String,
    },
    StopAllSessions,
    Shutdown,
    ClearAll {
        response: mpsc::SyncSender<Result<(), String>>,
    },
}

#[derive(Debug, Clone)]
pub struct DataLogEntry {
    pub session_id: String,
    pub direction: DataDirection,
    pub data_mode: String,
    pub encoding: String,
    pub payload: Vec<u8>,
    pub timestamp: chrono::DateTime<Local>,
}

#[derive(Debug)]
pub enum LogEntry {
    Command(LogCommand),
    SessionData(DataLogEntry),
    SystemEvent {
        level: String,
        message: String,
        timestamp: chrono::DateTime<Local>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub session_enabled: bool,
    pub log_dir: PathBuf,
    pub file_max_size: u64,
    pub buffer_size: usize,
    pub flush_interval_ms: u64,
    pub retention_days: u64,
}

impl LogConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_FILE_SIZE..=MAX_FILE_SIZE).contains(&self.file_max_size) {
            return Err(format!(
                "file_max_size must be between {MIN_FILE_SIZE} and {MAX_FILE_SIZE} bytes"
            ));
        }
        if !(MIN_BUFFER_SIZE..=MAX_BUFFER_SIZE).contains(&self.buffer_size) {
            return Err(format!(
                "buffer_size must be between {MIN_BUFFER_SIZE} and {MAX_BUFFER_SIZE} bytes"
            ));
        }
        if !(MIN_FLUSH_INTERVAL_MS..=MAX_FLUSH_INTERVAL_MS).contains(&self.flush_interval_ms) {
            return Err(format!(
                "flush_interval_ms must be between {MIN_FLUSH_INTERVAL_MS} and {MAX_FLUSH_INTERVAL_MS}"
            ));
        }
        if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&self.retention_days) {
            return Err(format!(
                "retention_days must be between {MIN_RETENTION_DAYS} and {MAX_RETENTION_DAYS}"
            ));
        }
        Ok(())
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            session_enabled: true,
            log_dir: PathBuf::new(),
            file_max_size: 10 * 1024 * 1024,
            buffer_size: 4096,
            flush_interval_ms: 500,
            retention_days: 7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfigUpdate {
    pub session_enabled: Option<bool>,
    pub file_max_size: Option<u64>,
    pub buffer_size: Option<usize>,
    pub flush_interval_ms: Option<u64>,
    pub retention_days: Option<u64>,
}

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
    pub storage_max_size: u64,
}

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
    pub session_queue_drops: u64,
    pub system_queue_drops: u64,
    pub session_write_failures: u64,
    pub system_write_failures: u64,
    pub maintenance_failures: u64,
    pub session_queue_entries: usize,
    pub system_queue_entries: usize,
    pub active_session_logs: usize,
    pub storage_max_size: u64,
}

pub struct LogEngine {
    entry_tx: mpsc::SyncSender<LogEntry>,
    pending_rx: Mutex<Option<mpsc::Receiver<LogEntry>>>,
    consumer_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    active_logs: Arc<Mutex<HashMap<String, LogStatus>>>,
    config: Arc<Mutex<LogConfig>>,
}

impl LogEngine {
    pub fn new(config: LogConfig) -> Self {
        debug_assert!(config.validate().is_ok());
        SESSION_LOG_ENABLED.store(config.session_enabled, Ordering::Relaxed);
        clear_active_sessions();
        SESSION_PENDING.store(0, Ordering::Relaxed);
        SYSTEM_PENDING.store(0, Ordering::Relaxed);
        log::set_max_level(log::LevelFilter::Trace);

        let (entry_tx, entry_rx) = mpsc::sync_channel::<LogEntry>(LOG_QUEUE_CAPACITY);
        if let Ok(mut sender) = LOG_SENDER.lock() {
            *sender = Some(entry_tx.clone());
        }

        Self {
            entry_tx,
            pending_rx: Mutex::new(Some(entry_rx)),
            consumer_handle: Mutex::new(None),
            active_logs: Arc::new(Mutex::new(HashMap::new())),
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub fn sender(&self) -> mpsc::SyncSender<LogEntry> {
        self.entry_tx.clone()
    }

    pub fn get_active_logs(&self) -> Vec<LogStatus> {
        self.active_logs
            .lock()
            .map(|logs| logs.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn set_log_dir(&self, dir: PathBuf) -> Result<(), String> {
        if !dir.is_absolute() {
            return Err(format!("日志目录必须是绝对路径: {dir:?}"));
        }
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("无法创建日志目录 {dir:?}: {error}"))?;

        let mut pending_rx = self
            .pending_rx
            .lock()
            .map_err(|error| format!("log startup receiver lock poisoned: {error}"))?;
        if pending_rx.is_none() {
            return Err("日志目录已经配置，运行期间不允许重新绑定日志目录".to_string());
        }
        {
            let mut config = self
                .config
                .lock()
                .map_err(|error| format!("log config lock poisoned: {error}"))?;
            config.log_dir = dir;
            config.validate()?;
        }

        let receiver = pending_rx
            .take()
            .expect("pending receiver checked before take");
        let config = self.config.clone();
        let active_logs = self.active_logs.clone();
        let handle = std::thread::Builder::new()
            .name("tauterm-log-consumer".to_string())
            .spawn(move || Self::consumer_loop(receiver, config, active_logs))
            .map_err(|error| format!("启动日志消费者线程失败: {error}"))?;
        self.consumer_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .replace(handle);
        Ok(())
    }

    pub fn get_config(&self) -> Result<LogConfig, String> {
        self.config
            .lock()
            .map(|config| config.clone())
            .map_err(|error| format!("log config lock poisoned: {error}"))
    }

    pub fn get_config_response(&self) -> Result<LogConfigResponse, String> {
        let config = self.get_config()?;
        let (system_enabled, system_level) = system_log_config_checked()?;
        Ok(LogConfigResponse {
            system_enabled,
            system_level,
            session_enabled: config.session_enabled,
            log_dir: config.log_dir.to_string_lossy().into_owned(),
            file_max_size: config.file_max_size,
            buffer_size: config.buffer_size,
            flush_interval_ms: config.flush_interval_ms,
            retention_days: config.retention_days,
            storage_max_size: STORAGE_MAX_BYTES,
        })
    }

    pub fn update_config(&self, partial: LogConfigUpdate) -> Result<(), String> {
        let (previous, next, stop_all_sessions) = {
            let config = self
                .config
                .lock()
                .map_err(|error| format!("log config lock poisoned: {error}"))?;
            let previous = config.clone();
            let mut next = previous.clone();
            if let Some(value) = partial.session_enabled {
                next.session_enabled = value;
            }
            if let Some(value) = partial.file_max_size {
                next.file_max_size = value;
            }
            if let Some(value) = partial.buffer_size {
                next.buffer_size = value;
            }
            if let Some(value) = partial.flush_interval_ms {
                next.flush_interval_ms = value;
            }
            if let Some(value) = partial.retention_days {
                next.retention_days = value;
            }
            next.validate()?;
            let stop = previous.session_enabled && !next.session_enabled;
            (previous, next, stop)
        };

        if stop_all_sessions {
            SESSION_LOG_ENABLED.store(false, Ordering::Relaxed);
            if self
                .entry_tx
                .try_send(LogEntry::Command(LogCommand::StopAllSessions))
                .is_err()
            {
                SESSION_LOG_ENABLED.store(previous.session_enabled, Ordering::Relaxed);
                return Err("log consumer is unavailable while disabling Session Data Log".into());
            }
        }

        {
            let mut config = self
                .config
                .lock()
                .map_err(|error| format!("log config lock poisoned: {error}"))?;
            *config = next.clone();
        }
        SESSION_LOG_ENABLED.store(next.session_enabled, Ordering::Relaxed);
        Ok(())
    }

    pub fn get_health(&self) -> LogHealth {
        LogHealth {
            dropped_session_entries: DROPPED_SESSION_LOG_ENTRIES.load(Ordering::Relaxed),
            dropped_system_entries: DROPPED_SYSTEM_LOG_ENTRIES.load(Ordering::Relaxed),
            session_queue_drops: SESSION_QUEUE_DROPS.load(Ordering::Relaxed),
            system_queue_drops: SYSTEM_QUEUE_DROPS.load(Ordering::Relaxed),
            session_write_failures: SESSION_WRITE_FAILURES.load(Ordering::Relaxed),
            system_write_failures: SYSTEM_WRITE_FAILURES.load(Ordering::Relaxed),
            maintenance_failures: MAINTENANCE_FAILURES.load(Ordering::Relaxed),
            session_queue_entries: SESSION_PENDING.load(Ordering::Relaxed),
            system_queue_entries: SYSTEM_PENDING.load(Ordering::Relaxed),
            active_session_logs: ACTIVE_SESSION_LOGS
                .read()
                .map(|active| active.len())
                .unwrap_or_default(),
            storage_max_size: STORAGE_MAX_BYTES,
        }
    }

    pub fn cleanup_old_logs(config: &LogConfig) {
        if let Err(error) = Self::maintain_storage(config, &HashSet::new(), STORAGE_MAX_BYTES) {
            record_maintenance_failure(&error);
        }
    }

    fn consumer_loop(
        rx: mpsc::Receiver<LogEntry>,
        config: Arc<Mutex<LogConfig>>,
        active_logs: Arc<Mutex<HashMap<String, LogStatus>>>,
    ) {
        let read_config = || {
            config
                .lock()
                .map(|config| config.clone())
                .map_err(|error| format!("log config lock poisoned: {error}"))
        };
        let initial_config = match read_config() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("TauTerm: LogEngine consumer cannot start: {error}");
                return;
            }
        };
        debug_assert!(initial_config.log_dir.is_absolute());
        Self::cleanup_old_logs(&initial_config);

        let mut writers: HashMap<String, LogWriter> = HashMap::new();
        let mut system_writer: Option<SystemWriter> = None;
        let mut system_retry_after: Option<Instant> = None;
        let mut status_update_counter = 0_u32;
        let mut flush_interval = Duration::from_millis(initial_config.flush_interval_ms);
        let mut next_flush = Instant::now() + flush_interval;
        let mut next_maintenance = Instant::now() + MAINTENANCE_INTERVAL;

        loop {
            let current_config = match read_config() {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("TauTerm: LogEngine consumer stopped: {error}");
                    Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                    break;
                }
            };
            let configured_interval = Duration::from_millis(current_config.flush_interval_ms);
            if configured_interval != flush_interval {
                flush_interval = configured_interval;
                next_flush = Instant::now() + flush_interval;
            }

            let now = Instant::now();
            let deadline = std::cmp::min(next_flush, next_maintenance);
            let wait = deadline.saturating_duration_since(now);
            let received = rx.recv_timeout(wait);

            match received {
                Ok(LogEntry::Command(command)) => {
                    if Self::handle_command(
                        command,
                        &current_config,
                        &mut writers,
                        &active_logs,
                        &mut system_writer,
                    ) {
                        return;
                    }
                }
                Ok(LogEntry::SessionData(entry)) => {
                    release_reservation(&SESSION_PENDING);
                    if !current_config.session_enabled {
                        continue;
                    }
                    let mut failed = false;
                    let mut rotated = false;
                    if let Some(writer) = writers.get_mut(&entry.session_id) {
                        let before = writer.file_name();
                        match writer.reconfigure(
                            current_config.file_max_size,
                            current_config.buffer_size,
                        ) {
                            Ok(changed) => rotated |= changed,
                            Err(error) => {
                                record_session_write_loss(&format!(
                                    "session {} reconfigure failed: {error}",
                                    entry.session_id
                                ));
                                failed = true;
                            }
                        }
                        if !failed {
                            if let Err(error) = writer.write_entry(&entry) {
                                record_session_write_loss(&format!(
                                    "session {} write failed: {error}",
                                    entry.session_id
                                ));
                                failed = true;
                            }
                        }
                        rotated |= before != writer.file_name();
                        status_update_counter = status_update_counter.wrapping_add(1);
                        if !failed
                            && (rotated || status_update_counter.is_multiple_of(10))
                        {
                            if let Ok(mut logs) = active_logs.lock() {
                                if let Some(status) = logs.get_mut(&entry.session_id) {
                                    status.file_name = writer.file_name();
                                    status.bytes_written = writer.bytes_written();
                                }
                            }
                        }
                    }
                    if failed {
                        writers.remove(&entry.session_id);
                        set_session_active(&entry.session_id, false);
                        if let Ok(mut logs) = active_logs.lock() {
                            logs.remove(&entry.session_id);
                        }
                    }
                }
                Ok(LogEntry::SystemEvent {
                    level,
                    message,
                    timestamp,
                }) => {
                    release_reservation(&SYSTEM_PENDING);
                    if !SYSTEM_LOG_ENABLED.load(Ordering::Relaxed) {
                        continue;
                    }
                    if system_retry_after.is_some_and(|retry| Instant::now() < retry) {
                        record_system_write_loss("system sink is in retry backoff");
                        continue;
                    }
                    if system_writer.is_none() {
                        match SystemWriter::new(&current_config, &timestamp) {
                            Ok(writer) => {
                                system_writer = Some(writer);
                                system_retry_after = None;
                            }
                            Err(error) => {
                                record_system_write_loss(&format!(
                                    "cannot open system log segment: {error}"
                                ));
                                system_retry_after = Some(Instant::now() + SYSTEM_RETRY_BACKOFF);
                                continue;
                            }
                        }
                    }
                    let result = system_writer.as_mut().map(|writer| {
                        writer.write_event(&current_config, &level, &message, &timestamp)
                    });
                    if let Some(Err(error)) = result {
                        record_system_write_loss(&format!("system log write failed: {error}"));
                        if let Some(writer) = system_writer.as_mut() {
                            let _ = writer.close();
                        }
                        system_writer = None;
                        system_retry_after = Some(Instant::now() + SYSTEM_RETRY_BACKOFF);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    Self::flush_all(&mut writers, &active_logs, &mut system_writer);
                    break;
                }
            }

            let now = Instant::now();
            if now >= next_flush {
                Self::flush_all_open(&mut writers, &mut system_writer);
                next_flush = now + flush_interval;
            }
            if now >= next_maintenance {
                let protected = Self::protected_paths(&writers, system_writer.as_ref());
                if let Err(error) =
                    Self::maintain_storage(&current_config, &protected, STORAGE_MAX_BYTES)
                {
                    record_maintenance_failure(&error);
                }
                next_maintenance = now + MAINTENANCE_INTERVAL;
            }
        }
    }

    fn handle_command(
        command: LogCommand,
        config: &LogConfig,
        writers: &mut HashMap<String, LogWriter>,
        active_logs: &Arc<Mutex<HashMap<String, LogStatus>>>,
        system_writer: &mut Option<SystemWriter>,
    ) -> bool {
        match command {
            LogCommand::StartSession {
                session_id,
                session_name,
                port_name,
                data_mode,
                response,
            } => {
                if !config.session_enabled {
                    let _ = response.send(Err("Session Data Log is disabled in Settings".into()));
                    return false;
                }
                if writers.contains_key(&session_id) {
                    let _ = response.send(Err("Session Data Log is already active".into()));
                    return false;
                }
                match LogWriter::new(
                    &config.log_dir,
                    config.file_max_size,
                    config.buffer_size,
                    &session_id,
                    &session_name,
                    &port_name,
                    &data_mode,
                ) {
                    Ok(writer) => {
                        let status = LogStatus {
                            session_id: session_id.clone(),
                            file_name: writer.file_name(),
                            bytes_written: writer.bytes_written(),
                        };
                        writers.insert(session_id.clone(), writer);
                        set_session_active(&session_id, true);
                        if let Ok(mut logs) = active_logs.lock() {
                            logs.insert(session_id.clone(), status.clone());
                        }
                        if response.send(Ok(status)).is_err() {
                            writers.remove(&session_id);
                            set_session_active(&session_id, false);
                            if let Ok(mut logs) = active_logs.lock() {
                                logs.remove(&session_id);
                            }
                        } else {
                            log::info!("Session Data Log started: {session_id}");
                        }
                    }
                    Err(error) => {
                        let message = format!("无法创建日志文件: {error}");
                        let _ = response.send(Err(message.clone()));
                        log::error!("{message}");
                    }
                }
            }
            LogCommand::StopSession { session_id } => {
                set_session_active(&session_id, false);
                if let Some(mut writer) = writers.remove(&session_id) {
                    if let Err(error) = writer.flush() {
                        record_session_write_loss(&format!(
                            "session {session_id} final flush failed: {error}"
                        ));
                    }
                }
                if let Ok(mut logs) = active_logs.lock() {
                    logs.remove(&session_id);
                }
                log::info!("Session Data Log stopped: {session_id}");
            }
            LogCommand::StopAllSessions => {
                clear_active_sessions();
                for (session_id, mut writer) in writers.drain() {
                    if let Err(error) = writer.flush() {
                        record_session_write_loss(&format!(
                            "session {session_id} final flush failed: {error}"
                        ));
                    }
                }
                if let Ok(mut logs) = active_logs.lock() {
                    logs.clear();
                }
            }
            LogCommand::Shutdown => {
                Self::flush_all(writers, active_logs, system_writer);
                return true;
            }
            LogCommand::ClearAll { response } => {
                let result = Self::clear_all(config, writers, active_logs, system_writer);
                let _ = response.send(result);
            }
        }
        false
    }

    fn clear_all(
        config: &LogConfig,
        writers: &mut HashMap<String, LogWriter>,
        active_logs: &Arc<Mutex<HashMap<String, LogStatus>>>,
        system_writer: &mut Option<SystemWriter>,
    ) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Some(writer) = system_writer.as_mut() {
            if let Err(error) = writer.close() {
                errors.push(format!("system log close failed: {error}"));
            }
        }
        *system_writer = None;

        for (session_id, writer) in writers.iter_mut() {
            if let Err(error) = writer.close() {
                errors.push(format!("session {session_id} close failed: {error}"));
            }
        }

        match std::fs::read_dir(&config.log_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|extension| extension == "log") {
                        if let Err(error) = std::fs::remove_file(&path) {
                            errors.push(format!("delete {path:?} failed: {error}"));
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!(
                "read log directory {:?} failed: {error}",
                config.log_dir
            )),
        }

        let mut failed_sessions = Vec::new();
        for (session_id, writer) in writers.iter_mut() {
            match writer.reopen() {
                Ok(()) => {
                    if let Ok(mut logs) = active_logs.lock() {
                        if let Some(status) = logs.get_mut(session_id) {
                            status.file_name = writer.file_name();
                            status.bytes_written = writer.bytes_written();
                        }
                    }
                }
                Err(error) => {
                    errors.push(format!("session {session_id} reopen failed: {error}"));
                    failed_sessions.push(session_id.clone());
                }
            }
        }
        for session_id in failed_sessions {
            writers.remove(&session_id);
            set_session_active(&session_id, false);
            if let Ok(mut logs) = active_logs.lock() {
                logs.remove(&session_id);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn flush_all_open(writers: &mut HashMap<String, LogWriter>, system_writer: &mut Option<SystemWriter>) {
        for (session_id, writer) in writers.iter_mut() {
            if let Err(error) = writer.flush() {
                record_session_write_loss(&format!(
                    "session {session_id} periodic flush failed: {error}"
                ));
            }
        }
        if let Some(writer) = system_writer.as_mut() {
            if let Err(error) = writer.flush() {
                record_system_write_loss(&format!("system log periodic flush failed: {error}"));
            }
        }
    }

    fn flush_all(
        writers: &mut HashMap<String, LogWriter>,
        active_logs: &Arc<Mutex<HashMap<String, LogStatus>>>,
        system_writer: &mut Option<SystemWriter>,
    ) {
        Self::flush_all_open(writers, system_writer);
        clear_active_sessions();
        if let Ok(mut logs) = active_logs.lock() {
            logs.clear();
        }
    }

    fn protected_paths(
        writers: &HashMap<String, LogWriter>,
        system_writer: Option<&SystemWriter>,
    ) -> HashSet<PathBuf> {
        let mut protected = writers
            .values()
            .map(|writer| writer.current_path().to_path_buf())
            .collect::<HashSet<_>>();
        if let Some(writer) = system_writer {
            protected.insert(writer.current_path().to_path_buf());
        }
        protected
    }

    fn maintain_storage(
        config: &LogConfig,
        protected: &HashSet<PathBuf>,
        max_total_bytes: u64,
    ) -> Result<(), String> {
        let entries = match std::fs::read_dir(&config.log_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("cannot read log directory: {error}")),
        };
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(config.retention_days.saturating_mul(86_400)));
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "log") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if !protected.contains(&path) && cutoff.is_some_and(|cutoff| modified < cutoff) {
                if let Err(error) = std::fs::remove_file(&path) {
                    record_maintenance_failure(&format!(
                        "retention delete {path:?} failed: {error}"
                    ));
                }
                continue;
            }
            files.push((path, metadata.len(), modified));
        }

        let mut total = files.iter().map(|(_, size, _)| *size).sum::<u64>();
        if total <= max_total_bytes {
            return Ok(());
        }
        files.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in files {
            if total <= max_total_bytes {
                break;
            }
            if protected.contains(&path) {
                continue;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => total = total.saturating_sub(size),
                Err(error) => record_maintenance_failure(&format!(
                    "quota delete {path:?} failed: {error}"
                )),
            }
        }
        Ok(())
    }
}

impl Drop for LogEngine {
    fn drop(&mut self) {
        let handle = self
            .consumer_handle
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(handle) = handle {
            let _ = self.entry_tx.send(LogEntry::Command(LogCommand::Shutdown));
            let _ = handle.join();
        } else {
            self.pending_rx
                .get_mut()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            clear_active_sessions();
        }
    }
}

struct SystemWriter {
    file: Option<BufWriter<File>>,
    current_path: PathBuf,
    base_dir: PathBuf,
    date: String,
    segment_index: u32,
    bytes_written: u64,
    split_threshold: u64,
    buffer_size: usize,
}

impl SystemWriter {
    fn new(config: &LogConfig, timestamp: &chrono::DateTime<Local>) -> std::io::Result<Self> {
        let mut writer = Self {
            file: None,
            current_path: PathBuf::new(),
            base_dir: config.log_dir.clone(),
            date: timestamp.format("%Y%m%d").to_string(),
            segment_index: 0,
            bytes_written: 0,
            split_threshold: config.file_max_size,
            buffer_size: config.buffer_size,
        };
        writer.open_unique_segment()?;
        Ok(writer)
    }

    fn write_event(
        &mut self,
        config: &LogConfig,
        level: &str,
        message: &str,
        timestamp: &chrono::DateTime<Local>,
    ) -> std::io::Result<()> {
        let date = timestamp.format("%Y%m%d").to_string();
        if date != self.date {
            self.close()?;
            self.date = date;
            self.segment_index = 0;
            self.bytes_written = 0;
            self.open_unique_segment()?;
        }
        self.split_threshold = config.file_max_size;
        if self.buffer_size != config.buffer_size {
            self.close()?;
            self.buffer_size = config.buffer_size;
            self.segment_index = self.segment_index.saturating_add(1);
            self.open_unique_segment()?;
        }

        let timestamp = timestamp.to_rfc3339_opts(SecondsFormat::Millis, false);
        let message = sanitize_log(message);
        let line = format!("[{timestamp}] [{}] {message}\n", level.to_ascii_uppercase());
        if self.bytes_written > 0
            && self.bytes_written + line.len() as u64 > self.split_threshold
        {
            self.close()?;
            self.segment_index = self.segment_index.saturating_add(1);
            self.open_unique_segment()?;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("system log writer is not open"))?;
        file.write_all(line.as_bytes())?;
        self.bytes_written += line.len() as u64;
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }

    fn close(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok(())
    }

    fn current_path(&self) -> &Path {
        &self.current_path
    }

    fn open_unique_segment(&mut self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        loop {
            let file_name = format!(
                "TauTerm_{}_p{}_{:03}.log",
                self.date,
                std::process::id(),
                self.segment_index
            );
            let path = self.base_dir.join(file_name);
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => {
                    self.file = Some(BufWriter::with_capacity(self.buffer_size, file));
                    self.current_path = path;
                    self.bytes_written = 0;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    self.segment_index = self.segment_index.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for SystemWriter {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(session_id: &str) -> DataLogEntry {
        DataLogEntry {
            session_id: session_id.to_string(),
            direction: DataDirection::RX,
            data_mode: "text".to_string(),
            encoding: "utf-8".to_string(),
            payload: b"hello".to_vec(),
            timestamp: Local::now(),
        }
    }

    #[test]
    fn inactive_session_logging_does_not_enter_the_queue() {
        let engine = LogEngine::new(LogConfig::default());
        try_send_session_log(&engine.sender(), sample_entry("not-active"));
        let guard = engine.pending_rx.lock().unwrap();
        assert!(guard.as_ref().unwrap().try_recv().is_err());
    }

    #[test]
    fn runtime_config_is_validated_at_the_rust_boundary() {
        let engine = LogEngine::new(LogConfig::default());
        assert!(engine
            .update_config(LogConfigUpdate {
                session_enabled: None,
                file_max_size: Some(0),
                buffer_size: None,
                flush_interval_ms: None,
                retention_days: None,
            })
            .is_err());
        assert!(engine
            .update_config(LogConfigUpdate {
                session_enabled: None,
                file_max_size: None,
                buffer_size: None,
                flush_interval_ms: Some(0),
                retention_days: None,
            })
            .is_err());
    }

    #[test]
    fn debug_level_is_not_filtered_by_the_global_log_facade() {
        set_system_log_config(true, "debug").unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Trace);
        assert!(system_level_allows(log::Level::Debug));
        assert!(!system_level_allows(log::Level::Trace));
    }

    #[test]
    fn startup_system_events_wait_for_final_log_directory() {
        set_system_log_config(true, "info").unwrap();
        let root = tempfile::tempdir().unwrap();
        let final_dir = root.path().join("final-logs");
        let timestamp = Local::now();
        let engine = LogEngine::new(LogConfig {
            flush_interval_ms: 100,
            ..Default::default()
        });
        try_send_system_event(
            &engine.sender(),
            "warn".into(),
            "startup-before-directory".into(),
            timestamp,
        );
        assert!(!final_dir.exists());
        engine.set_log_dir(final_dir.clone()).unwrap();

        let mut written = false;
        for _ in 0..100 {
            if let Ok(entries) = std::fs::read_dir(&final_dir) {
                for entry in entries.flatten() {
                    if std::fs::read_to_string(entry.path())
                        .is_ok_and(|content| content.contains("startup-before-directory"))
                    {
                        written = true;
                        break;
                    }
                }
            }
            if written {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(written, "queued startup event was not written");
    }

    #[test]
    fn log_directory_is_absolute_and_single_assignment() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let engine = LogEngine::new(LogConfig::default());
        assert!(engine.set_log_dir(PathBuf::from("relative-logs")).is_err());
        engine.set_log_dir(first).unwrap();
        assert!(engine.set_log_dir(second).is_err());
    }

    #[test]
    fn storage_quota_deletes_oldest_closed_segments_only() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.log");
        let second = root.path().join("second.log");
        let protected = root.path().join("protected.log");
        std::fs::write(&first, vec![0_u8; 80]).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(&second, vec![0_u8; 80]).unwrap();
        std::fs::write(&protected, vec![0_u8; 80]).unwrap();

        let config = LogConfig {
            log_dir: root.path().to_path_buf(),
            ..Default::default()
        };
        let protected_set = HashSet::from([protected.clone()]);
        LogEngine::maintain_storage(&config, &protected_set, 170).unwrap();

        assert!(!first.exists());
        assert!(second.exists());
        assert!(protected.exists());
    }
}
