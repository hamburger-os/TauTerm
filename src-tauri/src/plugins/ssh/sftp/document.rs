//! SFTP 远程文档读写。
//!
//! 远程文档是文件管理器“打开/编辑”工作流的协议边界，不属于批量文件传输任务。
//! 读取始终基于原始字节；保存由后端完成编码、并发版本校验、同目录临时文件写入与
//! commit/rollback，前端不会用“下载到本地再上传”的方式伪装文档编辑。

use crc32fast::hash as crc32;
use encoding_rs::{BIG5, EUC_JP, EUC_KR, GB18030, SHIFT_JIS, WINDOWS_1252};
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use super::attrs::{
    entry_type_from_permissions, is_regular_file, permissions_to_string, SftpEntryType,
};
use super::capability::SftpCapability;
use super::paths::sibling_artifact as remote_sibling_artifact;
use super::service::sftp_read_head;

pub const DOCUMENT_PREVIEW_LIMIT: u64 = 1_048_576;
pub const DOCUMENT_EDIT_LIMIT: u64 = 4 * 1_048_576;
pub const DOCUMENT_HEX_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentVersion {
    pub size: u64,
    pub modified: Option<u64>,
    pub content_crc32: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentReadResult {
    pub data: Vec<u8>,
    pub total_size: u64,
    pub modified: Option<u64>,
    pub permissions: String,
    pub truncated: bool,
    pub editable: bool,
    pub version: RemoteDocumentVersion,
    /// 本次文档会话绑定的真实目标路径（链接在打开时解析一次）。
    pub resolved_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentFormat {
    pub encoding: String,
    pub line_ending: String,
    pub bom: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentSaveRequest {
    pub session_id: String,
    pub remote_path: String,
    pub text: String,
    pub format: RemoteDocumentFormat,
    pub expected_version: RemoteDocumentVersion,
    #[serde(default)]
    pub force: bool,
    /// 打开时绑定的真实目标路径；提交前若链接已重指向，保存按冲突拒绝。
    #[serde(default)]
    pub expected_resolved_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentSaveResult {
    pub status: &'static str,
    pub version: Option<RemoteDocumentVersion>,
    pub current_version: Option<RemoteDocumentVersion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentEncodeRequest {
    pub text: String,
    pub format: RemoteDocumentFormat,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDocumentEncodeResult {
    pub data: Vec<u8>,
    pub total_size: u64,
    pub truncated: bool,
}

struct RemoteSnapshot {
    data: Vec<u8>,
    size: u64,
    modified: Option<u64>,
    permissions: Option<u32>,
}

impl RemoteSnapshot {
    fn version(&self, complete: bool) -> RemoteDocumentVersion {
        RemoteDocumentVersion {
            size: self.size,
            modified: self.modified,
            content_crc32: complete.then(|| crc32(&self.data)),
        }
    }
}

enum CommitDocumentOutcome {
    Committed,
    Conflict(RemoteDocumentVersion),
}

async fn read_snapshot(
    cap: &SftpCapability,
    remote_path: &str,
    max_bytes: u64,
) -> Result<RemoteSnapshot, String> {
    let (size, modified, permissions) = {
        let sftp = cap.client().await?;
        let stat = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
        if !is_regular_file(stat.permissions, stat.is_dir()) {
            return Err(format!("仅支持打开普通文件: {}", remote_path));
        }
        (
            stat.size.unwrap_or(0),
            stat.mtime.map(u64::from),
            stat.permissions,
        )
    };

    let (data, observed_size) = sftp_read_head(cap, remote_path, max_bytes).await?;
    if observed_size != size {
        return Err(format!(
            "远程文件 '{}' 在读取期间发生变化，请重试",
            remote_path
        ));
    }
    let total_read = data.len() as u64;

    let unchanged = {
        let sftp = cap.client().await?;
        match sftp.symlink_metadata(remote_path).await {
            Ok(stat) => {
                is_regular_file(stat.permissions, stat.is_dir())
                    && stat.size.unwrap_or(0) == size
                    && (modified.is_none() || stat.mtime.map(u64::from) == modified)
            }
            Err(_) => false,
        }
    };
    if !unchanged || (size <= max_bytes && total_read != size) {
        return Err(format!(
            "远程文件 '{}' 在读取期间发生变化，请重试",
            remote_path
        ));
    }

    Ok(RemoteSnapshot {
        data,
        size,
        modified,
        permissions,
    })
}

async fn read_complete_snapshot(
    cap: &SftpCapability,
    remote_path: &str,
) -> Result<RemoteSnapshot, String> {
    let snapshot = read_snapshot(cap, remote_path, DOCUMENT_EDIT_LIMIT + 1).await?;
    if snapshot.size > DOCUMENT_EDIT_LIMIT {
        return Err(format!(
            "远程文件超过可编辑上限（{} bytes > {} bytes）",
            snapshot.size, DOCUMENT_EDIT_LIMIT
        ));
    }
    if snapshot.data.len() as u64 != snapshot.size {
        return Err(format!("未能完整读取远程文件 '{}'", remote_path));
    }
    Ok(snapshot)
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn serialize_text(text: &str, format: &RemoteDocumentFormat) -> Result<Vec<u8>, String> {
    let normalized = normalize_line_endings(text);
    let normalized = match format.line_ending.as_str() {
        "lf" => normalized,
        "crlf" => normalized.replace('\n', "\r\n"),
        "cr" => normalized.replace('\n', "\r"),
        other => return Err(format!("不支持的换行格式: {}", other)),
    };

    let mut output = match format.encoding.as_str() {
        "utf-8" => normalized.into_bytes(),
        "utf-16le" => normalized
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        "utf-16be" => normalized
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
        "gb18030" | "big5" | "shift_jis" | "euc-jp" | "euc-kr" | "windows-1252" => {
            if format.bom {
                return Err(format!("编码 {} 不支持 BOM", format.encoding));
            }
            let encoding = match format.encoding.as_str() {
                "gb18030" => GB18030,
                "big5" => BIG5,
                "shift_jis" => SHIFT_JIS,
                "euc-jp" => EUC_JP,
                "euc-kr" => EUC_KR,
                "windows-1252" => WINDOWS_1252,
                _ => unreachable!(),
            };
            let (encoded, _, had_errors) = encoding.encode(&normalized);
            if had_errors {
                return Err(format!(
                    "当前文本包含无法用 {} 表示的字符；已拒绝保存以避免静默替换",
                    format.encoding
                ));
            }
            encoded.into_owned()
        }
        other => return Err(format!("不支持的字符编码: {}", other)),
    };

    if format.bom {
        let bom: &[u8] = match format.encoding.as_str() {
            "utf-8" => &[0xef, 0xbb, 0xbf],
            "utf-16le" => &[0xff, 0xfe],
            "utf-16be" => &[0xfe, 0xff],
            _ => unreachable!(),
        };
        let mut with_bom = Vec::with_capacity(bom.len() + output.len());
        with_bom.extend_from_slice(bom);
        with_bom.append(&mut output);
        output = with_bom;
    }

    if output.len() as u64 > DOCUMENT_EDIT_LIMIT {
        return Err(format!(
            "保存后的文档超过可编辑上限（{} bytes > {} bytes）",
            output.len(),
            DOCUMENT_EDIT_LIMIT
        ));
    }
    Ok(output)
}

async fn current_regular_metadata(
    cap: &SftpCapability,
    remote_path: &str,
) -> Result<Option<u32>, String> {
    let sftp = cap.client().await?;
    let stat = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
    if !is_regular_file(stat.permissions, stat.is_dir()) {
        return Err(format!("仅支持保存普通文件: {}", remote_path));
    }
    Ok(stat.permissions)
}

async fn restore_document_backup(
    cap: &SftpCapability,
    backup_path: &str,
    final_path: &str,
) -> Result<(), String> {
    let sftp = cap.client().await?;
    sftp.rename(backup_path, final_path).await.map_err(|error| {
        format!(
            "回滚远程原文件失败，原文件仍保留在 '{}': {}",
            backup_path, error
        )
    })
}

async fn commit_document_temp(
    cap: &SftpCapability,
    temp_path: &str,
    final_path: &str,
    expected_version: Option<&RemoteDocumentVersion>,
) -> Result<CommitDocumentOutcome, String> {
    let backup_path = remote_sibling_artifact(final_path, "backup");
    {
        let sftp = cap.client().await?;
        let stat = sftp
            .symlink_metadata(final_path)
            .await
            .map_err(|e| format!("提交前获取文件信息 '{}' 失败: {}", final_path, e))?;
        if !is_regular_file(stat.permissions, stat.is_dir()) {
            return Err(format!("提交目标不再是普通文件: {}", final_path));
        }
        sftp.rename(final_path, &backup_path)
            .await
            .map_err(|e| format!("备份远程原文件 '{}' 失败: {}", final_path, e))?;
    }

    let backup_snapshot = if expected_version.is_some() {
        match read_snapshot(cap, &backup_path, DOCUMENT_EDIT_LIMIT + 1).await {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                let reason = format!("提交前校验远程原文件失败: {}", error);
                return match restore_document_backup(cap, &backup_path, final_path).await {
                    Ok(()) => Err(reason),
                    Err(rollback_error) => Err(format!("{}；{}", reason, rollback_error)),
                };
            }
        }
    } else {
        None
    };

    if let (Some(expected), Some(snapshot)) = (expected_version, backup_snapshot.as_ref()) {
        let complete = snapshot.data.len() as u64 == snapshot.size;
        let current_version = snapshot.version(complete);
        if &current_version != expected {
            return match restore_document_backup(cap, &backup_path, final_path).await {
                Ok(()) => Ok(CommitDocumentOutcome::Conflict(current_version)),
                Err(rollback_error) => {
                    Err(format!("提交前检测到远程文件并发修改；{}", rollback_error))
                }
            };
        }
    }

    let permissions = match backup_snapshot.as_ref() {
        Some(snapshot) => snapshot.permissions,
        None => match current_regular_metadata(cap, &backup_path).await {
            Ok(permissions) => permissions,
            Err(error) => {
                let reason = format!("读取远程原文件权限失败: {}", error);
                return match restore_document_backup(cap, &backup_path, final_path).await {
                    Ok(()) => Err(reason),
                    Err(rollback_error) => Err(format!("{}；{}", reason, rollback_error)),
                };
            }
        },
    };

    if let Some(mode) = permissions {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(0o100000 | (mode & 0o7777));
        let sftp = cap.client().await?;
        if let Err(error) = sftp.set_metadata(temp_path, attrs).await {
            let reason = format!("同步远程文档权限失败: {}", error);
            return match restore_document_backup(cap, &backup_path, final_path).await {
                Ok(()) => Err(reason),
                Err(rollback_error) => Err(format!("{}；{}", reason, rollback_error)),
            };
        }
    }

    let commit_result = {
        let sftp = cap.client().await?;
        sftp.rename(temp_path, final_path).await
    };

    match commit_result {
        Ok(()) => {
            cap.remove_best_effort(&backup_path).await;
            Ok(CommitDocumentOutcome::Committed)
        }
        Err(commit_error) => {
            let reason = format!("提交远程文档 '{}' 失败: {}", final_path, commit_error);
            match restore_document_backup(cap, &backup_path, final_path).await {
                Ok(()) => Err(reason),
                Err(rollback_error) => Err(format!("{}；{}", reason, rollback_error)),
            }
        }
    }
}

/// 解析文档会话绑定的目标对象路径。
///
/// 打开文档时如果是符号链接，就解析一次并绑定真实目标；后续读取、版本校验、
/// 临时文件提交都作用于目标对象，链接本身保持不变（write-through）。链接的
/// 重指向在提交前由 `expected_resolved_path` 检出并按冲突拒绝。
async fn resolve_document_target(
    cap: &SftpCapability,
    remote_path: &str,
) -> Result<String, String> {
    let kind = {
        let sftp = cap.client().await?;
        let stat = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
        entry_type_from_permissions(stat.permissions, stat.is_dir())
    };
    if kind != SftpEntryType::Symlink {
        return Ok(remote_path.to_string());
    }

    let resolved = super::service::sftp_resolve_entry(cap, remote_path)
        .await
        .map_err(|error| error.message)?;
    if !resolved.target_exists {
        return Err(format!(
            "符号链接目标不可用，无法打开文档: {} ({})",
            remote_path,
            resolved
                .resolution_error
                .unwrap_or_else(|| "无法解析链接目标".to_string())
        ));
    }
    resolved
        .target_path
        .ok_or_else(|| format!("无法解析符号链接的真实目标路径: {}", remote_path))
}

/// 读取远程文档。预览最多 1 MiB，完整打开最多 `DOCUMENT_EDIT_LIMIT`。
///
/// 截断快照永远只读；只有完整快照才允许保存，因此这里不提供 `truncated && editable`
/// 的组合。
pub(crate) async fn read_document(
    cap: &SftpCapability,
    remote_path: &str,
    full: bool,
) -> Result<RemoteDocumentReadResult, String> {
    let max_bytes = if full {
        DOCUMENT_EDIT_LIMIT + 1
    } else {
        DOCUMENT_PREVIEW_LIMIT
    };
    let target_path = resolve_document_target(cap, remote_path).await?;
    let snapshot = read_snapshot(cap, &target_path, max_bytes).await?;

    if full && snapshot.size > DOCUMENT_EDIT_LIMIT {
        return Err(format!(
            "远程文件超过可编辑上限（{} bytes > {} bytes）",
            snapshot.size, DOCUMENT_EDIT_LIMIT
        ));
    }

    let complete = snapshot.data.len() as u64 == snapshot.size;
    Ok(RemoteDocumentReadResult {
        total_size: snapshot.size,
        modified: snapshot.modified,
        permissions: permissions_to_string(snapshot.permissions),
        truncated: !complete,
        editable: complete && snapshot.size <= DOCUMENT_EDIT_LIMIT,
        version: snapshot.version(complete),
        data: snapshot.data,
        resolved_path: target_path,
    })
}

/// 保存远程文档：严格序列化 → 同目录排他临时文件 → flush → commit/rollback。
///
/// 非强制保存会在写临时文件前和正式 commit 前各校验一次远端版本；并发修改返回
/// `conflict`，绝不静默覆盖。
pub(crate) async fn save_document(
    cap: &SftpCapability,
    request: RemoteDocumentSaveRequest,
) -> Result<RemoteDocumentSaveResult, String> {
    if request.expected_version.content_crc32.is_none() && !request.force {
        return Err("保存前必须先完整加载文档，缺少内容版本".to_string());
    }

    // 链接在打开时绑定；提交前重新解析，链接被重指向时按冲突拒绝，绝不写到新目标。
    let target_path = resolve_document_target(cap, &request.remote_path).await?;
    if let Some(expected) = request.expected_resolved_path.as_deref() {
        if expected != target_path {
            return Err(format!(
                "符号链接目标已改变，保存冲突：文档打开时绑定 '{}'，当前指向 '{}'，未写入",
                expected, target_path
            ));
        }
    }

    let bytes = serialize_text(&request.text, &request.format)?;
    let before = read_complete_snapshot(cap, &target_path).await?;
    let before_version = before.version(true);

    if !request.force && before_version != request.expected_version {
        return Ok(RemoteDocumentSaveResult {
            status: "conflict",
            version: None,
            current_version: Some(before_version),
        });
    }

    let temp_path = remote_sibling_artifact(&target_path, "document-part");
    let mut temp_file = {
        let sftp = cap.client().await?;
        sftp.open_with_flags(
            &temp_path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
        )
        .await
        .map_err(|e| format!("创建远程文档临时文件 '{}' 失败: {}", temp_path, e))?
    };

    if let Err(error) = temp_file.write_all(&bytes).await {
        drop(temp_file);
        cap.remove_best_effort(&temp_path).await;
        return Err(format!("写入远程文档临时文件失败: {}", error));
    }
    if let Err(error) = temp_file.flush().await {
        drop(temp_file);
        cap.remove_best_effort(&temp_path).await;
        return Err(format!("刷新远程文档临时文件失败: {}", error));
    }
    drop(temp_file);

    let expected_version = (!request.force).then_some(&request.expected_version);
    match commit_document_temp(cap, &temp_path, &target_path, expected_version).await {
        Ok(CommitDocumentOutcome::Committed) => {}
        Ok(CommitDocumentOutcome::Conflict(current_version)) => {
            cap.remove_best_effort(&temp_path).await;
            return Ok(RemoteDocumentSaveResult {
                status: "conflict",
                version: None,
                current_version: Some(current_version),
            });
        }
        Err(error) => {
            cap.remove_best_effort(&temp_path).await;
            return Err(error);
        }
    }

    let saved = read_complete_snapshot(cap, &target_path).await?;
    let saved_version = saved.version(true);
    log::info!(
        "SFTP 远程文档保存完成: {} -> {} ({} bytes)",
        request.remote_path,
        target_path,
        saved.size
    );
    Ok(RemoteDocumentSaveResult {
        status: "saved",
        version: Some(saved_version),
        current_version: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(encoding: &str, line_ending: &str, bom: bool) -> RemoteDocumentFormat {
        RemoteDocumentFormat {
            encoding: encoding.to_string(),
            line_ending: line_ending.to_string(),
            bom,
        }
    }

    #[test]
    fn utf8_roundtrip_format_keeps_bom_and_crlf() {
        let bytes = serialize_text("a\nb\n", &format("utf-8", "crlf", true)).unwrap();
        assert_eq!(&bytes[..3], &[0xef, 0xbb, 0xbf]);
        assert_eq!(&bytes[3..], b"a\r\nb\r\n");
    }

    #[test]
    fn utf16_endianness_is_explicit() {
        assert_eq!(
            serialize_text("A", &format("utf-16le", "lf", true)).unwrap(),
            vec![0xff, 0xfe, 0x41, 0x00]
        );
        assert_eq!(
            serialize_text("A", &format("utf-16be", "lf", true)).unwrap(),
            vec![0xfe, 0xff, 0x00, 0x41]
        );
    }

    #[test]
    fn legacy_encoding_refuses_unmappable_text() {
        let result = serialize_text("中文🙂", &format("windows-1252", "lf", false));
        assert!(result.is_err());
    }

    #[test]
    fn normalizes_mixed_line_endings_before_serializing() {
        let bytes = serialize_text("a\r\nb\rc\n", &format("utf-8", "lf", false)).unwrap();
        assert_eq!(bytes, b"a\nb\nc\n");
    }
}
