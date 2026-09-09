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
    FileTransfer, FileTransferError, FileTransferOptions, OverwritePolicy, ProgressPosition,
    TransferDirection, UnifiedProgress,
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

fn local_safe_component(name: &str) -> Result<String, FileTransferError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .chars()
            .any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || name.ends_with(' ')
        || name.ends_with('.')
    {
        return Err(FileTransferError::Other(format!(
            "远端文件名 '{}' 无法安全映射到本地路径",
            name
        )));
    }

    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return Err(FileTransferError::Other(format!(
            "远端文件名 '{}' 是本地平台保留名称",
            name
        )));
    }

    Ok(name.to_string())
}

fn safe_local_basename(remote_path: &str) -> Result<String, FileTransferError> {
    local_safe_component(&remote_basename(remote_path))
}

fn safe_local_relative(base: &str, path: &str) -> Result<std::path::PathBuf, FileTransferError> {
    let base = base.trim_end_matches('/');
    let relative = path
        .strip_prefix(&format!("{}/", base))
        .or_else(|| path.strip_prefix(base))
        .unwrap_or(path)
        .trim_start_matches('/');

    let mut local = std::path::PathBuf::new();
    for component in relative.split('/') {
        local.push(local_safe_component(component)?);
    }
    if local.as_os_str().is_empty() {
        return Err(FileTransferError::Other(format!(
            "远端路径 '{}' 无法映射到本地相对路径",
            path
        )));
    }
    Ok(local)
}

fn local_directory_keep_both_candidate(path: &Path, index: u32) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    parent.join(format!("{} ({})", name, index))
}

async fn try_create_local_directory(candidate: &Path) -> Result<bool, FileTransferError> {
    match tokio::fs::create_dir(candidate).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(FileTransferError::Other(format!(
            "创建本地目录 '{}' 失败: {}",
            candidate.display(),
            e
        ))),
    }
}

/// 为目录下载原子保留一个目标根目录。
///
/// - Skip：已有同名对象时整棵目录跳过。
/// - KeepBoth：使用 create_dir 的排他创建语义原子选择 "name (N)"。
/// - Replace：仅允许目标不存在。目录级 Replace 需要递归删除/回滚，风险远高于
///   文件替换，因此当前明确拒绝已有对象，要求用户选择 KeepBoth 或 Skip。
async fn prepare_local_directory_destination(
    requested: &Path,
    policy: OverwritePolicy,
) -> Result<Option<std::path::PathBuf>, FileTransferError> {
    if let Some(parent) = requested.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            FileTransferError::Other(format!(
                "创建下载目标父目录 '{}' 失败: {}",
                parent.display(),
                e
            ))
        })?;
    }

    match policy {
        OverwritePolicy::Skip => {
            if try_create_local_directory(requested).await? {
                Ok(Some(requested.to_path_buf()))
            } else {
                Ok(None)
            }
        }
        OverwritePolicy::KeepBoth => {
            for index in 0..=9999 {
                let candidate = if index == 0 {
                    requested.to_path_buf()
                } else {
                    local_directory_keep_both_candidate(requested, index)
                };
                if try_create_local_directory(&candidate).await? {
                    return Ok(Some(candidate));
                }
            }
            Err(FileTransferError::Other(format!(
                "无法为目录 '{}' 生成不冲突名称",
                requested.display()
            )))
        }
        OverwritePolicy::Replace => match tokio::fs::symlink_metadata(requested).await {
            Ok(_) => Err(FileTransferError::Other(format!(
                "目录目标 '{}' 已存在；目录替换不会自动合并或递归覆盖，请选择保留两份或跳过",
                requested.display()
            ))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(requested).await.map_err(|e| {
                    FileTransferError::Other(format!(
                        "创建本地目录 '{}' 失败: {}",
                        requested.display(),
                        e
                    ))
                })?;
                Ok(Some(requested.to_path_buf()))
            }
            Err(e) => Err(FileTransferError::Other(format!(
                "检查本地目录目标 '{}' 失败: {}",
                requested.display(),
                e
            ))),
        },
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
                    let local_path = match explicit_destination {
                        Some(path) => path,
                        None => Path::new(download_dir)
                            .join(safe_local_basename(remote_path)?)
                            .to_string_lossy()
                            .to_string(),
                    };
                    plans.push(ReceiveFilePlan {
                        remote_path: remote_path.clone(),
                        local_path,
                        size: stat.size,
                    });
                }
                SftpEntryType::Directory => {
                    let requested_root = match explicit_destination {
                        Some(path) => std::path::PathBuf::from(path),
                        None => Path::new(download_dir).join(safe_local_basename(remote_path)?),
                    };
                    let Some(local_root) = prepare_local_directory_destination(
                        &requested_root,
                        options.overwrite_policy,
                    )
                    .await?
                    else {
                        results.push(BatchFileResult {
                            file_name: remote_basename(remote_path),
                            status: "skipped".into(),
                            size: 0,
                            error: None,
                        });
                        continue;
                    };

                    let tree = crate::transfer::ssh_file_service::sftp_list_tree_recursive(
                        &self.session,
                        &self.sftp_cache,
                        remote_path,
                    )
                    .await
                    .map_err(FileTransferError::Other)?;

                    for item in tree {
                        let relative = match safe_local_relative(remote_path, &item.path) {
                            Ok(relative) => relative,
                            Err(error) => {
                                results.push(BatchFileResult {
                                    file_name: item.path,
                                    status: "failed".into(),
                                    size: 0,
                                    error: Some(error.to_string()),
                                });
                                continue;
                            }
                        };
                        let local_path = local_root.join(&relative).to_string_lossy().to_string();
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
