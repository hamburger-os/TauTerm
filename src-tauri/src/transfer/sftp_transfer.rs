//! SFTP 文件传输适配器
//!
//! 将 `ssh_file_service.rs` 中的 SFTP 自由函数适配到统一的 `FileTransfer` trait。
//! 通过 `SshSideChannel::create_file_transfer()` 创建，消除 commands.rs 中的
//! `downcast_ref::<SshSideChannel>()` 类型不安全转换。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::kernel::file_transfer::{
    FileTransfer, FileTransferError, FileTransferOptions, ProgressPosition, TransferDirection,
    UnifiedProgress,
};
use crate::transfer::ssh_file_service::{SftpEntryType, SftpUploadOptions, SftpWriteOutcome};
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
        Self {
            session,
            sftp_cache,
        }
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

#[derive(Debug, Clone)]
struct ReceiveFilePlan {
    remote_path: String,
    local_path: String,
    size: u64,
}

fn remote_basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("download")
        .to_string()
}

fn remote_relative(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    path.strip_prefix(&format!("{}/", base))
        .or_else(|| path.strip_prefix(base))
        .unwrap_or(path)
        .trim_start_matches('/')
        .to_string()
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
        options: &FileTransferOptions,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        let mut results = Vec::new();
        let total = files.len();
        let total_aggregate = files.iter().map(|f| f.size).sum::<u64>();
        let mut completed_bytes: u64 = 0;
        let rd = remote_dir.map(|d| d.trim_end_matches('/')).unwrap_or("/");

        log::info!(
            "SFTP 批量上传开始: {} 个文件 → {} (合计 {} bytes, overwrite={:?})",
            total,
            rd,
            total_aggregate,
            options.overwrite_policy
        );

        for (i, file) in files.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                for remaining in files.iter().skip(i) {
                    results.push(BatchFileResult {
                        file_name: remaining.name.clone(),
                        status: "skipped".into(),
                        size: 0,
                        error: Some("传输已取消".into()),
                    });
                }
                break;
            }

            let remote_path = if rd == "/" || rd.is_empty() {
                format!("/{}", file.name)
            } else {
                format!("{}/{}", rd, file.name)
            };
            let display_name = file.name.clone();
            let pt = progress.clone();
            let fname = display_name.clone();
            let base_completed = completed_bytes;
            let on_progress = move |done: u64, total_bytes: u64, speed: Option<f64>| {
                let _ = pt.send(UnifiedProgress::chunk_with_speed(
                    "sftp",
                    &fname,
                    done,
                    total_bytes,
                    ProgressPosition {
                        file_index: i,
                        total_files: total,
                        aggregate_bytes: base_completed + done,
                        aggregate_total: total_aggregate,
                    },
                    TransferDirection::Send,
                    speed,
                ));
            };

            let _ = progress.send(UnifiedProgress::file_start(
                "sftp",
                &display_name,
                file.size,
                ProgressPosition {
                    file_index: i,
                    total_files: total,
                    aggregate_bytes: completed_bytes,
                    aggregate_total: total_aggregate,
                },
                TransferDirection::Send,
            ));

            let result = crate::transfer::ssh_file_service::sftp_upload(
                &self.session,
                &self.sftp_cache,
                &file.path,
                &remote_path,
                SftpUploadOptions {
                    mtime: Some(file.mtime).filter(|&t| t > 0),
                    overwrite_policy: options.overwrite_policy,
                },
                Some(&on_progress),
                Some(&cancel),
            )
            .await;

            match result {
                Ok(SftpWriteOutcome::Completed { bytes, final_path }) => {
                    completed_bytes += bytes;
                    log::info!(
                        "SFTP 上传完成 {}/{}: {} -> {} ({} bytes)",
                        i + 1,
                        total,
                        display_name,
                        final_path,
                        bytes
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &display_name,
                        bytes,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Send,
                        true,
                        None,
                    ));
                    results.push(BatchFileResult {
                        file_name: display_name,
                        status: "completed".into(),
                        size: bytes,
                        error: None,
                    });
                }
                Ok(SftpWriteOutcome::Skipped { final_path }) => {
                    log::info!("SFTP 上传按覆盖策略跳过: {}", final_path);
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &display_name,
                        0,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Send,
                        true,
                        None,
                    ));
                    results.push(BatchFileResult {
                        file_name: display_name,
                        status: "skipped".into(),
                        size: 0,
                        error: None,
                    });
                }
                Err(e) => {
                    let is_cancelled = cancel.load(Ordering::SeqCst);
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &display_name,
                        0,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Send,
                        false,
                        Some(e.clone()),
                    ));
                    results.push(BatchFileResult {
                        file_name: display_name,
                        status: if is_cancelled { "skipped" } else { "failed" }.into(),
                        size: 0,
                        error: Some(e),
                    });
                    if is_cancelled {
                        for remaining in files.iter().skip(i + 1) {
                            results.push(BatchFileResult {
                                file_name: remaining.name.clone(),
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

        let _ = progress.send(UnifiedProgress::batch_complete(
            "sftp",
            TransferDirection::Send,
            completed,
            failed,
            skipped,
        ));

        if cancel.load(Ordering::SeqCst) {
            return Err(FileTransferError::Cancelled);
        }
        if failed > 0 {
            let first_err = results
                .iter()
                .filter(|r| r.status == "failed")
                .filter_map(|r| r.error.as_deref())
                .next()
                .unwrap_or("部分文件上传失败");
            return Err(FileTransferError::Other(format!(
                "{} 个文件上传失败：{}",
                failed, first_err
            )));
        }
        Ok(results)
    }

    async fn receive(
        &self,
        download_dir: &str,
        remote_paths: &[String],
        options: &FileTransferOptions,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        let mut plans: Vec<ReceiveFilePlan> = Vec::new();
        let mut results: Vec<BatchFileResult> = Vec::new();

        // 先建立明确的源→目标计划。目录节点本身会创建到本地，因此空目录可完整保留；
        // 符号链接默认不跟随，避免递归穿出用户选中的目录树。
        for (root_index, remote_path) in remote_paths.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                return Err(FileTransferError::Cancelled);
            }
            let stat = crate::transfer::ssh_file_service::sftp_stat(
                &self.session,
                &self.sftp_cache,
                remote_path,
            )
            .await
            .map_err(FileTransferError::Other)?;

            let explicit_destination = options
                .destination_paths
                .get(root_index)
                .filter(|path| !path.trim().is_empty())
                .cloned();

            match stat.entry_type {
                SftpEntryType::File => {
                    let local_path = explicit_destination.unwrap_or_else(|| {
                        Path::new(download_dir)
                            .join(remote_basename(remote_path))
                            .to_string_lossy()
                            .to_string()
                    });
                    plans.push(ReceiveFilePlan {
                        remote_path: remote_path.clone(),
                        local_path,
                        size: stat.size,
                    });
                }
                SftpEntryType::Directory => {
                    let local_root = explicit_destination.unwrap_or_else(|| {
                        Path::new(download_dir)
                            .join(remote_basename(remote_path))
                            .to_string_lossy()
                            .to_string()
                    });
                    tokio::fs::create_dir_all(&local_root).await.map_err(|e| {
                        FileTransferError::Other(format!(
                            "创建本地目录 '{}' 失败: {}",
                            local_root, e
                        ))
                    })?;

                    let tree = crate::transfer::ssh_file_service::sftp_list_tree_recursive(
                        &self.session,
                        &self.sftp_cache,
                        remote_path,
                    )
                    .await
                    .map_err(FileTransferError::Other)?;

                    for item in tree {
                        let relative = remote_relative(remote_path, &item.path);
                        let local_path = Path::new(&local_root)
                            .join(&relative)
                            .to_string_lossy()
                            .to_string();
                        match item.entry_type {
                            SftpEntryType::Directory => {
                                tokio::fs::create_dir_all(&local_path).await.map_err(|e| {
                                    FileTransferError::Other(format!(
                                        "创建本地目录 '{}' 失败: {}",
                                        local_path, e
                                    ))
                                })?;
                            }
                            SftpEntryType::File => plans.push(ReceiveFilePlan {
                                remote_path: item.path,
                                local_path,
                                size: item.size,
                            }),
                            SftpEntryType::Symlink => results.push(BatchFileResult {
                                file_name: item.path,
                                status: "skipped".into(),
                                size: 0,
                                error: Some("符号链接默认不跟随，已跳过".into()),
                            }),
                            _ => results.push(BatchFileResult {
                                file_name: item.path,
                                status: "skipped".into(),
                                size: 0,
                                error: Some("非常规文件类型未复制".into()),
                            }),
                        }
                    }
                }
                SftpEntryType::Symlink => results.push(BatchFileResult {
                    file_name: remote_path.clone(),
                    status: "skipped".into(),
                    size: 0,
                    error: Some("符号链接默认不跟随，已跳过".into()),
                }),
                _ => results.push(BatchFileResult {
                    file_name: remote_path.clone(),
                    status: "skipped".into(),
                    size: 0,
                    error: Some("非常规文件类型未复制".into()),
                }),
            }
        }

        let total = plans.len();
        let total_aggregate = plans.iter().map(|plan| plan.size).sum::<u64>();
        let mut completed_bytes = 0u64;

        for (i, plan) in plans.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                for remaining in plans.iter().skip(i) {
                    results.push(BatchFileResult {
                        file_name: remote_basename(&remaining.remote_path),
                        status: "skipped".into(),
                        size: 0,
                        error: Some("传输已取消".into()),
                    });
                }
                break;
            }

            let file_name = remote_basename(&plan.remote_path);
            let pt = progress.clone();
            let fname = file_name.clone();
            let base_completed = completed_bytes;
            let ta = total_aggregate;
            let on_progress = move |done: u64, total_bytes: u64, speed: Option<f64>| {
                let _ = pt.send(UnifiedProgress::chunk_with_speed(
                    "sftp",
                    &fname,
                    done,
                    total_bytes,
                    ProgressPosition {
                        file_index: i,
                        total_files: total,
                        aggregate_bytes: base_completed + done,
                        aggregate_total: ta,
                    },
                    TransferDirection::Receive,
                    speed,
                ));
            };

            let _ = progress.send(UnifiedProgress::file_start(
                "sftp",
                &file_name,
                plan.size,
                ProgressPosition {
                    file_index: i,
                    total_files: total,
                    aggregate_bytes: completed_bytes,
                    aggregate_total: total_aggregate,
                },
                TransferDirection::Receive,
            ));

            let result = crate::transfer::ssh_file_service::sftp_download(
                &self.session,
                &self.sftp_cache,
                &plan.remote_path,
                &plan.local_path,
                options.overwrite_policy,
                Some(&on_progress),
                Some(&cancel),
            )
            .await;

            match result {
                Ok(SftpWriteOutcome::Completed { bytes, final_path }) => {
                    completed_bytes += bytes;
                    log::info!(
                        "SFTP 下载完成 {}/{}: {} -> {} ({} bytes)",
                        i + 1,
                        total,
                        file_name,
                        final_path,
                        bytes
                    );
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &file_name,
                        bytes,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Receive,
                        true,
                        None,
                    ));
                    results.push(BatchFileResult {
                        file_name,
                        status: "completed".into(),
                        size: bytes,
                        error: None,
                    });
                }
                Ok(SftpWriteOutcome::Skipped { final_path }) => {
                    log::info!("SFTP 下载按覆盖策略跳过: {}", final_path);
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &file_name,
                        0,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Receive,
                        true,
                        None,
                    ));
                    results.push(BatchFileResult {
                        file_name,
                        status: "skipped".into(),
                        size: 0,
                        error: None,
                    });
                }
                Err(e) => {
                    let is_cancelled = cancel.load(Ordering::SeqCst);
                    let _ = progress.send(UnifiedProgress::file_complete(
                        "sftp",
                        &file_name,
                        0,
                        ProgressPosition {
                            file_index: i,
                            total_files: total,
                            aggregate_bytes: completed_bytes,
                            aggregate_total: total_aggregate,
                        },
                        TransferDirection::Receive,
                        false,
                        Some(e.clone()),
                    ));
                    results.push(BatchFileResult {
                        file_name,
                        status: if is_cancelled { "skipped" } else { "failed" }.into(),
                        size: 0,
                        error: Some(e),
                    });
                    if is_cancelled {
                        for remaining in plans.iter().skip(i + 1) {
                            results.push(BatchFileResult {
                                file_name: remote_basename(&remaining.remote_path),
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

        let _ = progress.send(UnifiedProgress::batch_complete(
            "sftp",
            TransferDirection::Receive,
            completed,
            failed,
            skipped,
        ));

        if cancel.load(Ordering::SeqCst) {
            return Err(FileTransferError::Cancelled);
        }
        if failed > 0 {
            let first_err = results
                .iter()
                .filter(|r| r.status == "failed")
                .filter_map(|r| r.error.as_deref())
                .next()
                .unwrap_or("部分文件下载失败");
            return Err(FileTransferError::Other(format!(
                "{} 个文件下载失败：{}",
                failed, first_err
            )));
        }

        Ok(results)
    }
}
