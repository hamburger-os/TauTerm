//! SFTP 远程文档读写。
//!
//! 远程文档是文件管理器“打开/编辑”工作流的协议边界，不属于批量文件传输任务。
//! 读取始终基于原始字节；保存由后端完成编码、并发版本校验、同目录临时文件写入与
//! commit/rollback，前端不会用“下载到本地再上传”的方式伪装文档编辑。

use crc32fast::hash as crc32;
use encoding_rs::{BIG5, EUC_JP, EUC_KR, GB18030, SHIFT_JIS, WINDOWS_1252};
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::kernel::session_store::SessionState;
use crate::plugins::ssh::handler::SshHandler;
use crate::plugins::ssh::{SshAdapter, SshRuntime, PLUGIN_ID};
use crate::AppState;

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

fn get_ssh_runtime(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<Arc<SshRuntime>, String> {
    let parent_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let parent_id = store
            .resolve_parent_id(session_id)
            .ok_or_else(|| store.session_not_found(session_id))?;
        if store
            .get_session(&parent_id)
            .is_some_and(|handle| handle.state == SessionState::Disconnected)
        {
            return Err("会话已断开".to_string());
        }
        parent_id
    };

    state
        .plugin::<SshAdapter>(PLUGIN_ID)
        .runtime(&parent_id)
        .ok_or_else(|| format!("会话 {} 不包含 SSH runtime（可能不是 SSH 连接）", parent_id))
}

async fn ensure_sftp(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
) -> Result<(), String> {
    let mut cache = sftp_cache.lock().await;
    if cache.is_some() {
        return Ok(());
    }

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
    Ok(())
}

fn is_regular_file(permissions: Option<u32>, fallback_is_dir: bool) -> bool {
    match permissions.map(|value| value & 0o170000) {
        Some(0o100000) => true,
        Some(0) | None => !fallback_is_dir,
        _ => false,
    }
}

fn permissions_to_string(permissions: Option<u32>) -> String {
    let p = permissions.unwrap_or(0);
    let mut value = String::with_capacity(10);
    value.push(match p & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o010000 => 'p',
        0o140000 => 's',
        0o060000 => 'b',
        0o020000 => 'c',
        _ => '-',
    });
    value.push(if p & 0o400 != 0 { 'r' } else { '-' });
    value.push(if p & 0o200 != 0 { 'w' } else { '-' });
    value.push(match (p & 0o4000 != 0, p & 0o100 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    value.push(if p & 0o040 != 0 { 'r' } else { '-' });
    value.push(if p & 0o020 != 0 { 'w' } else { '-' });
    value.push(match (p & 0o2000 != 0, p & 0o010 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    value.push(if p & 0o004 != 0 { 'r' } else { '-' });
    value.push(if p & 0o002 != 0 { 'w' } else { '-' });
    value.push(match (p & 0o1000 != 0, p & 0o001 != 0) {
        (true, true) => 't',
        (true, false) => 'T',
        (false, true) => 'x',
        (false, false) => '-',
    });
    value
}

async fn read_snapshot(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
    max_bytes: u64,
) -> Result<RemoteSnapshot, String> {
    ensure_sftp(session, sftp_cache).await?;

    let (mut file, size, modified, permissions) = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        let stat = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
        if !is_regular_file(stat.permissions, stat.is_dir()) {
            return Err(format!("仅支持打开普通文件: {}", remote_path));
        }
        let file = sftp
            .open(remote_path)
            .await
            .map_err(|e| format!("打开远程文件 '{}' 失败: {}", remote_path, e))?;
        (
            file,
            stat.size.unwrap_or(0),
            stat.mtime.map(u64::from),
            stat.permissions,
        )
    };

    let read_len = size.min(max_bytes);
    let mut data = vec![0u8; read_len as usize];
    let mut total_read = 0usize;
    while total_read < data.len() {
        let read = file
            .read(&mut data[total_read..])
            .await
            .map_err(|e| format!("读取远程文件 '{}' 失败: {}", remote_path, e))?;
        if read == 0 {
            break;
        }
        total_read += read;
    }
    data.truncate(total_read);
    drop(file);

    let unchanged = {
        let cache = sftp_cache.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        match sftp.symlink_metadata(remote_path).await {
            Ok(stat) => {
                is_regular_file(stat.permissions, stat.is_dir())
                    && stat.size.unwrap_or(0) == size
                    && (modified.is_none() || stat.mtime.map(u64::from) == modified)
            }
            Err(_) => false,
        }
    };
    if !unchanged || (size <= max_bytes && total_read as u64 != size) {
        return Err(format!("远程文件 '{}' 在读取期间发生变化，请重试", remote_path));
    }

    Ok(RemoteSnapshot {
        data,
        size,
        modified,
        permissions,
    })
}

async fn read_complete_snapshot(
    session: &Arc<russh::client::Handle<SshHandler>>,
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<RemoteSnapshot, String> {
    let snapshot = read_snapshot(session, sftp_cache, remote_path, DOCUMENT_EDIT_LIMIT + 1).await?;
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

fn serialize_text(text: &str, format: &RemoteDocumentFormat) -> Result<Vec<u8>, String> {
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
            output.len(), DOCUMENT_EDIT_LIMIT
        ));
    }
    Ok(output)
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

fn remote_sibling_artifact(path: &str, tag: &str) -> String {
    let (dir, name) = remote_parts(path);
    remote_join(
        dir,
        &format!(".{}.tauterm-document-{}-{}", name, tag, uuid::Uuid::new_v4()),
    )
}

async fn remove_remote_best_effort(
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    path: &str,
) {
    let cache = sftp_cache.lock().await;
    if let Some(sftp) = cache.as_ref() {
        if let Err(error) = sftp.remove_file(path).await {
            log::warn!("清理远程文档临时文件 '{}' 失败: {}", path, error);
        }
    }
}

async fn current_regular_metadata(
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    remote_path: &str,
) -> Result<Option<u32>, String> {
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
    let stat = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("获取文件信息 '{}' 失败: {}", remote_path, e))?;
    if !is_regular_file(stat.permissions, stat.is_dir()) {
        return Err(format!("仅支持保存普通文件: {}", remote_path));
    }
    Ok(stat.permissions)
}

async fn commit_document_temp(
    sftp_cache: &Arc<Mutex<Option<russh_sftp::client::SftpSession>>>,
    temp_path: &str,
    final_path: &str,
) -> Result<(), String> {
    let cache = sftp_cache.lock().await;
    let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
    let stat = sftp
        .symlink_metadata(final_path)
        .await
        .map_err(|e| format!("提交前获取文件信息 '{}' 失败: {}", final_path, e))?;
    if !is_regular_file(stat.permissions, stat.is_dir()) {
        return Err(format!("提交目标不再是普通文件: {}", final_path));
    }

    let backup_path = remote_sibling_artifact(final_path, "backup");
    sftp.rename(final_path, &backup_path)
        .await
        .map_err(|e| format!("备份远程原文件 '{}' 失败: {}", final_path, e))?;

    match sftp.rename(temp_path, final_path).await {
        Ok(()) => {
            if let Err(error) = sftp.remove_file(&backup_path).await {
                log::warn!("删除远程文档提交备份 '{}' 失败: {}", backup_path, error);
            }
            Ok(())
        }
        Err(commit_error) => match sftp.rename(&backup_path, final_path).await {
            Ok(()) => Err(format!("提交远程文档 '{}' 失败: {}", final_path, commit_error)),
            Err(rollback_error) => Err(format!(
                "提交远程文档 '{}' 失败: {}；回滚也失败，原文件仍保留在 '{}': {}",
                final_path, commit_error, backup_path, rollback_error
            )),
        },
    }
}

#[tauri::command]
pub async fn sftp_read_document_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    full: bool,
) -> Result<RemoteDocumentReadResult, String> {
    let runtime = get_ssh_runtime(&state, &session_id)?;
    let max_bytes = if full {
        DOCUMENT_EDIT_LIMIT + 1
    } else {
        DOCUMENT_PREVIEW_LIMIT
    };
    let snapshot = read_snapshot(&runtime.session, &runtime.sftp, &remote_path, max_bytes).await?;

    if full && snapshot.size > DOCUMENT_EDIT_LIMIT {
        return Err(format!(
            "远程文件超过可编辑上限（{} bytes > {} bytes）",
            snapshot.size, DOCUMENT_EDIT_LIMIT
        ));
    }

    let complete = snapshot.data.len() as u64 == snapshot.size;
    let result = RemoteDocumentReadResult {
        total_size: snapshot.size,
        modified: snapshot.modified,
        permissions: permissions_to_string(snapshot.permissions),
        truncated: !complete,
        editable: complete && snapshot.size <= DOCUMENT_EDIT_LIMIT,
        version: snapshot.version(complete),
        data: snapshot.data,
    };
    Ok(result)
}

#[tauri::command]
pub async fn remote_document_encode_preview_cmd(
    request: RemoteDocumentEncodeRequest,
) -> Result<RemoteDocumentEncodeResult, String> {
    let bytes = serialize_text(&request.text, &request.format)?;
    let total_size = bytes.len() as u64;
    let visible = bytes.len().min(DOCUMENT_HEX_LIMIT);
    Ok(RemoteDocumentEncodeResult {
        data: bytes[..visible].to_vec(),
        total_size,
        truncated: visible < bytes.len(),
    })
}

#[tauri::command]
pub async fn sftp_save_document_cmd(
    state: State<'_, AppState>,
    request: RemoteDocumentSaveRequest,
) -> Result<RemoteDocumentSaveResult, String> {
    if request.expected_version.content_crc32.is_none() && !request.force {
        return Err("保存前必须先完整加载文档，缺少内容版本".to_string());
    }

    let runtime = get_ssh_runtime(&state, &request.session_id)?;
    let bytes = serialize_text(&request.text, &request.format)?;
    let before = read_complete_snapshot(&runtime.session, &runtime.sftp, &request.remote_path).await?;
    let before_version = before.version(true);

    if !request.force && before_version != request.expected_version {
        return Ok(RemoteDocumentSaveResult {
            status: "conflict",
            version: None,
            current_version: Some(before_version),
        });
    }

    let temp_path = remote_sibling_artifact(&request.remote_path, "part");
    let mut temp_file = {
        let cache = runtime.sftp.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        sftp.open_with_flags(
            &temp_path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
        )
        .await
        .map_err(|e| format!("创建远程文档临时文件 '{}' 失败: {}", temp_path, e))?
    };

    if let Err(error) = temp_file.write_all(&bytes).await {
        drop(temp_file);
        remove_remote_best_effort(&runtime.sftp, &temp_path).await;
        return Err(format!("写入远程文档临时文件失败: {}", error));
    }
    if let Err(error) = temp_file.flush().await {
        drop(temp_file);
        remove_remote_best_effort(&runtime.sftp, &temp_path).await;
        return Err(format!("刷新远程文档临时文件失败: {}", error));
    }
    drop(temp_file);

    // 文档写入期间远端仍可能被其他工具修改。非 force 保存必须在正式提交前再次
    // 比较完整版本，避免 check-then-write 窗口造成 silent lost update。
    if !request.force {
        let after_write = read_complete_snapshot(
            &runtime.session,
            &runtime.sftp,
            &request.remote_path,
        )
        .await?;
        let current_version = after_write.version(true);
        if current_version != request.expected_version {
            remove_remote_best_effort(&runtime.sftp, &temp_path).await;
            return Ok(RemoteDocumentSaveResult {
                status: "conflict",
                version: None,
                current_version: Some(current_version),
            });
        }
    }

    // 保留原文件权限，但让文件内容修改自然获得新的 mtime。
    let permissions = current_regular_metadata(&runtime.sftp, &request.remote_path).await?;
    if let Some(mode) = permissions {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(0o100000 | (mode & 0o7777));
        let cache = runtime.sftp.lock().await;
        let sftp = cache.as_ref().ok_or_else(|| "SFTP 未初始化".to_string())?;
        if let Err(error) = sftp.set_metadata(&temp_path, attrs).await {
            drop(cache);
            remove_remote_best_effort(&runtime.sftp, &temp_path).await;
            return Err(format!("同步远程文档权限失败: {}", error));
        }
    }

    if let Err(error) = commit_document_temp(&runtime.sftp, &temp_path, &request.remote_path).await {
        remove_remote_best_effort(&runtime.sftp, &temp_path).await;
        return Err(error);
    }

    let saved = read_complete_snapshot(&runtime.session, &runtime.sftp, &request.remote_path).await?;
    let saved_version = saved.version(true);
    log::info!(
        "SFTP 远程文档保存完成: {} ({} bytes)",
        request.remote_path,
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
