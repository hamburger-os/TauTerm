//! Journald 日志查看器模块
//!
//! 通过 SSH exec 通道在远程主机上执行 `journalctl -o json` 命令，
//! 提供实时流式追踪（journalctl -f）和历史查询（游标分页）两种模式。
//!
//! ## 架构
//!
//! - **实时流**: 打开持久 exec 通道，spawn tokio task 循环读取 stdout 行，
//!   解析 JSON 后通过 Tauri 事件 `journald:entry` 推送到前端。
//!   通过全局 `ACTIVE_STREAMS` 注册表管理取消标志。
//! - **历史查询**: 单次 exec 调用，收集所有 stdout 行，提取最后一条的
//!   `__CURSOR` 作为分页游标返回前端。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, atomic::{AtomicBool, Ordering}, Mutex};

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::handler::SshHandler;

/// 全局活跃流注册表: session_id → cancel flag
///
/// 前端调用 `stop_journald_stream` 时查找对应 cancel 标志并设置为 true，
/// tokio 流式循环在每次迭代中检查此标志以优雅退出。
static ACTIVE_STREAMS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
/// 所有过滤参数在 shell 外拼接，不经过 shell 转义。
fn build_journalctl_args(filters: &JournaldQueryFilters, limit: usize, follow: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "json".into(),
        "--no-pager".into(),
    ];

    // 日志级别
    if let Some(ref level) = filters.level {
        args.push("-p".into());
        args.push(level.clone());
    }

    // 服务单元
    if let Some(ref unit) = filters.unit {
        if !unit.is_empty() {
            args.push("-u".into());
            args.push(unit.clone());
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
            args.push(since.clone());
        }
    }
    if let Some(ref until) = filters.until {
        if !until.is_empty() {
            args.push("-U".into());
            args.push(until.clone());
        }
    }

    // 关键字搜索
    if let Some(ref keyword) = filters.keyword {
        if !keyword.is_empty() {
            args.push("--grep=".to_string() + keyword);
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
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut map = ACTIVE_STREAMS.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&session_id) {
            return Err("Journald 实时追踪已在运行中".to_string());
        }
        map.insert(session_id.clone(), cancel.clone());
    }

    // 2. 打开 exec 通道（失败时回滚注册表）
    let mut channel = match session.channel_open_session().await {
        Ok(ch) => ch,
        Err(e) => {
            if let Ok(mut map) = ACTIVE_STREAMS.lock() {
                map.remove(&session_id);
            }
            return Err(format!("打开 SSH exec 通道失败: {}", e));
        }
    };

    let args = build_journalctl_args(filters, 0, true);
    let cmd = build_command("journalctl", &args);

    log::info!("[journald:{}] 启动实时追踪: {}", session_id, cmd);

    if let Err(e) = channel.exec(true, cmd.as_str()).await {
        if let Ok(mut map) = ACTIVE_STREAMS.lock() {
            map.remove(&session_id);
        }
        return Err(format!("执行 journalctl -f 失败: {}", e));
    }

    // 4. spawn tokio task 循环读取 stdout
    let sid = session_id.clone();
    tokio::spawn(async move {
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
                                let line_bytes = line_buf[..newline_pos].to_vec();
                                line_buf = line_buf[newline_pos + 1..].to_vec();

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

        // 清理注册表（即使 mutex 被污染也尝试恢复数据）
        match ACTIVE_STREAMS.lock() {
            Ok(mut map) => {
                map.remove(&sid);
            }
            Err(poisoned) => {
                log::error!("[journald:{}] ACTIVE_STREAMS mutex 已污染，尝试恢复", sid);
                let mut map = poisoned.into_inner();
                map.remove(&sid);
            }
        }
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
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Ok(entry) = serde_json::from_value::<JournalEntry>(json_val) {
            let _ = app_handle.emit("journald:entry", serde_json::json!({
                "session_id": session_id,
                "entry": entry,
            }));
        }
    }
}

/// 停止 journald 实时追踪
///
/// 设置对应 session 的 cancel 标志为 true。
/// 即使 session_id 不在注册表中也不会报错（幂等操作）。
pub fn stop_journald_stream(session_id: &str) {
    match ACTIVE_STREAMS.lock() {
        Ok(map) => {
            if let Some(cancel) = map.get(session_id) {
                cancel.store(true, Ordering::SeqCst);
                log::info!("[journald:{}] 发送取消信号", session_id);
            }
        }
        Err(poisoned) => {
            log::error!("[journald:{}] ACTIVE_STREAMS mutex 已污染，尝试恢复", session_id);
            let map = poisoned.into_inner();
            if let Some(cancel) = map.get(session_id) {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
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

    // 游标分页
    if let Some(c) = cursor {
        if !c.is_empty() {
            args.push(format!("--after-cursor={}", c));
        }
    }

    let cmd = build_command("journalctl", &args);
    log::info!("[journald:query] {}", cmd);

    // 打开 exec 通道
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SSH exec 通道失败: {}", e))?;

    channel
        .exec(true, cmd.as_str())
        .await
        .map_err(|e| {
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
                    e, trimmed
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
/// 每个参数值通过 [`shell_escape`] 做单引号转义后再用空格拼接，
/// 防止命令注入。
fn build_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        let escaped: Vec<String> = args.iter().map(|a| shell_escape(a)).collect();
        format!("{} {}", command, escaped.join(" "))
    }
}
