//! Journald 日志查看器模块
//!
//! 通过 SSH exec 通道在远程主机上执行 `journalctl -o json` 命令，
//! 支持三种操作模式：
//!
//! ## 架构
//!
//! - **实时流**: 打开持久 exec 通道，spawn tokio task 循环读取 stdout 行，
//!   解析 JSON 后通过 Tauri 事件 `journald:entry` 推送到前端。
//! - **历史查询**: 单次 exec 调用，收集所有 stdout 行，提取最后一条的
//!   `__CURSOR` 作为分页游标返回前端。
//! - **日志导出**: 循环分页拉取所有匹配过滤条件的日志条目，流式写入 JSON 文件。
//!   通过事件 `journald:export-progress` / `journald:export-complete` /
//!   `journald:export-error` / `journald:export-cancelled` 向前端报告进度。
//!
//! 所有活动会话通过 `ActiveSessions` 注册表管理，提供统一的注册、取消、
//! 注销接口。每个活跃操作以 RAII Drop 守卫自动清理注册表，panic 安全。

use std::collections::{hash_map::Entry, HashMap};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex,
};

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use super::handler::SshHandler;

/// 活跃会话注册表
///
/// 管理正在进行的操作（流式追踪、导出等）的 session_id → cancel flag 映射。
/// 所有方法在 mutex 毒化时都会尝试恢复内部数据并继续操作。
pub(crate) struct ActiveSessions {
    label: &'static str,
    inner: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ActiveSessions {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 注册 session，返回 cancel flag。如果 session 已存在返回错误。
    pub fn register(&self, session_id: &str) -> Result<Arc<AtomicBool>, String> {
        // 与其他方法一致：毒锁时恢复内部数据继续操作，避免 panic 后所有新操作永久失败
        let mut map = self.inner.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "[{label}:{sid}] mutex 已污染，尝试恢复",
                label = self.label,
                sid = session_id
            );
            poisoned.into_inner()
        });
        // entry() 在锁内合并"检查已存在 + 插入"为单次查找，保持原子性
        match map.entry(session_id.to_string()) {
            Entry::Occupied(_) => Err("操作已在运行中".to_string()),
            Entry::Vacant(entry) => {
                let cancel = Arc::new(AtomicBool::new(false));
                entry.insert(cancel.clone());
                log::info!(
                    "[{label}:{sid}] 已注册",
                    label = self.label,
                    sid = session_id
                );
                Ok(cancel)
            }
        }
    }

    /// 发送取消信号（幂等：session 不存在时静默忽略）
    pub fn cancel(&self, session_id: &str) {
        match self.inner.lock() {
            Ok(map) => {
                if let Some(cancel) = map.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                    log::info!(
                        "[{label}:{sid}] 发送取消信号",
                        label = self.label,
                        sid = session_id
                    );
                }
            }
            Err(poisoned) => {
                log::error!(
                    "[{label}:{sid}] mutex 已污染，尝试恢复",
                    label = self.label,
                    sid = session_id
                );
                let map = poisoned.into_inner();
                if let Some(cancel) = map.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    /// 从注册表中移除 session（幂等：session 不存在时静默忽略）
    pub fn unregister(&self, session_id: &str) {
        match self.inner.lock() {
            Ok(mut map) => {
                map.remove(session_id);
            }
            Err(poisoned) => {
                log::error!(
                    "[{label}:{sid}] mutex 已污染，尝试恢复",
                    label = self.label,
                    sid = session_id
                );
                poisoned.into_inner().remove(session_id);
            }
        }
    }

    /// 等待 session 从注册表移除（确认式停止的等待侧）。
    ///
    /// 每 50ms 轮询一次，条目消失或到达 timeout 时返回。
    /// 任务退出时 RAII guard 会 unregister，因此条目消失即表示任务已真正结束。
    pub async fn wait_until_unregistered(&self, session_id: &str, timeout: std::time::Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let present = self
                .inner
                .lock()
                .map(|m| m.contains_key(session_id))
                .unwrap_or(true); // 毒锁时保守认为仍在，由超时兜底
            if !present {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

/// RAII 注册表清理守卫 — Drop 时从注册表中移除 session。
///
/// 无论任务正常退出、提前 return 还是 panic 展开，都会执行清理。
/// 流式追踪与日志导出共用此类型（unregister 幂等，重复调用安全）。
pub(crate) struct RegistrationGuard<'a>(&'a str, &'a ActiveSessions);

impl<'a> Drop for RegistrationGuard<'a> {
    fn drop(&mut self) {
        self.1.unregister(self.0);
    }
}

/// 全局活跃流注册表
static ACTIVE_STREAMS: LazyLock<ActiveSessions> =
    LazyLock::new(|| ActiveSessions::new("journald:stream"));

/// 全局活跃导出注册表
static ACTIVE_EXPORTS: LazyLock<ActiveSessions> =
    LazyLock::new(|| ActiveSessions::new("journald:export"));

/// 单次查询批大小上限（防止超大响应）
const MAX_QUERY_LIMIT: usize = 500;

// ── 数据结构 ────────────────────────────────────────

/// 单条 journald 日志条目（从 `journalctl -o json` 解析）
///
/// 字段名保持 journald JSON 输出的原始命名（`__` 前缀为 journald 内部字段，
/// 大写字段为 journald 标准字段）。`#[serde(flatten)]` 捕获未显式列出的动态字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// 单调时间戳（微秒），journald 内部
    #[serde(rename = "__MONOTONIC_TIMESTAMP")]
    pub monotonic_timestamp: Option<String>,
    /// 墙上时钟时间戳（微秒），journald 内部
    #[serde(rename = "__REALTIME_TIMESTAMP")]
    pub realtime_timestamp: Option<String>,
    /// journald 游标（用于分页）
    #[serde(rename = "__CURSOR")]
    pub cursor: Option<String>,
    /// syslog 标识符（通常是进程名，如 "sshd"）
    #[serde(rename = "SYSLOG_IDENTIFIER")]
    pub syslog_identifier: Option<String>,
    /// systemd 单元名（如 "sshd.service"）
    #[serde(rename = "_SYSTEMD_UNIT")]
    pub systemd_unit: Option<String>,
    /// 日志消息正文
    #[serde(rename = "MESSAGE")]
    pub message: Option<String>,
    /// 优先级 0-7 (0=emerg, 7=debug)
    #[serde(rename = "PRIORITY")]
    pub priority: Option<String>,
    /// 来源主机名
    #[serde(rename = "_HOSTNAME")]
    pub hostname: Option<String>,
    /// 启动 ID
    #[serde(rename = "_BOOT_ID")]
    pub boot_id: Option<String>,
    /// 其他所有未显式列出的字段
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// 历史查询过滤条件
#[derive(Debug, Clone)]
pub struct JournaldQueryFilters {
    /// 日志级别过滤（"emerg".."debug" 或 None 表示全部）
    pub level: Option<String>,
    /// 关键字搜索（传递给 `--grep=`）
    pub keyword: Option<String>,
    /// systemd 服务单元过滤（`-u` 参数）
    pub unit: Option<String>,
    /// 仅内核消息（`-k` 开关，等效 `--dmesg`）
    pub kernel_only: bool,
    /// 起始时间（ISO 8601，`-S` 参数）
    pub since: Option<String>,
    /// 结束时间（ISO 8601，`-U` 参数）
    pub until: Option<String>,
}

/// 历史查询命令的响应
#[derive(Debug, Clone, Serialize)]
pub struct JournaldQueryResponse {
    pub entries: Vec<JournalEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

// ── 命令构建 ────────────────────────────────────────

/// 将 `journalctl` 过滤条件组装为命令行参数向量。
///
/// 内部常量参数（`-o`、`-f` 等）原样拼接；用户提供的值（level/unit/时间/关键字）
/// 在此处经 [`shell_escape`] 单引号转义，防止 shell 元字符被解释为命令。
fn build_journalctl_args(
    filters: &JournaldQueryFilters,
    limit: usize,
    follow: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-o".into(), "json".into(), "--no-pager".into()];

    // 日志级别
    if let Some(ref level) = filters.level {
        args.push("-p".into());
        args.push(shell_escape(level));
    }

    // 服务单元
    if let Some(ref unit) = filters.unit {
        if !unit.is_empty() {
            args.push("-u".into());
            args.push(shell_escape(unit));
        }
    }

    // 内核消息
    if filters.kernel_only {
        args.push("-k".into());
    }

    // 时间范围
    if let Some(ref since) = filters.since {
        if !since.is_empty() {
            args.push("-S".into());
            args.push(shell_escape(since));
        }
    }
    if let Some(ref until) = filters.until {
        if !until.is_empty() {
            args.push("-U".into());
            args.push(shell_escape(until));
        }
    }

    // 关键字搜索
    if let Some(ref keyword) = filters.keyword {
        if !keyword.is_empty() {
            args.push(format!("--grep={}", shell_escape(keyword)));
        }
    }

    // 实时跟踪模式
    if follow {
        args.push("-f".into());
    } else {
        args.push(format!("-n{}", limit));
    }

    args
}

// ── 流式追踪 ────────────────────────────────────────

/// 启动 journald 实时流式追踪
///
/// 打开 SSH exec 通道执行 `journalctl -o json -f`，spawn tokio task
/// 循环读取 stdout 行 → 解析 JSON → emit `journald:entry` 事件。
///
/// # 取消机制
///
/// cancel flag 存储在全局 `ACTIVE_STREAMS` 中，前端调用
/// `stop_journald_stream` 时设置 flag = true，循环在下一次迭代中退出。
///
/// # 事件
///
/// - `journald:entry` — 每条新日志条目
/// - `journald:error` — 流式读取错误
/// - `journald:stream-ended` — 流正常结束（远程 journald 退出或通道关闭）
pub async fn start_journald_stream(
    session: &Arc<russh::client::Handle<SshHandler>>,
    app_handle: AppHandle,
    session_id: String,
    filters: &JournaldQueryFilters,
) -> Result<(), String> {
    // 1. 原子地检查并预留注册表位置（防止 TOCTOU 竞态）
    let cancel = ACTIVE_STREAMS
        .register(&session_id)
        .map_err(|e| format!("Journald 实时追踪已在运行中: {}", e))?;

    // 2. 打开 exec 通道（失败时回滚注册表）
    let mut channel = match session.channel_open_session().await {
        Ok(ch) => ch,
        Err(e) => {
            ACTIVE_STREAMS.unregister(&session_id);
            return Err(format!("打开 SSH exec 通道失败: {}", e));
        }
    };

    let args = build_journalctl_args(filters, 0, true);
    let cmd = build_command("journalctl", &args);

    log::info!("[journald:{}] 启动实时追踪: {}", session_id, cmd);

    if let Err(e) = channel.exec(true, cmd.as_str()).await {
        ACTIVE_STREAMS.unregister(&session_id);
        return Err(format!("执行 journalctl -f 失败: {}", e));
    }

    // 4. spawn tokio task 循环读取 stdout
    let sid = session_id.clone();
    tokio::spawn(async move {
        // RAII 清理守卫 — 任务退出（含 panic）时从注册表中移除
        let _guard = RegistrationGuard(&sid, &ACTIVE_STREAMS);

        let mut line_buf: Vec<u8> = Vec::new();

        loop {
            tokio::select! {
                // 每 500ms 检查取消标志（即使 journald 无新日志也能响应停止）
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    if cancel.load(Ordering::SeqCst) {
                        log::info!("[journald:{}] 流式追踪被用户取消", sid);
                        flush_line_buffer(&line_buf, &app_handle, &sid);
                        let _ = app_handle.emit("journald:stream-ended", serde_json::json!({
                            "session_id": sid,
                            "reason": "cancelled",
                        }));
                        break;
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { ref data }) => {
                            line_buf.extend_from_slice(data);

                            // 按行分割处理（仅在完整行上做 UTF-8 解码，防止多字节字符跨越 chunk 边界被损坏）
                            while let Some(newline_pos) = line_buf.iter().position(|&b| b == b'\n') {
                                // drain 用 ptr::copy 移动剩余元素，避免原 to_vec 每次整段复制（O(n²)）
                                let mut line_bytes: Vec<u8> = line_buf.drain(..=newline_pos).collect();
                                line_bytes.pop(); // 移除行尾换行符

                                let line = String::from_utf8_lossy(&line_bytes);
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }

                                match serde_json::from_str::<JournalEntry>(trimmed) {
                                    Ok(entry) => {
                                        let _ = app_handle.emit("journald:entry", serde_json::json!({
                                            "session_id": sid,
                                            "entry": entry,
                                        }));
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "[journald:{}] JSON 解析失败: {} (raw: {:.100})",
                                            sid, e, trimmed
                                        );
                                    }
                                }
                            }
                        }
                        Some(ChannelMsg::Eof) | None => {
                            // EOF — 远程 journalctl 退出
                            log::info!("[journald:{}] journalctl -f 进程退出 (EOF)", sid);
                            flush_line_buffer(&line_buf, &app_handle, &sid);
                            let _ = app_handle.emit("journald:stream-ended", serde_json::json!({
                                "session_id": sid,
                                "reason": "eof",
                            }));
                            break;
                        }
                        Some(ChannelMsg::Close) => {
                            log::info!("[journald:{}] SSH 通道已关闭", sid);
                            flush_line_buffer(&line_buf, &app_handle, &sid);
                            let _ = app_handle.emit("journald:stream-ended", serde_json::json!({
                                "session_id": sid,
                                "reason": "channel_closed",
                            }));
                            break;
                        }
                        Some(_other) => {
                            // 忽略其他消息类型（如 WindowChange、Signal 等）
                        }
                    }
                }
            }
        }
        // _guard Drop 自动从 ACTIVE_STREAMS 注册表中移除
    });

    Ok(())
}

/// 处理行缓冲中的最后一行（无尾随换行符）
fn flush_line_buffer(line: &[u8], app_handle: &AppHandle, session_id: &str) {
    let line_str = String::from_utf8_lossy(line);
    let trimmed = line_str.trim();
    if trimmed.is_empty() {
        return;
    }
    // 直接反序列化为 JournalEntry，避免经过 Value 中间层的双重解析
    if let Ok(entry) = serde_json::from_str::<JournalEntry>(trimmed) {
        let _ = app_handle.emit(
            "journald:entry",
            serde_json::json!({
                "session_id": session_id,
                "entry": entry,
            }),
        );
    }
}

/// 停止 journald 实时追踪
///
/// 设置对应 session 的 cancel 标志为 true。
/// 即使 session_id 不在注册表中也不会报错（幂等操作）。
/// 不等待任务退出，适合 close_session 等不阻塞的清理路径。
pub fn stop_journald_stream(session_id: &str) {
    ACTIVE_STREAMS.cancel(session_id);
}

/// 确认式停止 journald 实时追踪
///
/// 设置 cancel 标志后，等待后端任务真正退出并释放注册表（≤500ms 检测周期，
/// 2s 超时兜底）再返回。前端 await 此调用结束后即可安全地立即重新开始。
pub async fn stop_journald_stream_confirm(session_id: &str) {
    stop_journald_stream(session_id);
    ACTIVE_STREAMS
        .wait_until_unregistered(session_id, std::time::Duration::from_secs(2))
        .await;
}

// ── 历史查询 ────────────────────────────────────────

/// 查询 journald 历史日志（单次请求）
///
/// 打开 SSH exec 通道执行 `journalctl -o json --no-pager [filters] -n <limit> [--after-cursor=<cursor>]`，
/// 收集所有 stdout 行并解析为 `JournalEntry` 列表。
///
/// # 分页
///
/// 提取最后一条条目的 `__CURSOR` 字段作为 `next_cursor` 供前端下次查询。
/// `has_more` 为 true 表示返回条目数达到 limit（可能有更多数据）。
///
/// # 参数
///
/// - `cursor`: 可选，journald 游标（`--after-cursor=`），用于分页
/// - `limit`: 返回的最大条目数（上限 500）
pub async fn journald_query(
    session: &Arc<russh::client::Handle<SshHandler>>,
    filters: &JournaldQueryFilters,
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<JournalEntry>, Option<String>), String> {
    let limit = limit.clamp(1, MAX_QUERY_LIMIT);

    let mut args = build_journalctl_args(filters, limit, false);

    // 游标分页（游标来自 journald 输出，做转义以防异常字符破坏命令）
    if let Some(c) = cursor {
        if !c.is_empty() {
            args.push(format!("--after-cursor={}", shell_escape(c)));
        }
    }

    let cmd = build_command("journalctl", &args);
    log::info!("[journald:query] {}", cmd);

    // 打开 exec 通道
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SSH exec 通道失败: {}", e))?;

    channel.exec(true, cmd.as_str()).await.map_err(|e| {
        let err_str = e.to_string();
        if err_str.contains("command not found") || err_str.contains("No such file") {
            "远程主机上 journald (journalctl) 不可用".to_string()
        } else {
            format!("执行 journalctl 查询失败: {}", e)
        }
    })?;

    // 读取所有 stdout（通过 wait() 收集 ChannelMsg::Data）
    let mut stdout_buf = Vec::new();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                stdout_buf.extend_from_slice(&data);
            }
            Some(ChannelMsg::Eof) | None => {
                break;
            }
            Some(ChannelMsg::Close) => {
                break;
            }
            Some(_other) => {
                // 忽略其他消息
            }
        }
    }

    let stdout_str = String::from_utf8_lossy(&stdout_buf);

    // 解析 JSON 行
    let mut entries: Vec<JournalEntry> = Vec::new();
    let mut next_cursor: Option<String> = None;

    for line in stdout_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<JournalEntry>(trimmed) {
            Ok(entry) => {
                next_cursor = entry.cursor.clone();
                entries.push(entry);
            }
            Err(e) => {
                log::warn!(
                    "[journald:query] JSON 解析失败: {} (raw: {:.100})",
                    e,
                    trimmed
                );
            }
        }
    }

    Ok((entries, next_cursor))
}

// ── 工具函数 ────────────────────────────────────────

/// 对 shell 字符串值做单引号转义。
///
/// 算法：`'` + `value.replace("'", "'\\''")` + `'`。
/// 阻止用户输入中的 `;` `|` `$()` 等 shell 元字符被解释为命令。
fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 将命令名和参数向量组装为 shell 命令字符串。
///
/// 参数应为已转义的值：用户输入在 [`build_journalctl_args`] / 游标处经
/// [`shell_escape`] 处理，内部常量参数原样传入，此处仅做空格拼接。
fn build_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
    }
}

// ── 导出 ────────────────────────────────────────────

/// 单次分页查询的最大条目数（导出用，与查询共用常数）
///
/// 取 `MAX_QUERY_LIMIT` 上限（500），减少 SSH exec 通道往返次数
/// （每次分页都需新建通道，高延迟连接下开销显著）。
const EXPORT_PAGE_LIMIT: usize = 500;

/// 启动 journald 日志导出
///
/// 循环分页拉取所有匹配过滤条件的日志条目，逐页流式写入 JSON 文件
/// （紧凑格式，避免全量收集导致的内存峰值），完成后原子改名到目标路径。
/// spawn tokio task 异步执行，通过 Tauri 事件向前端报告进度。
///
/// # 事件
///
/// - `journald:export-progress` — 每页发出（节流 ≥200ms，最终页必发），携带 `{ loaded: number }`
/// - `journald:export-complete` — 导出完成，携带 `{ file_path: string, total: number }`
/// - `journald:export-error` — 导出失败（查询/写入错误），携带 `{ error: string }`
/// - `journald:export-cancelled` — 导出被用户取消，携带 `{ session_id: string }`
pub async fn start_journald_export(
    session: &Arc<russh::client::Handle<SshHandler>>,
    app_handle: AppHandle,
    session_id: String,
    filters: &JournaldQueryFilters,
    file_path: String,
) -> Result<(), String> {
    // 1. 原子地检查并预留注册表位置
    let cancel = ACTIVE_EXPORTS
        .register(&session_id)
        .map_err(|_| "导出已在运行中".to_string())?;

    // 2. 克隆 session 和 filters 供 tokio task 使用
    let session_clone = session.clone();
    let filters_clone = filters.clone();
    let sid = session_id.clone();
    let fp = file_path.clone();

    log::info!("[journald:export:{}] 开始导出 -> {}", sid, fp);

    tokio::spawn(async move {
        // RAII 清理守卫 — 任务退出（含 panic）时从注册表移除
        let _guard = RegistrationGuard(&sid, &ACTIVE_EXPORTS);

        // 先写入临时文件，全部成功后再原子改名到目标路径（与目标同目录，保证同卷）
        let tmp_path = format!("{}.tmp", fp);

        // 临时文件清理守卫 — 取消/失败/panic 时删除残留的 .tmp 文件
        struct TmpCleanup {
            path: String,
            keep: bool,
        }
        impl Drop for TmpCleanup {
            fn drop(&mut self) {
                if !self.keep {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
        let mut tmp_cleaner = TmpCleanup {
            path: tmp_path.clone(),
            keep: false,
        };

        // 异步打开/写入，避免阻塞 tokio worker 线程
        let mut file = match tokio::fs::File::create(&tmp_path).await {
            Ok(f) => f,
            Err(e) => {
                emit_export_error(&app_handle, &sid, &format!("创建导出文件失败: {}", e));
                return;
            }
        };

        // JSON 数组头
        if let Err(e) = file.write_all(b"[\n").await {
            emit_export_error(&app_handle, &sid, &format!("写入导出文件失败: {}", e));
            return;
        }

        let mut cursor: Option<String> = None;
        let mut total: usize = 0;
        let mut first_entry = true;
        // 进度事件节流：距上次发送不足 200ms 时跳过（最终页必发）
        let mut last_emit = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .unwrap_or_else(std::time::Instant::now);

        loop {
            // 检查取消标志
            if cancel.load(Ordering::SeqCst) {
                log::info!("[journald:export:{}] 导出被用户取消", sid);
                let _ = app_handle.emit(
                    "journald:export-cancelled",
                    serde_json::json!({
                        "session_id": sid,
                    }),
                );
                return; // TmpCleanup Drop 删除临时文件
            }

            // 执行单次分页查询
            match journald_query(
                &session_clone,
                &filters_clone,
                cursor.as_deref(),
                EXPORT_PAGE_LIMIT,
            )
            .await
            {
                Ok((entries, next_cursor)) => {
                    let page_size = entries.len();

                    // 逐条流式写入（紧凑 JSON，内存占用与页大小成正比）
                    for entry in &entries {
                        if !first_entry {
                            if let Err(e) = file.write_all(b",\n").await {
                                emit_export_error(
                                    &app_handle,
                                    &sid,
                                    &format!("写入导出文件失败: {}", e),
                                );
                                return;
                            }
                        }
                        first_entry = false;
                        match serde_json::to_string(entry) {
                            Ok(json) => {
                                if let Err(e) = file.write_all(json.as_bytes()).await {
                                    emit_export_error(
                                        &app_handle,
                                        &sid,
                                        &format!("写入导出文件失败: {}", e),
                                    );
                                    return;
                                }
                            }
                            Err(e) => {
                                emit_export_error(
                                    &app_handle,
                                    &sid,
                                    &format!("序列化日志条目失败: {}", e),
                                );
                                return;
                            }
                        }
                    }

                    total += page_size;
                    cursor = next_cursor;

                    // 无更多数据时退出循环（满页但无游标同样终止，防止死循环）
                    let is_final = page_size < EXPORT_PAGE_LIMIT || cursor.is_none();

                    // 节流进度事件
                    let now = std::time::Instant::now();
                    if now.duration_since(last_emit) >= std::time::Duration::from_millis(200)
                        || is_final
                    {
                        let _ = app_handle.emit(
                            "journald:export-progress",
                            serde_json::json!({
                                "session_id": sid,
                                "loaded": total,
                            }),
                        );
                        last_emit = now;
                    }

                    if is_final {
                        break;
                    }
                }
                Err(e) => {
                    emit_export_error(&app_handle, &sid, &format!("查询失败: {}", e));
                    return; // TmpCleanup Drop 删除临时文件
                }
            }
        }

        // 写入 JSON 数组尾并落盘
        if let Err(e) = file.write_all(b"\n]\n").await {
            emit_export_error(&app_handle, &sid, &format!("写入导出文件失败: {}", e));
            return;
        }
        if let Err(e) = file.flush().await {
            emit_export_error(&app_handle, &sid, &format!("写入导出文件失败: {}", e));
            return;
        }
        drop(file); // 关闭文件句柄（Windows 下 rename 前必须释放）

        // 原子改名（同目录，跨平台安全）。目标已存在时先删除再重试（正常由保存对话框保证不存在）。
        if let Err(e) = tokio::fs::rename(&tmp_path, &fp).await {
            log::warn!(
                "[journald:export:{}] rename 失败（目标可能已存在），尝试覆盖: {}",
                sid,
                e
            );
            let _ = tokio::fs::remove_file(&fp).await;
            if let Err(e2) = tokio::fs::rename(&tmp_path, &fp).await {
                emit_export_error(&app_handle, &sid, &format!("保存导出文件失败: {}", e2));
                return;
            }
        }

        tmp_cleaner.keep = true; // 已改名成功，保留目标文件

        log::info!("[journald:export:{}] 导出完成: {} 条 → {}", sid, total, fp);
        let _ = app_handle.emit(
            "journald:export-complete",
            serde_json::json!({
                "session_id": sid,
                "file_path": fp,
                "total": total,
            }),
        );
        // _guard Drop 自动清理注册表
    });

    Ok(())
}

/// 导出失败统一处理：日志 + 发送 `journald:export-error` 事件
fn emit_export_error(app: &AppHandle, sid: &str, msg: &str) {
    log::error!("[journald:export:{}] {}", sid, msg);
    let _ = app.emit(
        "journald:export-error",
        serde_json::json!({
            "session_id": sid,
            "error": msg,
        }),
    );
}

/// 停止 journald 导出
///
/// 设置对应 session 的 cancel 标志为 true。
/// 即使 session_id 不在注册表中也不会报错（幂等操作）。
pub fn stop_journald_export(session_id: &str) {
    ACTIVE_EXPORTS.cancel(session_id);
}
