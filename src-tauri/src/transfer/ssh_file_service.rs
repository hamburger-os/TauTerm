//! SSH 文件服务模块（SFTP，基于 russh-sftp）
//!
//! 提供 SFTP 远程文件浏览和传输功能。
//! 所有函数为 async，与 russh 异步 I/O 模型一致。
//! russh Handle 内部线程安全，SFTP 操作与终端 I/O 可安全并发。
//!
//! SCP 已移除（用户决策：全面迁移到 russh，不保留 SCP）。

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Instant;
use std::pin::Pin;
use std::future::Future;
use tokio::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::Serialize;

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
}

impl ProgressThrottle {
    fn new() -> Self {
        Self {
            last_emit: Instant::now(),
            last_percent: 0,
        }
    }

    /// 返回 true 表示应该 emit 进度事件
    fn should_emit(&mut self, done: u64, total: u64) -> bool {
        if total == 0 {
            return false;
        }
        let percent = (done * 100) / total;
        let elapsed = self.last_emit.elapsed().as_millis() as u64;

        // 完成时强制 emit
        if done >= total {
            return true;
        }
        // 时间节流：距上次 emit 超过阈值
        if elapsed >= PROGRESS_THROTTLE_MS {
            self.last_emit = Instant::now();
            self.last_percent = percent;
            return true;
        }
        // 百分比节流：进度跳变超过阈值
        if percent.saturating_sub(self.last_percent) >= PROGRESS_THROTTLE_PERCENT {
            self.last_emit = Instant::now();
            self.last_percent = percent;
            return true;
        }
        false
    }
}

/// 传输被用户取消的错误
pub fn transfer_cancelled_error() -> String {
    "传输已被用户取消".to_string()
}

/// 判断错误是否为用户主动取消（替代字符串 contains 匹配，避免脆弱判断）
///
/// `sftp_upload` 等返回的取消错误由 `transfer_cancelled_error()` 产生，
/// 此函数提供语义化判断，调用方据此决定是否跳过远端清理（取消路径已自行清理）。
pub fn is_cancelled_error(err: &str) -> bool {
    err == transfer_cancelled_error()
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
        let channel = session.channel_open_session().await
            .map_err(|e| format!("打开 SFTP 通道失败: {}", e))?;
        channel.request_subsystem(true, "sftp").await
            .map_err(|e| format!("请求 SFTP 子系统失败: {}", e))?;
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream()).await
            .map_err(|e| format!("初始化 SFTP 会话失败: {}", e))?;
        *cache = Some(sftp);
        log::info!("SFTP 子系统通道已建立并缓存");
    }
    Ok(())
}

/// SFTP 目录项
#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    /// 文件/目录名（不含路径）
    pub name: String,
    /// 完整路径
    pub path: String,
    /// 是否为目录
    pub is_dir: bool,
    /// 文件大小（字节），目录为 0
    pub size: u64,
    /// 修改时间（Unix 时间戳，秒）
    pub modified: Option<u64>,
    /// 权限字符串（如 "-rw-r--r--"）
    pub permissions: Option<String>,
}

/// SFTP 文件信息（stat 结果）
#[derive(Debug, Clone, Serialize)]
pub struct SftpFileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
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

    let path = if remote_path.is_empty() { "." } else { remote_path };
    let read_dir = sftp.read_dir(path).await
        .map_err(|e| format!("读取目录 '{}' 失败: {}", path, e))?;

    let mut result: Vec<SftpEntry> = read_dir
        .into_iter()
        .map(|entry| {
            let name = entry.file_name();
            let full_path = if path.ends_with('/') {
                format!("{}{}", path, name)
            } else {
                format!("{}/{}", path, name)
            };
            let meta = entry.metadata();
            let perm_str = permissions_to_string(meta.permissions);
            SftpEntry {
                name,
                path: full_path,
                is_dir: meta.is_dir(),
                size: meta.size.unwrap_or(0),
                modified: meta.mtime.map(|t| t as u64),
                permissions: Some(perm_str),
            }
        })
        .collect();

    // 排序：目录优先，然后按名称字母序
    result.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(result)
}

/// 获取远程文件信息
pub async fn sftp_stat(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<SftpFileInfo, String> {
    get_or_create_sftp(session, sftp_cache).await?;
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let stat = sftp.metadata(remote_path).await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;

    let name = std::path::Path::new(remote_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| remote_path.to_string());

    Ok(SftpFileInfo {
        name,
        path: remote_path.to_string(),
        is_dir: stat.is_dir(),
        size: stat.size.unwrap_or(0),
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
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;

    let stat = sftp.metadata(remote_path).await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
    let total_size = stat.size.unwrap_or(0);

    let mut remote_file = sftp.open(remote_path).await
        .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?;

    let read_len = std::cmp::min(max_bytes, total_size);
    let mut buf = vec![0u8; read_len as usize];
    let mut total_read: u64 = 0;

    while total_read < read_len {
        let remaining = (read_len - total_read) as usize;
        let start = total_read as usize;
        let n = remote_file.read(&mut buf[start..start + remaining]).await
            .map_err(|e| format!("读取远程文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        total_read += n as u64;
    }

    buf.truncate(total_read as usize);
    log::info!(
        "SFTP 读取文件头: {} (读取 {} / 共 {} bytes)",
        remote_path, total_read, total_size
    );
    Ok((buf, total_size))
}

/// 下载远程文件到本地
/// - `on_progress` — 可选进度回调 (bytes_done, bytes_total)，经节流后调用
/// - `cancel` — 可选取消标志，传输循环每块检查
///
/// 设计要点：
/// - `sftp_cache` 锁仅在打开远程文件句柄时短暂持有，传输循环期间释放，
///   避免阻塞同会话的其他 SFTP 操作（目录刷新、stat 等）。
/// - 取消时清理本地半成品文件，避免残留。
pub async fn sftp_download(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
    local_path: &str,
    on_progress: Option<&(dyn Fn(u64, u64) + Send + Sync)>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<u64, String> {
    get_or_create_sftp(session, sftp_cache).await?;

    // 仅在打开句柄 + 获取大小时持锁，之后立即释放
    let (mut remote_file, remote_size): (russh_sftp::client::fs::File, u64) = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        let file = sftp.open(remote_path).await
            .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?;
        // 使用已打开句柄的 metadata，避免再次 sftp.metadata() 的额外 RTT
        let meta = file.metadata().await
            .map_err(|e| format!("获取远程文件信息 '{}' 失败: {}", remote_path, e))?;
        let size = meta.size.unwrap_or(0);
        (file, size)
    };

    let mut local_file = tokio::fs::File::create(local_path).await
        .map_err(|e| format!("创建本地文件 '{}' 失败: {}", local_path, e))?;

    let mut buf = [0u8; TRANSFER_BUF_SIZE];
    let mut total: u64 = 0;
    let mut throttle = ProgressThrottle::new();

    loop {
        if is_cancelled(cancel) {
            // 清理本地半成品文件
            let _ = tokio::fs::remove_file(local_path).await;
            return Err(transfer_cancelled_error());
        }
        let n = remote_file.read(&mut buf).await
            .map_err(|e| format!("读取远程文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        local_file.write_all(&buf[..n]).await
            .map_err(|e| format!("写入本地文件失败: {}", e))?;
        total += n as u64;
        if let Some(cb) = on_progress {
            if throttle.should_emit(total, remote_size) {
                cb(total, remote_size);
            }
        }
    }

    // 最终进度事件（确保 UI 显示 100%）
    if let Some(cb) = on_progress {
        cb(total, remote_size);
    }
    local_file.flush().await.map_err(|e| format!("刷新本地文件失败: {}", e))?;
    log::info!(
        "SFTP 下载完成: {} -> {} ({} bytes, remote_size={})",
        remote_path, local_path, total, remote_size
    );
    Ok(total)
}

/// 上传本地文件到远程
/// - `on_progress` — 可选进度回调 (bytes_done, bytes_total)，经节流后调用
/// - `cancel` — 可选取消标志，传输循环每块检查
///
/// 设计要点：
/// - `sftp_cache` 锁仅在创建远程文件句柄时短暂持有，传输循环期间释放。
/// - 取消时：先 `drop(remote_file)` 关闭远端句柄，再 `remove_file` 半成品文件，
///   最后将 SFTP 缓存置 None 强制下次重新协商（避免污染的 SFTP 通道复用导致性能下降）。
pub async fn sftp_upload(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    local_path: &str,
    remote_path: &str,
    on_progress: Option<&(dyn Fn(u64, u64) + Send + Sync)>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<u64, String> {
    get_or_create_sftp(session, sftp_cache).await?;

    let mut local_file = tokio::fs::File::open(local_path).await
        .map_err(|e| format!("打开本地文件 '{}' 失败: {}", local_path, e))?;

    let local_size = local_file.metadata().await
        .map(|m| m.len())
        .unwrap_or(0);

    // 仅在创建远程文件句柄时持锁，之后释放
    let mut remote_file: russh_sftp::client::fs::File = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        sftp.create(remote_path).await
            .map_err(|e| format!("创建远程文件 '{}' 失败: {}", remote_path, e))?
    };

    let mut buf = [0u8; TRANSFER_BUF_SIZE];
    let mut total: u64 = 0;
    let mut throttle = ProgressThrottle::new();

    loop {
        if is_cancelled(cancel) {
            // 1. 先关闭远端文件句柄（触发 SFTP close），避免 remove_file 时文件仍被占用
            remote_file.flush().await.ok();
            drop(remote_file);
            // 2. 删除远端半成品文件
            {
                let cache = sftp_cache.lock().await;
                if let Some(sftp) = cache.as_ref() {
                    let _ = sftp.remove_file(remote_path).await;
                }
            }
            // 3. 丢弃可能受污染的 SFTP 缓存，强制下次操作重新建立 SFTP 子系统通道。
            {
                let mut cache = sftp_cache.lock().await;
                *cache = None;
            }
            return Err(transfer_cancelled_error());
        }
        let n = local_file.read(&mut buf).await
            .map_err(|e| format!("读取本地文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        // 分块 write：使用 write_all 写入整块。
        // 注意：russh-sftp 的 File::write 是 async，write_all 内部循环直到全部写入。
        // 为支持取消，这里不做分片，依靠循环顶部的取消检查。
        remote_file.write_all(&buf[..n]).await
            .map_err(|e| format!("写入远程文件失败: {}", e))?;
        total += n as u64;
        if let Some(cb) = on_progress {
            if throttle.should_emit(total, local_size) {
                cb(total, local_size);
            }
        }
    }

    // 最终进度事件（确保 UI 显示 100%）
    if let Some(cb) = on_progress {
        cb(total, local_size);
    }
    remote_file.flush().await.map_err(|e| format!("刷新远程文件失败: {}", e))?;
    log::info!(
        "SFTP 上传完成: {} -> {} ({} bytes, local_size={})",
        local_path, remote_path, total, local_size
    );
    Ok(total)
}

/// 上传出错时清理远端半成品文件。
///
/// 在 sftp_upload 返回 Err 的所有路径调用，避免远端残留不完整文件
/// 导致下次上传同名文件时大小不匹配等问题。
pub async fn cleanup_remote_partial(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) {
    let _ = session; // 保持参数签名一致，session 已由 sftp_cache 内部使用
    // 确保 SFTP 已初始化（若 sftp_upload 在 get_or_create_sftp 前失败，缓存可能为 None）
    let _ = get_or_create_sftp(session, sftp_cache).await;
    let cache = sftp_cache.lock().await;
    if let Some(sftp) = cache.as_ref() {
        let _ = sftp.remove_file(remote_path).await;
    }
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

    let stat = sftp.metadata(remote_path).await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;

    if stat.is_dir() {
        // rmdir 要求目录为空
        match sftp.remove_dir(remote_path).await {
            Ok(()) => {
                log::info!("SFTP 已删除目录: {}", remote_path);
            }
            Err(e) => {
                return Err(format!("删除目录 '{}' 失败（可能非空）: {}", remote_path, e));
            }
        }
    } else {
        sftp.remove_file(remote_path).await
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

    sftp.rename(from_path, to_path).await
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

    sftp.create_dir(remote_path).await
        .map_err(|e| format!("创建目录 '{}' 失败: {}", remote_path, e))?;

    log::info!("SFTP 已创建目录: {}", remote_path);
    Ok(())
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

    let mut file = sftp.create(remote_path).await
        .map_err(|e| format!("创建文件 '{}' 失败: {}", remote_path, e))?;
    file.flush().await.map_err(|e| format!("刷新文件 '{}' 失败: {}", remote_path, e))?;

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

    let mut stat = sftp.metadata(remote_path).await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
    stat.permissions = Some(mode);
    sftp.set_metadata(remote_path, stat).await
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
            let stat = match sftp.metadata(remote_path).await {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("批量删除: 获取 '{}' 信息失败: {}", remote_path, e);
                    failed.push(remote_path.clone());
                    continue;
                }
            };
            if stat.is_dir() {
                sftp.remove_dir(remote_path).await.map_err(|e| format!("删除目录失败: {}", e))
            } else {
                sftp.remove_file(remote_path).await.map_err(|e| format!("删除文件失败: {}", e))
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

            let stat = sftp.metadata(path).await
                .map_err(|e| format!("获取 '{}' 信息失败: {}", path, e))?;

            if !stat.is_dir() {
                // 文件：持锁删除后释放
                sftp.remove_file(path).await
                    .map_err(|e| format!("删除文件 '{}' 失败: {}", path, e))?;
                log::info!("SFTP 递归删除文件: {}", path);
                return Ok(());
            }

            let read_dir = sftp.read_dir(path).await
                .map_err(|e| format!("读取目录 '{}' 失败: {}", path, e))?;

            let children: Vec<String> = read_dir.into_iter()
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
            sftp.remove_dir(path).await
                .map_err(|e| format!("删除目录 '{}' 失败: {}", path, e))?;
        }
        log::info!("SFTP 递归删除目录: {}", path);
        Ok(())
    })
}

/// 递归列出目录下所有文件（扁平列表，含相对路径和大小）
/// 最大递归深度为 100 层，超过后静默跳过深层子目录（防御恶意构造的深层目录）。
async fn list_dir_recursive(
    sftp: &russh_sftp::client::SftpSession,
    base_path: &str,
    prefix: &str,
) -> Result<Vec<(String, u64)>, String> {
    list_dir_recursive_impl(sftp, base_path, prefix, 0).await
}

type ListDirRecursiveFut<'a> = Pin<Box<dyn Future<Output = Result<Vec<(String, u64)>, String>> + Send + 'a>>;

fn list_dir_recursive_impl<'a>(
    sftp: &'a russh_sftp::client::SftpSession,
    base_path: &'a str,
    prefix: &'a str,
    depth: u32,
) -> ListDirRecursiveFut<'a> {
    Box::pin(async move {
        const MAX_DEPTH: u32 = 100;
        if depth > MAX_DEPTH {
            log::warn!("SFTP 递归列表超过最大深度 {} 层，跳过: {}", MAX_DEPTH, base_path);
            return Ok(Vec::new());
        }
        let mut files: Vec<(String, u64)> = Vec::new();
        let read_dir = sftp.read_dir(base_path).await
            .map_err(|e| format!("读取目录 '{}' 失败: {}", base_path, e))?;

        for entry in read_dir {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let full_path = if base_path.ends_with('/') {
                format!("{}{}", base_path, name)
            } else {
                format!("{}/{}", base_path, name)
            };

            let entry_stat = entry.metadata();
            if entry_stat.is_dir() {
                let child_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", prefix, name)
                };
                let child_files = list_dir_recursive_impl(sftp, &full_path, &child_prefix, depth + 1).await?;
                files.extend(child_files);
            } else {
                let rel_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", prefix, name)
                };
                files.push((rel_path, entry_stat.size.unwrap_or(0)));
            }
        }
        Ok(files)
    })
}

/// 递归下载远程目录到本地
/// - `on_progress` — 可选进度回调 (cur_file, files_done, files_total)
/// - `cancel` — 可选取消标志，每个文件开始前及块读取循环中检查
#[allow(clippy::type_complexity)]
pub async fn sftp_download_dir(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_dir: &str,
    local_dir: &std::path::Path,
    on_progress: Option<&(dyn Fn(&str, u64, u64) + Send + Sync)>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<u64, String> {
    get_or_create_sftp(session, sftp_cache).await?;

    // 收集文件列表（仅在持锁期间访问 SFTP 对象）
    let files = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        list_dir_recursive(sftp, remote_dir, "").await?
    };

    // Create local directory structure
    let mut dirs_set = std::collections::HashSet::new();
    for (rel_path, _) in &files {
        if let Some(parent) = std::path::Path::new(rel_path).parent() {
            if !parent.as_os_str().is_empty() {
                dirs_set.insert(parent.to_path_buf());
            }
        }
    }
    for d in &dirs_set {
        tokio::fs::create_dir_all(local_dir.join(d)).await
            .map_err(|e| format!("创建本地目录失败: {}", e))?;
    }

    // Download each file
    let mut total_bytes: u64 = 0;
    let files_total = files.len() as u64;
    let mut files_done: u64 = 0;

    for (rel_path, _file_size) in &files {
        if is_cancelled(cancel) {
            return Err(transfer_cancelled_error());
        }
        let remote_path = format!("{}/{}", remote_dir, rel_path);
        let local_path = local_dir.join(rel_path);

        // 打开远程文件句柄（仅此段持锁）；后续 read 不再访问 sftp_cache
        let mut remote_file = {
            let cache = sftp_cache.lock().await;
            let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
            sftp.open(&remote_path).await
                .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?
        };

        let mut local_file = tokio::fs::File::create(&local_path).await
            .map_err(|e| format!("创建本地文件 '{}' 失败: {}", local_path.display(), e))?;

        let mut buf = [0u8; TRANSFER_BUF_SIZE];

        loop {
            if is_cancelled(cancel) {
                // 清理当前正在下载的半成品文件
                let _ = tokio::fs::remove_file(&local_path).await;
                return Err(transfer_cancelled_error());
            }
            let n = remote_file.read(&mut buf).await
                .map_err(|e| format!("读取远程文件失败: {}", e))?;
            if n == 0 {
                break;
            }
            local_file.write_all(&buf[..n]).await
                .map_err(|e| format!("写入本地文件失败: {}", e))?;
            total_bytes += n as u64;
        }

        local_file.flush().await.map_err(|e| format!("刷新本地文件失败: {}", e))?;
        files_done += 1;

        if let Some(cb) = on_progress {
            cb(rel_path, files_done, files_total);
        }
    }

    log::info!(
        "SFTP 递归下载目录完成: {} -> {} ({} files, {} bytes)",
        remote_dir,
        local_dir.display(),
        files_total,
        total_bytes
    );
    Ok(total_bytes)
}

// ── 辅助函数 ───────────────────────────────────────────

/// 将 Unix 权限位转换为字符串表示（如 "-rw-r--r--"）
fn permissions_to_string(perm: Option<u32>) -> String {
    let p = perm.unwrap_or(0);
    let mut s = String::with_capacity(10);

    // 文件类型
    s.push(if p & 0o040000 != 0 { 'd' } else { '-' });
    // Owner
    s.push(if p & 0o400 != 0 { 'r' } else { '-' });
    s.push(if p & 0o200 != 0 { 'w' } else { '-' });
    s.push(if p & 0o100 != 0 { 'x' } else { '-' });
    // Group
    s.push(if p & 0o040 != 0 { 'r' } else { '-' });
    s.push(if p & 0o020 != 0 { 'w' } else { '-' });
    s.push(if p & 0o010 != 0 { 'x' } else { '-' });
    // Others
    s.push(if p & 0o004 != 0 { 'r' } else { '-' });
    s.push(if p & 0o002 != 0 { 'w' } else { '-' });
    s.push(if p & 0o001 != 0 { 'x' } else { '-' });

    s
}
