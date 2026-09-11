//! SSH journald data source.
//!
//! Owns journalctl command construction, bounded command execution, lossless
//! record normalization, newest-to-oldest cursor paging, realtime batching,
//! cancellation, and streaming export.

use std::collections::{hash_map::Entry, BTreeMap, HashMap};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex,
};
use std::time::Duration;

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

use super::handler::SshHandler;

const MAX_QUERY_LIMIT: usize = 500;
const EXPORT_PAGE_LIMIT: usize = MAX_QUERY_LIMIT;
const STREAM_BATCH_SIZE: usize = 128;
const STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const MAX_STREAM_LINE_BYTES: usize = 1024 * 1024;
const MAX_QUERY_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

// ── Operation lifecycle ────────────────────────────────────────────────

struct OperationState {
    cancelled: AtomicBool,
    cancel_notify: Notify,
    done: AtomicBool,
    done_notify: Notify,
}

impl OperationState {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            cancel_notify: Notify::new(),
            done: AtomicBool::new(false),
            done_notify: Notify::new(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.cancel_notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    fn mark_done(&self) {
        self.done.store(true, Ordering::Release);
        self.done_notify.notify_waiters();
    }

    async fn wait_done(&self) {
        loop {
            if self.done.load(Ordering::Acquire) {
                return;
            }
            let notified = self.done_notify.notified();
            if self.done.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

struct ActiveOperations {
    label: &'static str,
    inner: Mutex<HashMap<String, Arc<OperationState>>>,
}

impl ActiveOperations {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn register(&'static self, session_id: &str) -> Result<OperationGuard, String> {
        let mut operations = self.inner.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "[{}:{}] operation registry poisoned; recovering",
                self.label,
                session_id
            );
            poisoned.into_inner()
        });
        match operations.entry(session_id.to_string()) {
            Entry::Occupied(_) => Err("operation_already_running".to_string()),
            Entry::Vacant(entry) => {
                let state = Arc::new(OperationState::new());
                entry.insert(state.clone());
                Ok(OperationGuard {
                    session_id: session_id.to_string(),
                    registry: self,
                    state,
                })
            }
        }
    }

    fn cancel(&self, session_id: &str) -> Option<Arc<OperationState>> {
        let operations = self.inner.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "[{}:{}] operation registry poisoned; recovering",
                self.label,
                session_id
            );
            poisoned.into_inner()
        });
        let state = operations.get(session_id).cloned();
        if let Some(state) = &state {
            state.cancel();
        }
        state
    }

    fn unregister(&self, session_id: &str) {
        let mut operations = self.inner.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "[{}:{}] operation registry poisoned; recovering",
                self.label,
                session_id
            );
            poisoned.into_inner()
        });
        operations.remove(session_id);
    }
}

struct OperationGuard {
    session_id: String,
    registry: &'static ActiveOperations,
    state: Arc<OperationState>,
}

impl OperationGuard {
    fn state(&self) -> Arc<OperationState> {
        self.state.clone()
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.session_id);
        self.state.mark_done();
    }
}

static ACTIVE_STREAMS: LazyLock<ActiveOperations> =
    LazyLock::new(|| ActiveOperations::new("journald:stream"));
static ACTIVE_EXPORTS: LazyLock<ActiveOperations> =
    LazyLock::new(|| ActiveOperations::new("journald:export"));

// ── Domain model ──────────────────────────────────────────────────────

/// Normalized common fields plus the complete original journald object.
/// Journald JSON values are not assumed to be strings: duplicate or binary
/// fields may be arrays/null and are kept losslessly in `fields`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub monotonic_timestamp: Option<String>,
    pub realtime_timestamp: Option<String>,
    pub cursor: Option<String>,
    pub identifier: Option<String>,
    pub unit: Option<String>,
    pub message: Option<String>,
    pub priority: Option<String>,
    pub hostname: Option<String>,
    pub boot_id: Option<String>,
    pub fields: BTreeMap<String, Value>,
}

impl JournalEntry {
    fn from_raw(raw: Map<String, Value>) -> Self {
        Self {
            monotonic_timestamp: raw.get("__MONOTONIC_TIMESTAMP").and_then(value_to_text),
            realtime_timestamp: raw.get("__REALTIME_TIMESTAMP").and_then(value_to_text),
            cursor: raw.get("__CURSOR").and_then(value_to_text),
            identifier: raw.get("SYSLOG_IDENTIFIER").and_then(value_to_text),
            unit: raw.get("_SYSTEMD_UNIT").and_then(value_to_text),
            message: raw.get("MESSAGE").and_then(value_to_text),
            priority: raw.get("PRIORITY").and_then(value_to_text),
            hostname: raw.get("_HOSTNAME").and_then(value_to_text),
            boot_id: raw.get("_BOOT_ID").and_then(value_to_text),
            fields: raw.into_iter().collect(),
        }
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Array(values) => {
            let parts: Vec<String> = values.iter().filter_map(value_to_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

fn parse_entry(line: &str) -> Result<JournalEntry, serde_json::Error> {
    serde_json::from_str::<Map<String, Value>>(line).map(JournalEntry::from_raw)
}

#[derive(Debug, Clone)]
pub struct JournaldQueryFilters {
    pub level: Option<String>,
    /// PCRE expression passed to journalctl. The UI escapes literal search text.
    pub keyword: Option<String>,
    pub unit: Option<String>,
    /// Uses `_TRANSPORT=kernel`, not `-k`, so historical queries are not
    /// implicitly restricted to the current boot.
    pub kernel_only: bool,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JournaldQueryResponse {
    pub entries: Vec<JournalEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

pub fn error_code_for_message(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("operation_already_running") || lower.contains("already running") {
        "already_running"
    } else if lower.contains("command not found")
        || lower.contains("journalctl: not found")
        || lower.contains("no such file")
    {
        "command_unavailable"
    } else if lower.contains("permission denied") || lower.contains("not permitted") {
        "permission_denied"
    } else if lower.contains("invalid regular expression")
        || lower.contains("pcre2")
        || lower.contains("invalid argument")
    {
        "invalid_filter"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("cancel") {
        "cancelled"
    } else if lower.contains("json") || lower.contains("parse") {
        "parse_error"
    } else if lower.contains("ssh") || lower.contains("channel") || lower.contains("connection") {
        "connection_error"
    } else {
        "io_error"
    }
}

fn error_payload(message: impl Into<String>) -> Value {
    let message = message.into();
    serde_json::json!({
        "code": error_code_for_message(&message),
        "message": message,
    })
}

// ── Command construction and bounded runner ──────────────────────────

fn build_filter_args(filters: &JournaldQueryFilters) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "json".to_string(),
        "--no-pager".to_string(),
        "--quiet".to_string(),
    ];
    if let Some(level) = filters.level.as_deref().filter(|value| !value.is_empty()) {
        args.push("-p".to_string());
        args.push(shell_escape(level));
    }
    if let Some(unit) = filters.unit.as_deref().filter(|value| !value.is_empty()) {
        args.push("-u".to_string());
        args.push(shell_escape(unit));
    }
    if filters.kernel_only {
        args.push("_TRANSPORT=kernel".to_string());
    }
    if let Some(since) = filters.since.as_deref().filter(|value| !value.is_empty()) {
        args.push("-S".to_string());
        args.push(shell_escape(since));
    }
    if let Some(until) = filters.until.as_deref().filter(|value| !value.is_empty()) {
        args.push("-U".to_string());
        args.push(shell_escape(until));
    }
    if let Some(keyword) = filters.keyword.as_deref().filter(|value| !value.is_empty()) {
        args.push(format!("--grep={}", shell_escape(keyword)));
    }
    args
}

fn build_stream_args(filters: &JournaldQueryFilters) -> Vec<String> {
    let mut args = build_filter_args(filters);
    // TauTerm realtime means entries created after Start; do not inherit
    // journalctl's implicit tail.
    args.push("-n0".to_string());
    args.push("-f".to_string());
    args
}

fn build_history_args(
    filters: &JournaldQueryFilters,
    cursor: Option<&str>,
    limit: usize,
) -> Vec<String> {
    let mut args = build_filter_args(filters);
    // Reverse output makes every next page older. One look-ahead record proves
    // has_more; cursor pages need one additional record for the inclusive cursor.
    let lookahead = if cursor.is_some() { 2 } else { 1 };
    args.push("-r".to_string());
    args.push(format!("-n{}", limit.saturating_add(lookahead)));
    if let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) {
        args.push(format!("--cursor={}", shell_escape(cursor)));
    }
    args
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn build_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
    }
}

struct JournalctlOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_status: Option<u32>,
}

async fn run_journalctl(
    session: &Arc<russh::client::Handle<SshHandler>>,
    args: &[String],
) -> Result<JournalctlOutput, String> {
    let command = build_command("journalctl", args);
    log::debug!("[journald] exec: {}", command);

    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|error| format!("SSH channel open failed: {error}"))?;
    channel
        .exec(true, command.as_str())
        .await
        .map_err(|error| format!("journalctl exec failed: {error}"))?;

    let collect = async {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    if stdout.len().saturating_add(data.len()) > MAX_QUERY_OUTPUT_BYTES {
                        return Err("journalctl output exceeded bounded query buffer".to_string());
                    }
                    stdout.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExtendedData { data, ext }) => {
                    if ext == 1 && stderr.len() < MAX_STDERR_BYTES {
                        let remaining = MAX_STDERR_BYTES - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(remaining)]);
                    }
                }
                Some(ChannelMsg::ExitStatus {
                    exit_status: status,
                }) => {
                    exit_status = Some(status);
                }
                Some(ChannelMsg::Eof) => {
                    // Continue until close/None so an exit status arriving after EOF is observed.
                }
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        Ok(JournalctlOutput {
            stdout,
            stderr,
            exit_status,
        })
    };

    let output = tokio::time::timeout(QUERY_TIMEOUT, collect)
        .await
        .map_err(|_| "journalctl query timed out".to_string())??;
    if output.exit_status.unwrap_or(0) != 0 {
        return Err(journalctl_failure_message(&output));
    }
    Ok(output)
}

fn journalctl_failure_message(output: &JournalctlOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!(
            "journalctl exited with status {}",
            output.exit_status.unwrap_or(u32::MAX)
        )
    } else {
        format!(
            "journalctl exited with status {}: {}",
            output.exit_status.unwrap_or(u32::MAX),
            stderr
        )
    }
}

fn parse_output_entries(stdout: &[u8]) -> Vec<JournalEntry> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            match parse_entry(line) {
                Ok(entry) => Some(entry),
                Err(error) => {
                    log::warn!(
                        "[journald] invalid JSON record: {} (raw: {:.160})",
                        error,
                        line
                    );
                    None
                }
            }
        })
        .collect()
}

// ── History paging ───────────────────────────────────────────────────

fn finalize_page(
    mut entries: Vec<JournalEntry>,
    cursor: Option<&str>,
    limit: usize,
) -> JournaldQueryResponse {
    if let Some(cursor) = cursor {
        if let Some(index) = entries
            .iter()
            .position(|entry| entry.cursor.as_deref() == Some(cursor))
        {
            entries.remove(index);
        }
    }
    let has_more = entries.len() > limit;
    entries.truncate(limit);
    let next_cursor = if has_more {
        entries.last().and_then(|entry| entry.cursor.clone())
    } else {
        None
    };
    let has_more = has_more && next_cursor.is_some();
    JournaldQueryResponse {
        entries,
        next_cursor,
        has_more,
    }
}

pub async fn journald_query_page(
    session: &Arc<russh::client::Handle<SshHandler>>,
    filters: &JournaldQueryFilters,
    cursor: Option<&str>,
    limit: usize,
) -> Result<JournaldQueryResponse, String> {
    let limit = limit.clamp(1, MAX_QUERY_LIMIT);
    let output = run_journalctl(session, &build_history_args(filters, cursor, limit)).await?;
    Ok(finalize_page(
        parse_output_entries(&output.stdout),
        cursor,
        limit,
    ))
}

/// Adapter for the current Tauri command boundary. Paging semantics stay in
/// `journald_query_page`; `next_cursor` is present only when look-ahead proved
/// that another older page exists.
pub async fn journald_query(
    session: &Arc<russh::client::Handle<SshHandler>>,
    filters: &JournaldQueryFilters,
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<JournalEntry>, Option<String>), String> {
    let page = journald_query_page(session, filters, cursor, limit).await?;
    Ok((page.entries, page.next_cursor))
}

// ── Realtime stream ──────────────────────────────────────────────────

fn push_complete_lines(line_buffer: &mut Vec<u8>, batch: &mut Vec<JournalEntry>, session_id: &str) {
    while let Some(newline) = line_buffer.iter().position(|byte| *byte == b'\n') {
        let mut line = line_buffer.drain(..=newline).collect::<Vec<_>>();
        line.pop();
        let line = String::from_utf8_lossy(&line);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_entry(line) {
            Ok(entry) => batch.push(entry),
            Err(error) => log::warn!(
                "[journald:{}] invalid stream JSON: {} (raw: {:.160})",
                session_id,
                error,
                line
            ),
        }
    }
}

fn flush_partial_line(line_buffer: &mut Vec<u8>, batch: &mut Vec<JournalEntry>, session_id: &str) {
    if line_buffer.is_empty() {
        return;
    }
    let line = String::from_utf8_lossy(line_buffer);
    let line = line.trim();
    if !line.is_empty() {
        match parse_entry(line) {
            Ok(entry) => batch.push(entry),
            Err(error) => log::warn!(
                "[journald:{}] invalid final stream JSON: {} (raw: {:.160})",
                session_id,
                error,
                line
            ),
        }
    }
    line_buffer.clear();
}

fn emit_stream_batch(app: &AppHandle, session_id: &str, batch: &mut Vec<JournalEntry>) {
    if batch.is_empty() {
        return;
    }
    let entries = std::mem::take(batch);
    let _ = app.emit(
        "journald:batch",
        serde_json::json!({
            "session_id": session_id,
            "entries": entries,
            "dropped": 0usize,
        }),
    );
}

fn emit_stream_error(app: &AppHandle, session_id: &str, message: impl Into<String>) {
    let _ = app.emit(
        "journald:error",
        serde_json::json!({
            "session_id": session_id,
            "error": error_payload(message),
        }),
    );
}

fn emit_stream_ended(app: &AppHandle, session_id: &str, reason: &str) {
    let _ = app.emit(
        "journald:stream-ended",
        serde_json::json!({
            "session_id": session_id,
            "reason": reason,
        }),
    );
}

pub async fn start_journald_stream(
    session: &Arc<russh::client::Handle<SshHandler>>,
    app_handle: AppHandle,
    session_id: String,
    filters: &JournaldQueryFilters,
) -> Result<(), String> {
    let guard = ACTIVE_STREAMS
        .register(&session_id)
        .map_err(|error| format!("journald stream: {error}"))?;
    let operation = guard.state();

    let mut channel = match session.channel_open_session().await {
        Ok(channel) => channel,
        Err(error) => {
            drop(guard);
            return Err(format!("SSH channel open failed: {error}"));
        }
    };
    let command = build_command("journalctl", &build_stream_args(filters));
    if let Err(error) = channel.exec(true, command.as_str()).await {
        drop(guard);
        return Err(format!("journalctl follow exec failed: {error}"));
    }

    let sid = session_id.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let mut line_buffer = Vec::new();
        let mut batch = Vec::with_capacity(STREAM_BATCH_SIZE);
        let mut stderr = Vec::new();
        let mut exit_status = None;
        let mut flush_tick = tokio::time::interval(STREAM_FLUSH_INTERVAL);
        flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = operation.cancelled() => {
                    let _ = channel.close().await;
                    flush_partial_line(&mut line_buffer, &mut batch, &sid);
                    emit_stream_batch(&app_handle, &sid, &mut batch);
                    // Explicit Stop awaits this task through OperationState. Do not
                    // emit stream-ended for cancellation: a queued old end event
                    // could otherwise flip a freshly restarted stream back to false.
                    break;
                }
                _ = flush_tick.tick() => {
                    emit_stream_batch(&app_handle, &sid, &mut batch);
                }
                message = channel.wait() => {
                    match message {
                        Some(ChannelMsg::Data { data }) => {
                            line_buffer.extend_from_slice(&data);
                            if line_buffer.len() > MAX_STREAM_LINE_BYTES {
                                emit_stream_error(
                                    &app_handle,
                                    &sid,
                                    "journalctl stream line exceeded 1 MiB safety limit",
                                );
                                line_buffer.clear();
                            } else {
                                push_complete_lines(&mut line_buffer, &mut batch, &sid);
                                if batch.len() >= STREAM_BATCH_SIZE {
                                    emit_stream_batch(&app_handle, &sid, &mut batch);
                                }
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 && stderr.len() < MAX_STDERR_BYTES {
                                let remaining = MAX_STDERR_BYTES - stderr.len();
                                stderr.extend_from_slice(&data[..data.len().min(remaining)]);
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status: status }) => {
                            exit_status = Some(status);
                        }
                        Some(ChannelMsg::Eof) => {
                            flush_partial_line(&mut line_buffer, &mut batch, &sid);
                            emit_stream_batch(&app_handle, &sid, &mut batch);
                        }
                        Some(ChannelMsg::Close) | None => {
                            flush_partial_line(&mut line_buffer, &mut batch, &sid);
                            emit_stream_batch(&app_handle, &sid, &mut batch);
                            if exit_status.unwrap_or(0) != 0 {
                                let output = JournalctlOutput {
                                    stdout: Vec::new(),
                                    stderr,
                                    exit_status,
                                };
                                emit_stream_error(
                                    &app_handle,
                                    &sid,
                                    journalctl_failure_message(&output),
                                );
                                emit_stream_ended(&app_handle, &sid, "error");
                            } else {
                                emit_stream_ended(&app_handle, &sid, "eof");
                            }
                            break;
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    });
    Ok(())
}

pub fn stop_journald_stream(session_id: &str) {
    ACTIVE_STREAMS.cancel(session_id);
}

pub async fn stop_journald_stream_confirm(session_id: &str) {
    if let Some(operation) = ACTIVE_STREAMS.cancel(session_id) {
        operation.wait_done().await;
    }
}

// ── Streaming export ─────────────────────────────────────────────────

pub async fn start_journald_export(
    session: &Arc<russh::client::Handle<SshHandler>>,
    app_handle: AppHandle,
    session_id: String,
    filters: &JournaldQueryFilters,
    file_path: String,
) -> Result<(), String> {
    let guard = ACTIVE_EXPORTS
        .register(&session_id)
        .map_err(|error| format!("journald export: {error}"))?;
    let operation = guard.state();
    let session = session.clone();
    let filters = filters.clone();
    let sid = session_id.clone();
    let target_path = file_path.clone();

    tokio::spawn(async move {
        let _guard = guard;
        let temporary_path = format!("{}.tmp", target_path);

        struct TempFileGuard {
            path: String,
            keep: bool,
        }
        impl Drop for TempFileGuard {
            fn drop(&mut self) {
                if !self.keep {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }

        let mut temp_guard = TempFileGuard {
            path: temporary_path.clone(),
            keep: false,
        };
        let mut file = match tokio::fs::File::create(&temporary_path).await {
            Ok(file) => file,
            Err(error) => {
                emit_export_error(
                    &app_handle,
                    &sid,
                    format!("create export file failed: {error}"),
                );
                return;
            }
        };
        if let Err(error) = file.write_all(b"[\n").await {
            emit_export_error(
                &app_handle,
                &sid,
                format!("write export file failed: {error}"),
            );
            return;
        }

        let mut cursor: Option<String> = None;
        let mut total = 0usize;
        let mut first = true;
        let mut last_progress = std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(std::time::Instant::now);

        loop {
            if operation.is_cancelled() {
                emit_export_cancelled(&app_handle, &sid);
                return;
            }
            let page =
                match journald_query_page(&session, &filters, cursor.as_deref(), EXPORT_PAGE_LIMIT)
                    .await
                {
                    Ok(page) => page,
                    Err(error) => {
                        emit_export_error(&app_handle, &sid, format!("query failed: {error}"));
                        return;
                    }
                };

            for entry in &page.entries {
                if operation.is_cancelled() {
                    emit_export_cancelled(&app_handle, &sid);
                    return;
                }
                if !first {
                    if let Err(error) = file.write_all(b",\n").await {
                        emit_export_error(
                            &app_handle,
                            &sid,
                            format!("write export file failed: {error}"),
                        );
                        return;
                    }
                }
                first = false;
                let json = match serde_json::to_vec(entry) {
                    Ok(json) => json,
                    Err(error) => {
                        emit_export_error(
                            &app_handle,
                            &sid,
                            format!("serialize log entry failed: {error}"),
                        );
                        return;
                    }
                };
                if let Err(error) = file.write_all(&json).await {
                    emit_export_error(
                        &app_handle,
                        &sid,
                        format!("write export file failed: {error}"),
                    );
                    return;
                }
            }

            total += page.entries.len();
            let final_page = !page.has_more || page.next_cursor.is_none();
            let now = std::time::Instant::now();
            if final_page || now.duration_since(last_progress) >= Duration::from_millis(200) {
                let _ = app_handle.emit(
                    "journald:export-progress",
                    serde_json::json!({ "session_id": sid, "loaded": total }),
                );
                last_progress = now;
            }
            if final_page {
                break;
            }
            cursor = page.next_cursor;
        }

        if let Err(error) = file.write_all(b"\n]\n").await {
            emit_export_error(
                &app_handle,
                &sid,
                format!("write export file failed: {error}"),
            );
            return;
        }
        if let Err(error) = file.flush().await {
            emit_export_error(
                &app_handle,
                &sid,
                format!("flush export file failed: {error}"),
            );
            return;
        }
        drop(file);

        if operation.is_cancelled() {
            emit_export_cancelled(&app_handle, &sid);
            return;
        }
        if let Err(first_error) = tokio::fs::rename(&temporary_path, &target_path).await {
            let _ = tokio::fs::remove_file(&target_path).await;
            if let Err(second_error) = tokio::fs::rename(&temporary_path, &target_path).await {
                emit_export_error(
                    &app_handle,
                    &sid,
                    format!(
                        "save export file failed: {second_error} (initial rename: {first_error})"
                    ),
                );
                return;
            }
        }
        temp_guard.keep = true;
        let _ = app_handle.emit(
            "journald:export-complete",
            serde_json::json!({
                "session_id": sid,
                "file_path": target_path,
                "total": total,
            }),
        );
    });
    Ok(())
}

fn emit_export_error(app: &AppHandle, session_id: &str, message: impl Into<String>) {
    let message = message.into();
    log::error!("[journald:export:{}] {}", session_id, message);
    let _ = app.emit(
        "journald:export-error",
        serde_json::json!({
            "session_id": session_id,
            "error": error_payload(message),
        }),
    );
}

fn emit_export_cancelled(app: &AppHandle, session_id: &str) {
    let _ = app.emit(
        "journald:export-cancelled",
        serde_json::json!({ "session_id": session_id }),
    );
}

pub fn stop_journald_export(session_id: &str) {
    ACTIVE_EXPORTS.cancel(session_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters() -> JournaldQueryFilters {
        JournaldQueryFilters {
            level: None,
            keyword: None,
            unit: None,
            kernel_only: false,
            since: None,
            until: None,
        }
    }

    fn entry(cursor: &str, timestamp: &str) -> JournalEntry {
        JournalEntry {
            monotonic_timestamp: None,
            realtime_timestamp: Some(timestamp.to_string()),
            cursor: Some(cursor.to_string()),
            identifier: None,
            unit: None,
            message: Some(cursor.to_string()),
            priority: Some("6".to_string()),
            hostname: None,
            boot_id: None,
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn history_is_reverse_and_uses_inclusive_cursor() {
        let args = build_history_args(&filters(), Some("cursor-100"), 100);
        assert!(args.iter().any(|arg| arg == "-r"));
        assert!(args.iter().any(|arg| arg == "-n102"));
        assert!(args.iter().any(|arg| arg.starts_with("--cursor=")));
        assert!(!args.iter().any(|arg| arg.starts_with("--after-cursor=")));
    }

    #[test]
    fn realtime_has_no_implicit_tail() {
        let args = build_stream_args(&filters());
        assert!(args.iter().any(|arg| arg == "-n0"));
        assert!(args.iter().any(|arg| arg == "-f"));
    }

    #[test]
    fn kernel_filter_does_not_imply_current_boot() {
        let mut filter = filters();
        filter.kernel_only = true;
        let args = build_filter_args(&filter);
        assert!(args.iter().any(|arg| arg == "_TRANSPORT=kernel"));
        assert!(!args.iter().any(|arg| arg == "-k"));
    }

    #[test]
    fn exact_has_more_uses_lookahead_and_cursor() {
        let page = finalize_page(
            vec![entry("c5", "5"), entry("c4", "4"), entry("c3", "3")],
            None,
            2,
        );
        assert_eq!(page.entries.len(), 2);
        assert!(page.has_more);
        assert_eq!(page.next_cursor.as_deref(), Some("c4"));

        let next = finalize_page(
            vec![
                entry("c4", "4"),
                entry("c3", "3"),
                entry("c2", "2"),
                entry("c1", "1"),
            ],
            Some("c4"),
            2,
        );
        assert_eq!(
            next.entries
                .iter()
                .map(|entry| entry.cursor.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("c3"), Some("c2")]
        );
        assert!(next.has_more);
        assert_eq!(next.next_cursor.as_deref(), Some("c2"));
    }

    #[test]
    fn raw_non_string_fields_are_preserved() {
        let raw = r#"{"MESSAGE":["a","b"],"PRIORITY":6,"BINARY":[1,2,3],"__CURSOR":"c"}"#;
        let parsed = parse_entry(raw).expect("entry should parse");
        assert_eq!(parsed.message.as_deref(), Some("a\nb"));
        assert_eq!(parsed.priority.as_deref(), Some("6"));
        assert_eq!(parsed.cursor.as_deref(), Some("c"));
        assert!(matches!(parsed.fields.get("BINARY"), Some(Value::Array(_))));
    }
}
