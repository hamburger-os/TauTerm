//! SSH 文件服务模块（SFTP，基于 russh-sftp）
//!
//! 提供 SFTP 远程文件浏览和传输功能。
//! 所有函数为 async，与 russh 异步 I/O 模型一致。
//! russh Handle 内部线程安全，SFTP 操作与终端 I/O 可安全并发。
//!
//! SCP 已移除（用户决策：全面迁移到 russh，不保留 SCP）。

use serde::Serialize;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use russh_sftp::protocol::OpenFlags;

use crate::kernel::file_transfer::OverwritePolicy;
use crate::plugins::ssh::handler::SshHandler;

/// 传输缓冲区大小（256 KB — SSH 通道窗口约 2MB，256KB 在高 RTT 链路下能填满窗口）
const TRANSFER_BUF_SIZE: usize = 256 * 1024;

/// 进度回调最小间隔（毫秒）—— 降低 Tauri IPC + React 重渲染开销
const PROGRESS_THROTTLE_MS: u64 = 100;

/// 进度回调最小百分比增量 —— 确保即使大文件也有规律的 UI 更新
const PROGRESS_THROTTLE_PERCENT: u64 = 1;

/// 进度回调节流器
///
/// 组合时间（100ms）+ 百分比（1%）策略，避免高频 IPC 事件。
/// 传输完成时（done == total）应强制 emit。
struct ProgressThrottle {
    last_emit: Instant,
    last_percent: u64,
    last_done: u64,
}

impl ProgressThrottle {
    fn new() -> Self {
        Self {
            last_emit: Instant::now(),
            last_percent: 0,
            last_done: 0,
        }
    }

    /// 返回 true 表示应该 emit 进度事件。
    ///
    /// 每次真正 emit 时同步记录 `last_done`，用于尾部补样本去重；
    /// 最后一块若已经触发 100%，循环结束后不会再重复发送同一个 100% 样本。
    fn should_emit(&mut self, done: u64, total: u64) -> bool {
        if total == 0 {
            return false;
        }
        let percent = (done * 100) / total;
        let elapsed = self.last_emit.elapsed().as_millis() as u64;

        let should_emit = done >= total
            || elapsed >= PROGRESS_THROTTLE_MS
            || percent.saturating_sub(self.last_percent) >= PROGRESS_THROTTLE_PERCENT;

        if should_emit {
            self.last_emit = Instant::now();
            self.last_percent = percent;
            self.last_done = done;
        }
        should_emit
    }

    /// 仅当最后已传输字节数尚未发送时补一个尾部样本。
    fn should_emit_final(&mut self, done: u64, total: u64) -> bool {
        if self.last_done == done {
            return false;
        }
        self.last_emit = Instant::now();
        self.last_percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
        self.last_done = done;
        true
    }
}

struct TransferRateEstimator {
    started_at: Instant,
    last_sample_at: Instant,
    last_bytes: u64,
    ema_bytes_per_second: Option<f64>,
}

impl TransferRateEstimator {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            started_at: now,
            last_sample_at: now,
            last_bytes: 0,
            ema_bytes_per_second: None,
        }
    }

    /// 在真实 SFTP I/O 层按高精度 Instant 计算速率。
    ///
    /// 首个样本使用从传输开始到当前的平均速率，使单块/小文件也能得到有效值；
    /// 后续样本使用 τ=0.5s 的时间加权 EMA。无字节增量时不制造 0 B/s 样本。
    fn sample(&mut self, bytes_done: u64) -> Option<f64> {
        if bytes_done <= self.last_bytes {
            return self.ema_bytes_per_second;
        }

        let now = Instant::now();
        let delta_bytes = bytes_done - self.last_bytes;
        let delta_seconds = now.duration_since(self.last_sample_at).as_secs_f64();
        let total_seconds = now.duration_since(self.started_at).as_secs_f64();

        let instant = if self.last_bytes == 0 {
            if total_seconds > 0.0 {
                bytes_done as f64 / total_seconds
            } else {
                0.0
            }
        } else if delta_seconds > 0.0 {
            delta_bytes as f64 / delta_seconds
        } else {
            0.0
        };

        if instant.is_finite() && instant > 0.0 {
            let next = match self.ema_bytes_per_second {
                Some(previous) if delta_seconds > 0.0 => {
                    let alpha = 1.0 - (-delta_seconds / 0.5).exp();
                    instant * alpha + previous * (1.0 - alpha)
                }
                _ => instant,
            };
            self.ema_bytes_per_second = Some(next);
        }

        self.last_sample_at = now;
        self.last_bytes = bytes_done;
        self.ema_bytes_per_second
    }
}

/// 传输被用户取消的错误
pub fn transfer_cancelled_error() -> String {
    "传输已被用户取消".to_string()
}

/// 检查取消标志，若已取消则返回 true
fn is_cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false)
}

/// 获取或创建缓存的 SFTP 对象（仅在首次调用时打开子系统通道）
///
/// 后续调用直接复用缓存的 SftpSession，避免每次操作都进行 SSH 通道协商（节省 100-200ms × RTT）。
///
/// russh-sftp 要求先 `channel.request_subsystem(true, "sftp")` 激活 SFTP 子系统，
/// 再用 `SftpSession::new(channel)` 初始化协议握手。
async fn get_or_create_sftp(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
) -> Result<(), String> {
    let mut cache = sftp_cache.lock().await;
    if cache.is_none() {
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| format!("打开 SFTP 通道失败: {}", e))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("请求 SFTP 子系统失败: {}", e))?;
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| format!("初始化 SFTP 会话失败: {}", e))?;
        *cache = Some(sftp);
        log::info!("SFTP 子系统通道已建立并缓存");
    }
    Ok(())
}

/// SFTP 目录项类型。默认不跟随符号链接，避免目录遍历意外穿出用户选中的树。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpEntryType {
    File,
    Directory,
    Symlink,
    Fifo,
    Socket,
    BlockDevice,
    CharDevice,
    Other,
}

fn entry_type_from_permissions(perm: Option<u32>, fallback_is_dir: bool) -> SftpEntryType {
    match perm.map(|p| p & 0o170000) {
        Some(0o040000) => SftpEntryType::Directory,
        Some(0o100000) => SftpEntryType::File,
        Some(0o120000) => SftpEntryType::Symlink,
        Some(0o010000) => SftpEntryType::Fifo,
        Some(0o140000) => SftpEntryType::Socket,
        Some(0o060000) => SftpEntryType::BlockDevice,
        Some(0o020000) => SftpEntryType::CharDevice,
        Some(0) | None if fallback_is_dir => SftpEntryType::Directory,
        Some(0) | None => SftpEntryType::File,
        _ => SftpEntryType::Other,
    }
}

/// SFTP 目录项
#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    /// 文件/目录名（不含路径）
    pub name: String,
    /// 完整路径
    pub path: String,
    /// 是否为目录（兼容现有前端；新代码应优先使用 entry_type）
    pub is_dir: bool,
    pub entry_type: SftpEntryType,
    /// 文件大小（字节），目录为 0
    pub size: u64,
    /// 访问时间（Unix 时间戳，秒）
    pub accessed: Option<u64>,
    /// 修改时间（Unix 时间戳，秒）
    pub modified: Option<u64>,
    /// 权限字符串（如 "-rw-r--r--"）
    pub permissions: Option<String>,
}

/// SFTP 文件信息（stat 结果）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpFileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub entry_type: SftpEntryType,
    pub size: u64,
    /// 访问时间（Unix 时间戳，秒）
    pub accessed: Option<u64>,
    /// 修改时间（Unix 时间戳，秒）
    pub modified: Option<u64>,
    pub permissions: Option<String>,
}

/// 列出远程目录内容
pub async fn sftp_list_dir(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<Vec<SftpEntry>, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let path = if remote_path.is_empty() {
        "."
    } else {
        remote_path
    };
    let read_dir = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("读取目录 '{}' 失败: {}", path, e))?;

    let mut result: Vec<SftpEntry> = read_dir
        .into_iter()
        .filter_map(|entry| {
            let name = entry.file_name();
            if name == "." || name == ".." {
                return None;
            }
            let full_path = if path.ends_with('/') {
                format!("{}{}", path, name)
            } else {
                format!("{}/{}", path, name)
            };
            let meta = entry.metadata();
            let entry_type = entry_type_from_permissions(meta.permissions, meta.is_dir());
            let perm_str = permissions_to_string(meta.permissions);
            Some(SftpEntry {
                name,
                path: full_path,
                is_dir: entry_type == SftpEntryType::Directory,
                entry_type,
                size: meta.size.unwrap_or(0),
                accessed: meta.atime.map(|t| t as u64),
                modified: meta.mtime.map(|t| t as u64),
                permissions: Some(perm_str),
            })
        })
        .collect();

    result.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(result)
}

/// 获取远程文件信息（lstat 语义，不跟随符号链接）
pub async fn sftp_stat(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<SftpFileInfo, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let stat = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;

    let name = std::path::Path::new(remote_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| remote_path.to_string());
    let entry_type = entry_type_from_permissions(stat.permissions, stat.is_dir());

    Ok(SftpFileInfo {
        name,
        path: remote_path.to_string(),
        is_dir: entry_type == SftpEntryType::Directory,
        entry_type,
        size: stat.size.unwrap_or(0),
        accessed: stat.atime.map(|t| t as u64),
        modified: stat.mtime.map(|t| t as u64),
        permissions: Some(permissions_to_string(stat.permissions)),
    })
}

/// 读取文件头 N 字节（用于预览），返回 (数据, 文件总大小)
pub async fn sftp_read_head(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, u64), String> {
    get_or_create_sftp(session, sftp_cache).await?;

    // 只在 lstat + open 期间持缓存锁，真实读取期间释放，避免预览阻塞目录浏览/stat。
    let (mut remote_file, total_size) = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        let stat = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
        let entry_type = entry_type_from_permissions(stat.permissions, stat.is_dir());
        if entry_type != SftpEntryType::File {
            return Err(format!("仅支持预览普通文件，当前类型为 {:?}", entry_type));
        }
        let file = sftp
            .open(remote_path)
            .await
            .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?;
        (file, stat.size.unwrap_or(0))
    };

    let read_len = std::cmp::min(max_bytes, total_size);
    let mut buf = vec![0u8; read_len as usize];
    let mut total_read: u64 = 0;

    while total_read < read_len {
        let remaining = (read_len - total_read) as usize;
        let start = total_read as usize;
        let n = remote_file
            .read(&mut buf[start..start + remaining])
            .await
            .map_err(|e| format!("读取远程文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        total_read += n as u64;
    }

    buf.truncate(total_read as usize);
    log::info!(
        "SFTP 读取文件头: {} (读取 {} / 共 {} bytes)",
        remote_path,
        total_read,
        total_size
    );
    Ok((buf, total_size))
}

#[derive(Debug)]
pub enum SftpWriteOutcome {
    Completed { bytes: u64, final_path: String },
    Skipped { final_path: String },
}

fn sibling_local_artifact(path: &Path, tag: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "transfer".to_string());
    parent.join(format!(
        ".{}.tauterm-{}-{}",
        name,
        tag,
        uuid::Uuid::new_v4()
    ))
}

fn local_keep_both_candidate(path: &Path, index: u32) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    let name = match ext {
        Some(ext) if !ext.is_empty() => format!("{} ({}).{}", stem, index, ext),
        _ => format!("{} ({})", stem, index),
    };
    parent.join(name)
}

async fn resolve_local_destination(
    requested: &Path,
    policy: OverwritePolicy,
) -> Result<Option<PathBuf>, String> {
    match tokio::fs::symlink_metadata(requested).await {
        Ok(meta) => match policy {
            OverwritePolicy::Replace if meta.is_dir() => {
                Err(format!("下载目标已存在且是目录: {}", requested.display()))
            }
            OverwritePolicy::Replace | OverwritePolicy::KeepBoth => {
                Ok(Some(requested.to_path_buf()))
            }
            OverwritePolicy::Skip => Ok(None),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Some(requested.to_path_buf())),
        Err(e) => Err(format!(
            "检查下载目标 '{}' 失败: {}",
            requested.display(),
            e
        )),
    }
}

/// 同目录临时文件通过 hard-link 原子抢占最终名字，避免 Unix rename 的覆盖语义
/// 破坏 KeepBoth/Skip。返回 false 表示目标名已被其他对象占用。
async fn try_commit_local_noreplace(temp: &Path, candidate: &Path) -> Result<bool, String> {
    match tokio::fs::hard_link(temp, candidate).await {
        Ok(()) => {
            if let Err(e) = tokio::fs::remove_file(temp).await {
                log::warn!(
                    "本地无覆盖提交成功，但清理临时路径 '{}' 失败: {}",
                    temp.display(),
                    e
                );
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(format!(
            "无覆盖提交下载文件 '{}' 失败: {}",
            candidate.display(),
            e
        )),
    }
}

async fn commit_local_temp(
    temp: &Path,
    final_path: &Path,
    policy: OverwritePolicy,
) -> Result<Option<PathBuf>, String> {
    match policy {
        OverwritePolicy::Skip => {
            if try_commit_local_noreplace(temp, final_path).await? {
                Ok(Some(final_path.to_path_buf()))
            } else {
                Ok(None)
            }
        }
        OverwritePolicy::KeepBoth => {
            for index in 0..=9999 {
                let candidate = if index == 0 {
                    final_path.to_path_buf()
                } else {
                    local_keep_both_candidate(final_path, index)
                };
                if try_commit_local_noreplace(temp, &candidate).await? {
                    return Ok(Some(candidate));
                }
            }
            Err(format!(
                "无法为 '{}' 生成不冲突文件名",
                final_path.display()
            ))
        }
        OverwritePolicy::Replace => match tokio::fs::symlink_metadata(final_path).await {
            Ok(meta) if meta.is_dir() => {
                Err(format!("下载目标已存在且是目录: {}", final_path.display()))
            }
            Ok(_) => {
                let backup = sibling_local_artifact(final_path, "backup");
                tokio::fs::rename(final_path, &backup)
                    .await
                    .map_err(|e| format!("备份原文件 '{}' 失败: {}", final_path.display(), e))?;
                match tokio::fs::rename(temp, final_path).await {
                    Ok(()) => {
                        if let Err(e) = tokio::fs::remove_file(&backup).await {
                            log::warn!("删除本地提交备份 '{}' 失败: {}", backup.display(), e);
                        }
                        Ok(Some(final_path.to_path_buf()))
                    }
                    Err(commit_error) => match tokio::fs::rename(&backup, final_path).await {
                        Ok(()) => Err(format!(
                            "提交下载文件 '{}' 失败: {}",
                            final_path.display(),
                            commit_error
                        )),
                        Err(rollback_error) => Err(format!(
                            "提交下载文件 '{}' 失败: {}；回滚也失败，原文件仍保留在 '{}': {}",
                            final_path.display(),
                            commit_error,
                            backup.display(),
                            rollback_error
                        )),
                    },
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::rename(temp, final_path)
                    .await
                    .map(|_| Some(final_path.to_path_buf()))
                    .map_err(|e| format!("提交下载文件 '{}' 失败: {}", final_path.display(), e))
            }
            Err(e) => Err(format!(
                "检查下载目标 '{}' 失败: {}",
                final_path.display(),
                e
            )),
        },
    }
}

fn remote_parts(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

fn remote_join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else if dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

fn remote_keep_both_candidate(path: &str, index: u32) -> String {
    let (dir, name) = remote_parts(path);
    let dot = name.rfind('.').filter(|idx| *idx > 0);
    let next = match dot {
        Some(idx) => format!("{} ({}).{}", &name[..idx], index, &name[idx + 1..]),
        None => format!("{} ({})", name, index),
    };
    remote_join(dir, &next)
}

fn remote_sibling_artifact(path: &str, tag: &str) -> String {
    let (dir, name) = remote_parts(path);
    remote_join(
        dir,
        &format!(".{}.tauterm-{}-{}", name, tag, uuid::Uuid::new_v4()),
    )
}

async fn resolve_remote_destination(
    sftp: &russh_sftp::client::SftpSession,
    requested: &str,
    policy: OverwritePolicy,
) -> Result<Option<String>, String> {
    let exists = sftp
        .try_exists(requested)
        .await
        .map_err(|e| format!("检查远程目标 '{}' 失败: {}", requested, e))?;
    if !exists {
        return Ok(Some(requested.to_string()));
    }

    match policy {
        OverwritePolicy::Skip => Ok(None),
        OverwritePolicy::KeepBoth => Ok(Some(requested.to_string())),
        OverwritePolicy::Replace => {
            let meta = sftp
                .symlink_metadata(requested)
                .await
                .map_err(|e| format!("获取远程目标 '{}' 信息失败: {}", requested, e))?;
            if entry_type_from_permissions(meta.permissions, meta.is_dir())
                == SftpEntryType::Directory
            {
                Err(format!("远程目标已存在且是目录: {}", requested))
            } else {
                Ok(Some(requested.to_string()))
            }
        }
    }
}

/// SFTP v3 的普通 RENAME 在目标已存在时应失败；这里在失败后重新检查目标，
/// 将并发抢占识别为“名字已被占用”，供 KeepBoth 继续尝试下一个候选名。
async fn try_commit_remote_noreplace(
    sftp: &russh_sftp::client::SftpSession,
    temp: &str,
    candidate: &str,
) -> Result<bool, String> {
    if sftp
        .try_exists(candidate)
        .await
        .map_err(|e| format!("检查远程目标 '{}' 失败: {}", candidate, e))?
    {
        return Ok(false);
    }

    match sftp.rename(temp, candidate).await {
        Ok(()) => Ok(true),
        Err(rename_error) => {
            let now_exists = sftp
                .try_exists(candidate)
                .await
                .map_err(|e| format!("重新检查远程目标 '{}' 失败: {}", candidate, e))?;
            if now_exists {
                Ok(false)
            } else {
                Err(format!(
                    "无覆盖提交远程文件 '{}' 失败: {}",
                    candidate, rename_error
                ))
            }
        }
    }
}

async fn commit_remote_temp(
    sftp: &russh_sftp::client::SftpSession,
    temp: &str,
    final_path: &str,
    policy: OverwritePolicy,
) -> Result<Option<String>, String> {
    match policy {
        OverwritePolicy::Skip => {
            if try_commit_remote_noreplace(sftp, temp, final_path).await? {
                Ok(Some(final_path.to_string()))
            } else {
                Ok(None)
            }
        }
        OverwritePolicy::KeepBoth => {
            for index in 0..=9999 {
                let candidate = if index == 0 {
                    final_path.to_string()
                } else {
                    remote_keep_both_candidate(final_path, index)
                };
                if try_commit_remote_noreplace(sftp, temp, &candidate).await? {
                    return Ok(Some(candidate));
                }
            }
            Err(format!("无法为 '{}' 生成不冲突文件名", final_path))
        }
        OverwritePolicy::Replace => {
            let exists = sftp
                .try_exists(final_path)
                .await
                .map_err(|e| format!("检查远程目标 '{}' 失败: {}", final_path, e))?;
            if !exists {
                return sftp
                    .rename(temp, final_path)
                    .await
                    .map(|_| Some(final_path.to_string()))
                    .map_err(|e| format!("提交远程文件 '{}' 失败: {}", final_path, e));
            }

            let meta = sftp
                .symlink_metadata(final_path)
                .await
                .map_err(|e| format!("获取远程目标 '{}' 信息失败: {}", final_path, e))?;
            if entry_type_from_permissions(meta.permissions, meta.is_dir())
                == SftpEntryType::Directory
            {
                return Err(format!("远程目标已存在且是目录: {}", final_path));
            }

            let backup = remote_sibling_artifact(final_path, "backup");
            sftp.rename(final_path, &backup)
                .await
                .map_err(|e| format!("备份远程原文件 '{}' 失败: {}", final_path, e))?;
            match sftp.rename(temp, final_path).await {
                Ok(()) => {
                    if let Err(e) = sftp.remove_file(&backup).await {
                        log::warn!("删除远程提交备份 '{}' 失败: {}", backup, e);
                    }
                    Ok(Some(final_path.to_string()))
                }
                Err(commit_error) => match sftp.rename(&backup, final_path).await {
                    Ok(()) => Err(format!(
                        "提交远程文件 '{}' 失败: {}",
                        final_path, commit_error
                    )),
                    Err(rollback_error) => Err(format!(
                        "提交远程文件 '{}' 失败: {}；回滚也失败，原文件仍保留在 '{}': {}",
                        final_path, commit_error, backup, rollback_error
                    )),
                },
            }
        }
    }
}

/// 下载远程文件到本地。
///
/// 始终先写同目录临时文件，只有完整写入、flush 与取消窗口结束后才提交到正式路径。
pub async fn sftp_download(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
    local_path: &str,
    overwrite_policy: OverwritePolicy,
    on_progress: Option<&(dyn Fn(u64, u64, Option<f64>) + Send + Sync)>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<SftpWriteOutcome, String> {
    get_or_create_sftp(session, sftp_cache).await?;

    let requested = Path::new(local_path);
    let Some(final_path) = resolve_local_destination(requested, overwrite_policy).await? else {
        return Ok(SftpWriteOutcome::Skipped {
            final_path: requested.to_string_lossy().to_string(),
        });
    };

    let (mut remote_file, remote_size) = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        let meta = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取远程文件信息 '{}' 失败: {}", remote_path, e))?;
        let entry_type = entry_type_from_permissions(meta.permissions, meta.is_dir());
        if entry_type != SftpEntryType::File {
            return Err(format!(
                "仅支持下载普通文件，'{}' 类型为 {:?}",
                remote_path, entry_type
            ));
        }
        let file = sftp
            .open(remote_path)
            .await
            .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?;
        (file, meta.size.unwrap_or(0))
    };

    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建本地目录 '{}' 失败: {}", parent.display(), e))?;
    }

    let temp_path = sibling_local_artifact(&final_path, "part");
    let mut local_file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("创建本地临时文件 '{}' 失败: {}", temp_path.display(), e))?;

    let mut buf = [0u8; TRANSFER_BUF_SIZE];
    let mut total: u64 = 0;
    let mut throttle = ProgressThrottle::new();
    let mut rate = TransferRateEstimator::new();

    loop {
        if is_cancelled(cancel) {
            drop(local_file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(transfer_cancelled_error());
        }
        let n = match remote_file.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                drop(local_file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(format!("读取远程文件失败: {}", e));
            }
        };
        if n == 0 {
            break;
        }
        if let Err(e) = local_file.write_all(&buf[..n]).await {
            drop(local_file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(format!("写入本地临时文件失败: {}", e));
        }
        total += n as u64;
        if let Some(cb) = on_progress {
            if throttle.should_emit(total, remote_size) {
                cb(total, remote_size, rate.sample(total));
            }
        }
    }

    if let Some(cb) = on_progress {
        if throttle.should_emit_final(total, remote_size) {
            cb(total, remote_size, rate.sample(total));
        }
    }
    if let Err(e) = local_file.flush().await {
        drop(local_file);
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!("刷新本地临时文件失败: {}", e));
    }
    drop(local_file);

    if is_cancelled(cancel) {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(transfer_cancelled_error());
    }

    let committed_path = match commit_local_temp(&temp_path, &final_path, overwrite_policy).await {
        Ok(Some(path)) => path,
        Ok(None) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Ok(SftpWriteOutcome::Skipped {
                final_path: final_path.to_string_lossy().to_string(),
            });
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(e);
        }
    };

    log::info!(
        "SFTP 下载完成: {} -> {} ({} bytes, remote_size={})",
        remote_path,
        committed_path.display(),
        total,
        remote_size
    );
    Ok(SftpWriteOutcome::Completed {
        bytes: total,
        final_path: committed_path.to_string_lossy().to_string(),
    })
}

#[derive(Debug, Clone, Copy)]
pub struct SftpUploadOptions {
    pub mtime: Option<u64>,
    pub overwrite_policy: OverwritePolicy,
}

/// 上传本地文件到远程。
///
/// 写入远端同目录临时文件，flush/mtime 完成后才通过 rename 提交；Replace 时会先
/// 备份旧目标并在提交失败时回滚，取消/失败绝不删除原有正式文件。
pub async fn sftp_upload(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    local_path: &str,
    remote_path: &str,
    options: SftpUploadOptions,
    on_progress: Option<&(dyn Fn(u64, u64, Option<f64>) + Send + Sync)>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<SftpWriteOutcome, String> {
    let SftpUploadOptions {
        mtime,
        overwrite_policy,
    } = options;
    get_or_create_sftp(session, sftp_cache).await?;

    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("打开本地文件 '{}' 失败: {}", local_path, e))?;
    let local_size = local_file.metadata().await.map(|m| m.len()).unwrap_or(0);

    let (final_path, temp_path, mut remote_file) = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        let Some(final_path) =
            resolve_remote_destination(sftp, remote_path, overwrite_policy).await?
        else {
            return Ok(SftpWriteOutcome::Skipped {
                final_path: remote_path.to_string(),
            });
        };
        let temp_path = remote_sibling_artifact(&final_path, "part");
        let file = sftp
            .open_with_flags(
                &temp_path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
            )
            .await
            .map_err(|e| format!("创建远程临时文件 '{}' 失败: {}", temp_path, e))?;
        (final_path, temp_path, file)
    };

    let mut buf = [0u8; TRANSFER_BUF_SIZE];
    let mut total: u64 = 0;
    let mut throttle = ProgressThrottle::new();
    let mut rate = TransferRateEstimator::new();

    loop {
        if is_cancelled(cancel) {
            remote_file.flush().await.ok();
            drop(remote_file);
            let cache = sftp_cache.lock().await;
            if let Some(sftp) = cache.as_ref() {
                let _ = sftp.remove_file(&temp_path).await;
            }
            return Err(transfer_cancelled_error());
        }
        let n = match local_file.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                drop(remote_file);
                let cache = sftp_cache.lock().await;
                if let Some(sftp) = cache.as_ref() {
                    let _ = sftp.remove_file(&temp_path).await;
                }
                return Err(format!("读取本地文件失败: {}", e));
            }
        };
        if n == 0 {
            break;
        }
        if let Err(e) = remote_file.write_all(&buf[..n]).await {
            drop(remote_file);
            let cache = sftp_cache.lock().await;
            if let Some(sftp) = cache.as_ref() {
                let _ = sftp.remove_file(&temp_path).await;
            }
            return Err(format!("写入远程临时文件失败: {}", e));
        }
        total += n as u64;
        if let Some(cb) = on_progress {
            if throttle.should_emit(total, local_size) {
                cb(total, local_size, rate.sample(total));
            }
        }
    }

    if let Some(cb) = on_progress {
        if throttle.should_emit_final(total, local_size) {
            cb(total, local_size, rate.sample(total));
        }
    }
    if let Err(e) = remote_file.flush().await {
        drop(remote_file);
        let cache = sftp_cache.lock().await;
        if let Some(sftp) = cache.as_ref() {
            let _ = sftp.remove_file(&temp_path).await;
        }
        return Err(format!("刷新远程临时文件失败: {}", e));
    }
    drop(remote_file);

    // 时间戳同步是 best-effort：写在临时文件上，因此即使服务器不支持也不会
    // 破坏正式目标；失败会记录日志但不把完整数据传输降级为失败。
    if let Some(mtime_secs) = mtime {
        let cache = sftp_cache.lock().await;
        if let Some(sftp) = cache.as_ref() {
            match sftp.metadata(&temp_path).await {
                Ok(mut stat) => {
                    let ts = mtime_secs.min(u32::MAX as u64) as u32;
                    stat.mtime = Some(ts);
                    stat.atime = Some(ts);
                    if let Err(e) = sftp.set_metadata(&temp_path, stat).await {
                        log::warn!("同步远程临时文件时间戳 '{}' 失败: {}", temp_path, e);
                    }
                }
                Err(e) => {
                    log::warn!("读取远程临时文件元数据 '{}' 失败: {}", temp_path, e);
                }
            }
        }
    }

    if is_cancelled(cancel) {
        let cache = sftp_cache.lock().await;
        if let Some(sftp) = cache.as_ref() {
            let _ = sftp.remove_file(&temp_path).await;
        }
        return Err(transfer_cancelled_error());
    }

    let commit_result = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        commit_remote_temp(sftp, &temp_path, &final_path, overwrite_policy).await
    };
    let committed_path = match commit_result {
        Ok(Some(path)) => path,
        Ok(None) => {
            let cache = sftp_cache.lock().await;
            if let Some(sftp) = cache.as_ref() {
                let _ = sftp.remove_file(&temp_path).await;
            }
            return Ok(SftpWriteOutcome::Skipped { final_path });
        }
        Err(e) => {
            let cache = sftp_cache.lock().await;
            if let Some(sftp) = cache.as_ref() {
                let _ = sftp.remove_file(&temp_path).await;
            }
            return Err(e);
        }
    };

    log::info!(
        "SFTP 上传完成: {} -> {} ({} bytes, local_size={})",
        local_path,
        committed_path,
        total,
        local_size
    );
    Ok(SftpWriteOutcome::Completed {
        bytes: total,
        final_path: committed_path,
    })
}

/// 删除远程文件
pub async fn sftp_delete(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let stat = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;

    if stat.is_dir() {
        // rmdir 要求目录为空
        match sftp.remove_dir(remote_path).await {
            Ok(()) => {
                log::info!("SFTP 已删除目录: {}", remote_path);
            }
            Err(e) => {
                return Err(format!(
                    "删除目录 '{}' 失败（可能非空）: {}",
                    remote_path, e
                ));
            }
        }
    } else {
        sftp.remove_file(remote_path)
            .await
            .map_err(|e| format!("删除文件 '{}' 失败: {}", remote_path, e))?;
        log::info!("SFTP 已删除文件: {}", remote_path);
    }

    Ok(())
}

/// 重命名/移动远程文件
pub async fn sftp_rename(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    from_path: &str,
    to_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    sftp.rename(from_path, to_path)
        .await
        .map_err(|e| format!("重命名 '{}' -> '{}' 失败: {}", from_path, to_path, e))?;

    log::info!("SFTP 重命名: {} -> {}", from_path, to_path);
    Ok(())
}

/// 创建远程目录
pub async fn sftp_mkdir(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    sftp.create_dir(remote_path)
        .await
        .map_err(|e| format!("创建目录 '{}' 失败: {}", remote_path, e))?;

    log::info!("SFTP 已创建目录: {}", remote_path);
    Ok(())
}

async fn try_create_remote_directory(
    sftp: &russh_sftp::client::SftpSession,
    candidate: &str,
) -> Result<bool, String> {
    if sftp
        .try_exists(candidate)
        .await
        .map_err(|e| format!("检查远程目录 '{}' 失败: {}", candidate, e))?
    {
        return Ok(false);
    }

    match sftp.create_dir(candidate).await {
        Ok(()) => Ok(true),
        Err(create_error) => {
            let now_exists = sftp
                .try_exists(candidate)
                .await
                .map_err(|e| format!("重新检查远程目录 '{}' 失败: {}", candidate, e))?;
            if now_exists {
                Ok(false)
            } else {
                Err(format!(
                    "创建远程目录 '{}' 失败: {}",
                    candidate, create_error
                ))
            }
        }
    }
}

/// 为目录上传原子保留远端根目录。
///
/// 目录 Replace 不做隐式递归覆盖：若目标已经存在则明确失败；KeepBoth 使用
/// create_dir 的排他创建语义挑选一个不冲突名字；Skip 在冲突时跳过整棵目录。
pub async fn sftp_prepare_upload_directory(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    requested: &str,
    policy: OverwritePolicy,
) -> Result<Option<String>, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    match policy {
        OverwritePolicy::Skip => {
            if try_create_remote_directory(sftp, requested).await? {
                Ok(Some(requested.to_string()))
            } else {
                Ok(None)
            }
        }
        OverwritePolicy::KeepBoth => {
            for index in 0..=9999 {
                let candidate = if index == 0 {
                    requested.to_string()
                } else {
                    let (dir, name) = remote_parts(requested);
                    remote_join(dir, &format!("{} ({})", name, index))
                };
                if try_create_remote_directory(sftp, &candidate).await? {
                    return Ok(Some(candidate));
                }
            }
            Err(format!("无法为远程目录 '{}' 生成不冲突名称", requested))
        }
        OverwritePolicy::Replace => {
            if sftp
                .try_exists(requested)
                .await
                .map_err(|e| format!("检查远程目录 '{}' 失败: {}", requested, e))?
            {
                return Err(format!(
                    "远程目录 '{}' 已存在；目录替换不会自动合并或递归覆盖，请选择保留两份或跳过",
                    requested
                ));
            }
            if try_create_remote_directory(sftp, requested).await? {
                Ok(Some(requested.to_string()))
            } else {
                Err(format!("远程目录 '{}' 在提交时被其他操作占用", requested))
            }
        }
    }
}

/// 确保远端子目录存在。已有对象必须确实是目录；不会跟随符号链接。
pub async fn sftp_ensure_directory(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    if sftp
        .try_exists(remote_path)
        .await
        .map_err(|e| format!("检查远程目录 '{}' 失败: {}", remote_path, e))?
    {
        let meta = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取远程目录 '{}' 信息失败: {}", remote_path, e))?;
        if entry_type_from_permissions(meta.permissions, meta.is_dir()) == SftpEntryType::Directory
        {
            return Ok(());
        }
        return Err(format!("远程路径 '{}' 已存在且不是目录", remote_path));
    }

    if try_create_remote_directory(sftp, remote_path).await? {
        Ok(())
    } else {
        let meta = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取远程目录 '{}' 信息失败: {}", remote_path, e))?;
        if entry_type_from_permissions(meta.permissions, meta.is_dir()) == SftpEntryType::Directory
        {
            Ok(())
        } else {
            Err(format!("远程路径 '{}' 被非目录对象占用", remote_path))
        }
    }
}

/// 创建空文件（touch）
pub async fn sftp_new_file(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let mut file = sftp
        .open_with_flags(
            remote_path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
        )
        .await
        .map_err(|e| format!("创建文件 '{}' 失败（目标可能已存在）: {}", remote_path, e))?;
    file.flush()
        .await
        .map_err(|e| format!("刷新文件 '{}' 失败: {}", remote_path, e))?;

    log::info!("SFTP 已创建空文件: {}", remote_path);
    Ok(())
}

/// 修改远程文件权限（通过 set_metadata 实现）
pub async fn sftp_chmod(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
    mode: u32,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let mut stat = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
    let entry_type = entry_type_from_permissions(stat.permissions, stat.is_dir());
    if !matches!(entry_type, SftpEntryType::File | SftpEntryType::Directory) {
        return Err(format!(
            "仅支持修改普通文件或目录权限，'{}' 类型为 {:?}",
            remote_path, entry_type
        ));
    }

    // SFTP permissions 字段包含 POSIX mode；保留文件类型位，只替换权限位。
    let file_type_bits = stat.permissions.unwrap_or(0) & 0o170000;
    stat.permissions = Some(file_type_bits | (mode & 0o7777));
    sftp.set_metadata(remote_path, stat)
        .await
        .map_err(|e| format!("修改权限 '{}' 失败: {}", remote_path, e))?;

    log::info!("SFTP chmod: {} -> {:o}", remote_path, mode);
    Ok(())
}

/// 批量删除远程文件和空目录
/// 返回删除失败的项目路径列表（空列表表示全部成功）
///
/// 每次迭代独立获取/释放 sftp_cache 锁，避免长时间持锁阻塞
/// 同会话的其他 SFTP 操作（目录刷新、stat、下载等）。
pub async fn sftp_delete_batch(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    paths: &[String],
) -> Result<Vec<String>, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let mut failed: Vec<String> = Vec::new();

    for remote_path in paths {
        let result = {
            let cache = sftp_cache.lock().await;
            let sftp = match cache.as_ref() {
                Some(s) => s,
                None => {
                    failed.push(remote_path.clone());
                    continue;
                }
            };
            let stat = match sftp.symlink_metadata(remote_path).await {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("批量删除: 获取 '{}' 信息失败: {}", remote_path, e);
                    failed.push(remote_path.clone());
                    continue;
                }
            };
            if stat.is_dir() {
                sftp.remove_dir(remote_path)
                    .await
                    .map_err(|e| format!("删除目录失败: {}", e))
            } else {
                sftp.remove_file(remote_path)
                    .await
                    .map_err(|e| format!("删除文件失败: {}", e))
            }
        }; // sftp_cache 锁在此释放，允许其他 SFTP 操作穿插

        match result {
            Ok(()) => log::info!("SFTP 批量删除: {}", remote_path),
            Err(e) => {
                log::warn!("SFTP 批量删除 '{}' 失败: {}", remote_path, e);
                failed.push(remote_path.clone());
            }
        }
    }

    Ok(failed)
}

/// 递归删除远程文件或目录（包括所有子内容）
///
/// 每次递归层级独立获取/释放 `sftp_cache` 锁，避免长时间持锁阻塞
/// 同会话的其他 SFTP 操作（目录刷新、stat、下载等）。
/// 与 `sftp_delete_batch` 的逐条目释放锁策略一致。
pub async fn sftp_delete_recursive(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<(), String> {
    get_or_create_sftp(session, sftp_cache).await?;
    delete_recursive_inner(sftp_cache, remote_path, 0).await
}

type DeleteRecursiveFut<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

fn delete_recursive_inner<'a>(
    sftp_cache: &'a Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    path: &'a str,
    depth: u32,
) -> DeleteRecursiveFut<'a> {
    Box::pin(async move {
        const MAX_DEPTH: u32 = 100;
        if depth > MAX_DEPTH {
            log::warn!("SFTP 递归删除超过最大深度 {} 层，跳过: {}", MAX_DEPTH, path);
            return Err(format!(
                "递归删除超过最大深度限制 ({} 层)，路径: {}。可能为恶意嵌套或循环符号链接。",
                MAX_DEPTH, path
            ));
        }
        let (_is_dir, children) = {
            let cache = sftp_cache.lock().await;
            let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

            let stat = sftp
                .symlink_metadata(path)
                .await
                .map_err(|e| format!("获取 '{}' 信息失败: {}", path, e))?;

            if !stat.is_dir() {
                // 文件：持锁删除后释放
                sftp.remove_file(path)
                    .await
                    .map_err(|e| format!("删除文件 '{}' 失败: {}", path, e))?;
                log::info!("SFTP 递归删除文件: {}", path);
                return Ok(());
            }

            let read_dir = sftp
                .read_dir(path)
                .await
                .map_err(|e| format!("读取目录 '{}' 失败: {}", path, e))?;

            let children: Vec<String> = read_dir
                .into_iter()
                .filter_map(|entry| {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        return None;
                    }
                    Some(if path.ends_with('/') {
                        format!("{}{}", path, name)
                    } else {
                        format!("{}/{}", path, name)
                    })
                })
                .collect();

            (true, children)
        }; // 锁在此释放

        // 递归删除子条目（每层独立获取锁）
        for child_path in &children {
            delete_recursive_inner(sftp_cache, child_path, depth + 1).await?;
        }

        // 删除已清空的目录本身
        {
            let cache = sftp_cache.lock().await;
            let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
            sftp.remove_dir(path)
                .await
                .map_err(|e| format!("删除目录 '{}' 失败: {}", path, e))?;
        }
        log::info!("SFTP 递归删除目录: {}", path);
        Ok(())
    })
}

// ── 辅助函数 ───────────────────────────────────────────

/// 将 Unix 权限位转换为字符串表示（如 "-rw-r--r--"）
fn permissions_to_string(perm: Option<u32>) -> String {
    let p = perm.unwrap_or(0);
    let mut s = String::with_capacity(10);

    let type_char = match p & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o010000 => 'p',
        0o140000 => 's',
        0o060000 => 'b',
        0o020000 => 'c',
        _ => '-',
    };
    s.push(type_char);
    s.push(if p & 0o400 != 0 { 'r' } else { '-' });
    s.push(if p & 0o200 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o4000 != 0, p & 0o100 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if p & 0o040 != 0 { 'r' } else { '-' });
    s.push(if p & 0o020 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o2000 != 0, p & 0o010 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if p & 0o004 != 0 { 'r' } else { '-' });
    s.push(if p & 0o002 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o1000 != 0, p & 0o001 != 0) {
        (true, true) => 't',
        (true, false) => 'T',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s
}

// ── 递归目录列表 ────────────────────────────────────────

/// 目录树条目。递归枚举不跟随符号链接；调用方可决定是否跳过或以后实现保留链接。
#[derive(Debug, Clone)]
pub struct SftpTreeEntry {
    pub path: String,
    pub entry_type: SftpEntryType,
    pub size: u64,
}

/// 递归列出远程目录树（包含目录本身的子目录条目，因此空目录不会丢失）。
pub async fn sftp_list_tree_recursive(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_dir: &str,
) -> Result<Vec<SftpTreeEntry>, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let mut result = Vec::new();
    list_tree_recursive_inner(sftp_cache, remote_dir, 0, &mut result).await?;
    Ok(result)
}

type ListRecursiveFut<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

fn list_tree_recursive_inner<'a>(
    sftp_cache: &'a Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    path: &'a str,
    depth: u32,
    result: &'a mut Vec<SftpTreeEntry>,
) -> ListRecursiveFut<'a> {
    Box::pin(async move {
        const MAX_DEPTH: u32 = 50;
        if depth > MAX_DEPTH {
            return Err(format!(
                "SFTP 递归列表超过最大深度 {} 层: {}",
                MAX_DEPTH, path
            ));
        }

        let children: Vec<SftpTreeEntry> = {
            let cache = sftp_cache.lock().await;
            let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
            let read_dir = sftp
                .read_dir(path)
                .await
                .map_err(|e| format!("读取目录 '{}' 失败: {}", path, e))?;

            read_dir
                .into_iter()
                .filter_map(|entry| {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        return None;
                    }
                    let full_path = if path.ends_with('/') {
                        format!("{}{}", path, name)
                    } else {
                        format!("{}/{}", path, name)
                    };
                    let meta = entry.metadata();
                    let entry_type = entry_type_from_permissions(meta.permissions, meta.is_dir());
                    Some(SftpTreeEntry {
                        path: full_path,
                        entry_type,
                        size: meta.size.unwrap_or(0),
                    })
                })
                .collect()
        };

        for child in children {
            let is_dir = child.entry_type == SftpEntryType::Directory;
            let child_path = child.path.clone();
            result.push(child);
            if is_dir {
                list_tree_recursive_inner(sftp_cache, &child_path, depth + 1, result).await?;
            }
        }

        Ok(())
    })
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn completed_chunk_is_not_emitted_twice() {
        let mut throttle = ProgressThrottle::new();
        assert!(throttle.should_emit(1024, 1024));
        assert!(!throttle.should_emit_final(1024, 1024));
    }

    #[test]
    fn missing_tail_sample_is_emitted_exactly_once() {
        let mut throttle = ProgressThrottle::new();
        assert!(throttle.should_emit(512, 1024));
        assert!(throttle.should_emit_final(1024, 1024));
        assert!(!throttle.should_emit_final(1024, 1024));
    }

    #[test]
    fn rate_estimator_never_invents_zero_speed_for_no_progress() {
        let mut rate = TransferRateEstimator::new();
        assert_eq!(rate.sample(0), None);
        let sample = rate
            .sample(1024)
            .expect("first non-zero byte sample should have a rate");
        assert!(sample.is_finite());
        assert!(sample > 0.0);
        assert_eq!(rate.sample(1024), Some(sample));
    }

    #[test]
    fn permission_strings_preserve_posix_special_bits() {
        assert_eq!(permissions_to_string(Some(0o104755)), "-rwsr-xr-x");
        assert_eq!(permissions_to_string(Some(0o102755)), "-rwxr-sr-x");
        assert_eq!(permissions_to_string(Some(0o101755)), "-rwxr-xr-t");
        assert_eq!(permissions_to_string(Some(0o107000)), "---S--S--T");
    }

    #[tokio::test]
    async fn local_keep_both_never_overwrites_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("report.txt");
        let temp_path = dir.path().join(".report.part");
        tokio::fs::write(&final_path, b"old")
            .await
            .expect("write old");
        tokio::fs::write(&temp_path, b"new")
            .await
            .expect("write temp");

        let committed = commit_local_temp(&temp_path, &final_path, OverwritePolicy::KeepBoth)
            .await
            .expect("commit")
            .expect("committed path");

        assert_ne!(committed, final_path);
        assert_eq!(
            tokio::fs::read(&final_path).await.expect("read old"),
            b"old"
        );
        assert_eq!(tokio::fs::read(&committed).await.expect("read new"), b"new");
        assert!(tokio::fs::symlink_metadata(&temp_path).await.is_err());
    }

    #[tokio::test]
    async fn local_replace_commits_new_bytes_without_leaving_backup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("report.txt");
        let temp_path = dir.path().join(".report.part");
        tokio::fs::write(&final_path, b"old")
            .await
            .expect("write old");
        tokio::fs::write(&temp_path, b"new")
            .await
            .expect("write temp");

        let committed = commit_local_temp(&temp_path, &final_path, OverwritePolicy::Replace)
            .await
            .expect("commit")
            .expect("committed path");

        assert_eq!(committed, final_path);
        assert_eq!(
            tokio::fs::read(&final_path).await.expect("read final"),
            b"new"
        );
        let mut entries = tokio::fs::read_dir(dir.path()).await.expect("read dir");
        while let Some(entry) = entries.next_entry().await.expect("entry") {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.contains("tauterm-backup"), "backup leaked: {name}");
        }
    }

    #[tokio::test]
    async fn local_skip_preserves_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("report.txt");
        let temp_path = dir.path().join(".report.part");
        tokio::fs::write(&final_path, b"old")
            .await
            .expect("write old");
        tokio::fs::write(&temp_path, b"new")
            .await
            .expect("write temp");

        let committed = commit_local_temp(&temp_path, &final_path, OverwritePolicy::Skip)
            .await
            .expect("commit");
        assert!(committed.is_none());
        assert_eq!(
            tokio::fs::read(&final_path).await.expect("read final"),
            b"old"
        );
    }
}
