//! SFTP 文件传输适配器
//!
//! 将 `ssh_file_service.rs` 中的 SFTP 自由函数适配到统一的 `FileTransfer` trait。
//! 通过 `SshSideChannel::create_file_transfer()` 创建，消除 commands.rs 中的
//! `downcast_ref::<SshSideChannel>()` 类型不安全转换。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::kernel::file_transfer::{FileTransfer, FileTransferError, TransferDirection, UnifiedProgress};
use crate::transfer::types::{BatchFileResult, FileInfo};

/// SFTP 文件传输处理器
///
/// 从 SSH 侧通道创建，复用现有的 SSH session 和缓存的 SFTP 子系统。
/// 传输操作与终端 I/O 并行执行，不阻塞 shell 交互。
pub struct SftpFileTransfer {
    session: Arc<russh::client::Handle<crate::plugins::ssh::handler::SshHandler>>,
    sftp_cache: Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
}

impl SftpFileTransfer {
    pub fn new(
        session: Arc<russh::client::Handle<crate::plugins::ssh::handler::SshHandler>>,
        sftp_cache: Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    ) -> Self {
        Self { session, sftp_cache }
    }

    /// 获取内部 SSH session（保留用于未来扩展，当前所有操作通过 FileTransfer trait）
    #[allow(dead_code)]
    pub fn session(&self) -> &Arc<russh::client::Handle<crate::plugins::ssh::handler::SshHandler>> {
        &self.session
    }

    /// 获取内部 SFTP 缓存（保留用于未来扩展，当前所有操作通过 FileTransfer trait）
    #[allow(dead_code)]
    pub fn sftp_cache(&self) -> &Arc<Mutex<Option<russh_sftp::client::SftpSession>>> {
        &self.sftp_cache
    }
}

#[async_trait::async_trait]
impl FileTransfer for SftpFileTransfer {
    fn protocol(&self) -> &str {
        "sftp"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn send(
        &self,
        files: &[FileInfo],
        remote_dir: Option<&str>,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        let mut results = Vec::new();
        let total = files.len();
        let total_aggregate = files.iter().map(|f| f.size).sum::<u64>();
        let mut completed_bytes: u64 = 0;
        let rd = remote_dir.map(|d| d.trim_end_matches('/')).unwrap_or("/");

        log::info!(
            "SFTP 批量上传开始: {} 个文件 → {} (合计 {} bytes)",
            total, rd, total_aggregate
        );

        for (i, file) in files.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                log::info!("SFTP 上传已取消 (文件 {}/{})", i + 1, total);
                results.push(BatchFileResult {
                    file_name: file.name.clone(),
                    status: "skipped".into(),
                    size: 0,
                    error: Some("传输已取消".into()),
                });
                continue;
            }

            // 构建远程路径：remote_dir / filename（不污染 FileInfo.name）
            let remote_path = if rd == "/" || rd.is_empty() {
                format!("/{}", file.name)
            } else {
                format!("{}/{}", rd, file.name)
            };

            let display_name = file.name.clone();
            let pt = progress.clone();
            let fname = display_name.clone();
            let on_progress = move |done: u64, total_bytes: u64| {
                let _ = pt.send(UnifiedProgress::chunk(
                    "sftp", &fname, done, total_bytes,
                    i, total,
                    completed_bytes + done,
                    total_aggregate,
                    TransferDirection::Send,
                ));
            };

            log::debug!(
                "SFTP 上传文件 {}/{}: {} → {} ({} bytes)",
                i + 1, total, file.path, remote_path, file.size
            );

            let _ = progress.send(UnifiedProgress::file_start(
                "sftp", &display_name, file.size,
                i, total,
                completed_bytes, total_aggregate,
                TransferDirection::Send,
            ));

            let result = crate::transfer::ssh_file_service::sftp_upload(
                &self.session,
                &self.sftp_cache,
                &file.path,
                &remote_path,
                Some(file.mtime).filter(|&t| t > 0),
                Some(&on_progress),
                Some(&cancel),
            ).await;

            match result {
                Ok(bytes) => {
                    completed_bytes += bytes;
                    log::info!(
                        "SFTP 上传完成 {}/{}: {} ({} bytes, 聚合 {}/{})",
                        i + 1, total, display_name, bytes,
                        completed_bytes, total_aggregate
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp", &display_name, bytes,
                        i, total,
                        completed_bytes, total_aggregate,
                        TransferDirection::Send, true, None,
                    ));
                    results.push(BatchFileResult {
                        file_name: display_name.clone(),
                        status: "completed".into(),
                        size: bytes,
                        error: None,
                    });
                }
                Err(e) => {
                    let is_cancelled = cancel.load(Ordering::SeqCst);
                    log::error!(
                        "SFTP 上传失败 {}/{}: {} — {}",
                        i + 1, total, display_name, e
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp", &display_name, 0,
                        i, total,
                        completed_bytes, total_aggregate,
                        TransferDirection::Send, false,
                        Some(e.clone()),
                    ));
                    results.push(BatchFileResult {
                        file_name: display_name.clone(),
                        status: if is_cancelled { "skipped" } else { "failed" }.into(),
                        size: 0,
                        error: Some(e),
                    });
                    if is_cancelled {
                        break;
                    }
                    // 非取消失败：清理远端半成品文件，避免残留不完整数据
                    // 注：cleanup_remote_partial 返回 ()，错误已在内部记录日志
                    log::info!("SFTP 上传失败，清理远端残缺文件: {}", remote_path);
                    crate::transfer::ssh_file_service::cleanup_remote_partial(
                        &self.session,
                        &self.sftp_cache,
                        &remote_path,
                    ).await;
                }
            }
        }

        let completed = results.iter().filter(|r| r.status == "completed").count();
        let failed = results.iter().filter(|r| r.status == "failed").count();
        let skipped = results.iter().filter(|r| r.status == "skipped").count();

        log::info!(
            "SFTP 批量上传完成: {} 成功, {} 失败, {} 跳过 (共 {} 个, 合计 {} bytes)",
            completed, failed, skipped, total, completed_bytes
        );

        let _ = progress.send(UnifiedProgress::batch_complete(
            "sftp", TransferDirection::Send,
            completed, failed, skipped,
        ));

        // 如果有失败且没有成功（排除纯取消场景），向上传播错误
        if failed > 0 && completed == 0 {
            let first_err = results.iter()
                .filter_map(|r| r.error.as_deref())
                .next()
                .unwrap_or("所有文件传输失败");
            return Err(FileTransferError::Other(first_err.to_string()));
        }

        Ok(results)
    }

    async fn receive(
        &self,
        download_dir: &str,
        remote_paths: &[String],
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        use std::sync::atomic::Ordering;

        // Phase 1: 解析远程路径 → (base_dir, full_path) 对
        // base_dir 非空表示该文件来自目录递归展开，用于计算相对路径保留目录结构
        let mut resolved_pairs: Vec<(String, String)> = Vec::new();
        for path in remote_paths {
            let is_dir = {
                let cache = self.sftp_cache.lock().await;
                match cache.as_ref() {
                    Some(sftp) => sftp.metadata(path).await
                        .map(|m| m.is_dir())
                        .unwrap_or(false),
                    None => false,
                }
            };
            if is_dir {
                log::info!("SFTP 检测到目录，递归列举: {}", path);
                let files = crate::transfer::ssh_file_service::sftp_list_dir_recursive(
                    &self.session, &self.sftp_cache, path,
                ).await.map_err(FileTransferError::Other)?;
                log::info!("SFTP 目录 '{}' 包含 {} 个文件", path, files.len());
                for f in files {
                    resolved_pairs.push((path.clone(), f));
                }
            } else {
                resolved_pairs.push((String::new(), path.clone()));
            }
        }

        if resolved_pairs.is_empty() {
            return Err(FileTransferError::Other(
                "没有可下载的文件（目录为空或路径不存在）".into(),
            ));
        }

        let mut results = Vec::new();
        let total = resolved_pairs.len();

        log::info!(
            "SFTP 批量下载开始: {} 个文件 → {}",
            total, download_dir
        );

        // 预取所有文件大小以计算聚合总量
        // 注：tokio::sync::Mutex 设计允许跨 .await 持锁；若频繁超大规模目录下载
        // 导致其他 SFTP 操作阻塞，可考虑分批次获取 size 或缓存 SftpSession 句柄
        let mut file_sizes: Vec<u64> = Vec::with_capacity(total);
        let mut total_aggregate: u64 = 0;
        {
            let cache = self.sftp_cache.lock().await;
            if let Some(sftp) = cache.as_ref() {
                for (_base, remote_path) in resolved_pairs.iter() {
                    let sz = sftp.metadata(remote_path).await
                        .map(|m| m.size.unwrap_or(0))
                        .unwrap_or(0);
                    file_sizes.push(sz);
                    total_aggregate += sz;
                }
            } else {
                file_sizes.resize(total, 0);
            }
        }
        log::info!(
            "SFTP 下载聚合总量: {} bytes ({} 个文件)",
            total_aggregate, total
        );

        let mut completed_bytes: u64 = 0;

        for (i, (base_dir, remote_path)) in resolved_pairs.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                log::info!("SFTP 下载已取消 (文件 {}/{})", i + 1, total);
                results.push(BatchFileResult {
                    file_name: remote_path.clone(),
                    status: "skipped".into(),
                    size: 0,
                    error: Some("传输已取消".into()),
                });
                for (_b, remaining) in resolved_pairs.iter().skip(i + 1) {
                    results.push(BatchFileResult {
                        file_name: remaining.clone(),
                        status: "skipped".into(),
                        size: 0,
                        error: Some("传输已取消".into()),
                    });
                }
                break;
            }

            // 计算本地相对路径：保留目录结构
            let relative = if base_dir.is_empty() {
                // 单个文件：仅用文件名
                std::path::Path::new(remote_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| remote_path.clone())
            } else {
                // 目录下载：去掉 base_dir 前缀得到相对路径
                let base = base_dir.trim_end_matches('/');
                remote_path
                    .strip_prefix(&format!("{}/", base))
                    .or_else(|| remote_path.strip_prefix(base))
                    .unwrap_or(remote_path)
                    .trim_start_matches('/')
                    .to_string()
            };
            let local_file_path = std::path::Path::new(download_dir)
                .join(&relative)
                .to_string_lossy()
                .to_string();

            let file_name = std::path::Path::new(remote_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| remote_path.clone());
            let file_size = file_sizes.get(i).copied().unwrap_or(0);

            log::debug!(
                "SFTP 下载文件 {}/{}: {} → {} ({} bytes, 聚合 {}/{})",
                i + 1, total, remote_path, local_file_path, file_size,
                completed_bytes, total_aggregate
            );

            let pt = progress.clone();
            let fname = file_name.clone();
            let cb = completed_bytes;
            let ta = total_aggregate;
            let on_progress = move |done: u64, total_bytes: u64| {
                let _ = pt.send(UnifiedProgress::chunk(
                    "sftp", &fname, done, total_bytes,
                    i, total,
                    cb + done,
                    ta,
                    TransferDirection::Receive,
                ));
            };

            let _ = progress.send(UnifiedProgress::file_start(
                "sftp", &file_name, file_size,
                i, total,
                completed_bytes, total_aggregate,
                TransferDirection::Receive,
            ));

            let result = crate::transfer::ssh_file_service::sftp_download(
                &self.session,
                &self.sftp_cache,
                remote_path,
                &local_file_path,
                Some(&on_progress),
                Some(&cancel),
            ).await;

            match result {
                Ok(bytes) => {
                    completed_bytes += bytes;
                    log::info!(
                        "SFTP 下载完成 {}/{}: {} ({} bytes, 聚合 {}/{})",
                        i + 1, total, file_name, bytes,
                        completed_bytes, total_aggregate
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp", &file_name, bytes,
                        i, total,
                        completed_bytes, total_aggregate,
                        TransferDirection::Receive, true, None,
                    ));
                    results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "completed".into(),
                        size: bytes,
                        error: None,
                    });
                }
                Err(e) => {
                    let is_cancelled = cancel.load(Ordering::SeqCst);
                    log::error!(
                        "SFTP 下载失败 {}/{}: {} — {}",
                        i + 1, total, file_name, e
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp", &file_name, 0,
                        i, total,
                        completed_bytes, total_aggregate,
                        TransferDirection::Receive, false,
                        Some(e.clone()),
                    ));
                    results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: if is_cancelled { "skipped" } else { "failed" }.into(),
                        size: 0,
                        error: Some(e),
                    });
                    if is_cancelled {
                        // 跳过剩余文件（使用 resolved_pairs 而非 remote_paths，
                        // 因为目录展开后条目数可能不同，索引 i 来自 resolved_pairs）
                        for (_b, remaining) in resolved_pairs.iter().skip(i + 1) {
                            results.push(BatchFileResult {
                                file_name: remaining.clone(),
                                status: "skipped".into(),
                                size: 0,
                                error: Some("传输已取消".into()),
                            });
                        }
                        break;
                    }
                }
            }
        }

        let completed = results.iter().filter(|r| r.status == "completed").count();
        let failed = results.iter().filter(|r| r.status == "failed").count();
        let skipped = results.iter().filter(|r| r.status == "skipped").count();

        log::info!(
            "SFTP 批量下载完成: {} 成功, {} 失败, {} 跳过 (共 {} 个)",
            completed, failed, skipped, total
        );

        let _ = progress.send(UnifiedProgress::batch_complete(
            "sftp", TransferDirection::Receive,
            completed, failed, skipped,
        ));

        // 如果有失败且没有成功（排除纯取消场景），向上传播错误
        if failed > 0 && completed == 0 {
            let first_err = results.iter()
                .filter_map(|r| r.error.as_deref())
                .next()
                .unwrap_or("所有文件下载失败");
            return Err(FileTransferError::Other(first_err.to_string()));
        }

        Ok(results)
    }
}
