//! SSH application-layer connector and SSH-only IPC commands.

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::session_store::{
    ContainerSessionCreateOptions, ContainerSessionRuntime, SessionState,
};
use crate::plugin_application::{
    create_terminal_sub_channel, terminal_sub_channel_connected_payload, ConnectSessionRequest,
    SessionConnectFuture,
};
use crate::AppState;

pub(crate) fn session_connector(
    app: AppHandle,
    request: ConnectSessionRequest,
) -> SessionConnectFuture {
    Box::pin(async move {
        let state: State<'_, AppState> = app.state();
        connect_session(app.clone(), state, request).await
    })
}

/// SSH 会话连接（新架构：SshAdapter::connect → ProtocolConnection → SessionStore）
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        mut params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;

    let effective_session_id = session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let pending_ssh_credential = crate::plugins::ssh::application::prepare_session_params(
        &state.credential_store,
        &effective_session_id,
        &mut params,
    )?;
    let ssh_config = crate::plugins::ssh::application::hydrate_config_with_pending(
        &state.credential_store,
        &params,
        pending_ssh_credential.as_ref(),
    )?;
    let journald_enabled_val = params
        .get("journald_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let file_service_enabled_val = params
        .get("file_service_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let file_service_protocol_val = params
        .get("file_service_protocol")
        .and_then(Value::as_str)
        .unwrap_or("sftp")
        .to_string();

    let default_session_name = format!("{}@{}", ssh_config.username, ssh_config.host);

    // SSH 插件自己持有 known-host 验证状态；AppState 只通过 PluginRuntime 取得 contribution。
    // 运行时认证秘密直接 move 给连接流程，避免为了会话名称再复制一份密码/私钥。
    let ssh_adapter =
        state.plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID);
    let conn = ssh_adapter
        .connect_with_config(ssh_config, app.clone())
        .await
        .map_err(|e| e.to_string())?;

    let session_name = name.unwrap_or(default_session_name);
    let transfer_enabled_val = transfer_enabled.unwrap_or(true);
    let transfer_protocol_val = transfer_protocol.unwrap_or_else(|| "sftp".into());
    let send_bar_enabled_val = send_bar_enabled.unwrap_or(true);

    let teardown_delay = conn.teardown_delay;
    let service = conn.service;
    let file_transfer = conn.file_transfer;
    let channel_factory = conn.channel_factory;
    let attachment = conn.on_attached;
    // 第一个 SSH PTY 已由 transport async bridge 收敛为普通 DataPlaneRuntime。
    let channel_for_ch0 = conn
        .data_plane
        .ok_or_else(|| "SSH 连接缺少 DataPlane".to_string())?;

    // 1. 创建容器父 session（无 I/O loop）
    let parent_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;

        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "ssh".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: transfer_enabled_val,
                transfer_protocol: Some(transfer_protocol_val.clone()),
                send_bar_enabled: send_bar_enabled_val,
                id_override: Some(effective_session_id.clone()),
            },
            ContainerSessionRuntime {
                service,
                file_transfer,
                channel_factory,
                io: None,
                automation_io: None,
                attachment,
                teardown_delay,
            },
        )?
    };

    // 2. 建立首个 PTY、验证运行态并激活 DataPlane。父 Session 创建后到凭据提交前，
    // 任意失败都统一回滚父 Session，避免分散的半连接清理分支。
    let setup_result: Result<_, String> = async {
        let host_key_fingerprint = state
            .plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID)
            .runtime(&parent_id)
            .and_then(|runtime| runtime.host_key_fingerprint.clone());
        if let Some(ref fp) = host_key_fingerprint {
            log::info!("SSH 主机密钥指纹: {}", fp);
        }

        let channel0_id =
            create_terminal_sub_channel(&app, &state, &parent_id, channel_for_ch0, false, false)
                .await?;

        let (actual_name, actual_params) = {
            let store = state.session_store.lock().map_err(|e| e.to_string())?;
            let handle = store
                .get_session(&parent_id)
                .ok_or_else(|| format!("SSH 父会话 {} 已在连接完成前关闭", parent_id))?;
            if handle.state != SessionState::Connected {
                return Err("SSH 父会话已在连接完成前断开".to_string());
            }
            (handle.name.clone(), handle.params.clone())
        };

        let channel0_connected =
            terminal_sub_channel_connected_payload(&state, &parent_id, &channel0_id)?;

        state
            .session_store
            .lock()
            .map_err(|e| e.to_string())?
            .activate_data_plane(&channel0_id)?;

        // 凭据提交是连接事务的最后一个可失败步骤。失败时外层统一关闭父 Session。
        if let Some(pending) = pending_ssh_credential {
            crate::plugins::ssh::application::commit_credential(
                &state.credential_store,
                pending,
            )?;
        }

        Ok((
            host_key_fingerprint,
            channel0_id,
            actual_name,
            actual_params,
            channel0_connected,
        ))
    }
    .await;

    let (
        host_key_fingerprint,
        channel0_id,
        actual_name,
        actual_params,
        channel0_connected,
    ) = match setup_result {
        Ok(value) => value,
        Err(error) => {
            log::error!("SSH 连接事务失败，回滚父会话 {}: {}", parent_id, error);
            if let Ok(mut store) = state.session_store.lock() {
                if let Err(cleanup_error) = store.close_session(&parent_id) {
                    log::warn!(
                        "SSH 连接事务回滚父会话失败 {}: {}",
                        parent_id,
                        cleanup_error
                    );
                }
            }
            return Err(error);
        }
    };

    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );

    log::info!(
        "SSH 会话已连接: {} @ {} (parent: {}, channel_0: {})",
        actual_name,
        endpoint,
        parent_id,
        channel0_id
    );

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": parent_id,
            "endpoint": endpoint,
            "connection_type": "ssh",
            "plugin_id": "ssh",
            "name": actual_name,
            "params": actual_params,
            "connected_at": connected_at,
            "transfer_enabled": transfer_enabled_val,
            "transfer_protocol": transfer_protocol_val,
            "send_bar_enabled": send_bar_enabled_val,
            "file_service_enabled": file_service_enabled_val,
            "file_service_protocol": file_service_protocol_val,
            "journald_enabled": journald_enabled_val,
            "host_key_fingerprint": host_key_fingerprint,
            "is_container": true,
        }),
    );
    let _ = app.emit("session-connected", channel0_connected);

    Ok(parent_id)
}

// ── 命令：SSH 主机密钥确认 ────────────────────────────

/// 用户确认或拒绝 SSH 主机密钥。
///
/// SSH 连接过程中，`build_connection_with_config` 发现新主机密钥时
/// 通过 `ssh-host-key-verify` 事件将指纹发送到前端。
/// 前端展示确认对话框后调用此命令，由 `HostKeyVerifier` 将用户决策
/// 回传给正在阻塞等待的 `build_connection_with_config`。
#[tauri::command]
pub async fn confirm_host_key(
    state: tauri::State<'_, AppState>,
    request_id: String,
    accepted: bool,
) -> Result<(), String> {
    let ok = state
        .plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID)
        .respond_to_host_key_verification(&request_id, accepted)
        .await?;
    if !ok {
        return Err("主机密钥验证请求未找到或已过期".into());
    }
    log::info!(
        "SSH 主机密钥请求 {}: {}",
        if accepted {
            "已接受并记住"
        } else {
            "已拒绝"
        },
        &request_id[..request_id.len().min(16)]
    );
    Ok(())
}

/// 主机密钥发生变化时，只有显式安全确认流程可以替换同算法的旧信任。
/// 当前连接仍保持拒绝；更新成功后用户需要重新发起连接。
#[tauri::command]
pub fn replace_host_key(
    state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    algorithm: String,
    fingerprint: String,
) -> Result<(), String> {
    if host.trim().is_empty()
        || algorithm.trim().is_empty()
        || fingerprint.trim().is_empty()
        || port == 0
    {
        return Err("SSH 主机密钥替换参数无效".into());
    }

    state
        .plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID)
        .replace_known_host(&host, port, &algorithm, &fingerprint)?;
    log::warn!(
        "SSH 主机信任已由用户显式替换: {}:{} ({})",
        host,
        port,
        algorithm
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournaldQueryRequest {
    pub session_id: String,
    pub level: Option<String>,
    pub keyword: Option<String>,
    pub unit: Option<String>,
    pub kernel_only: Option<bool>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournaldExportRequest {
    pub session_id: String,
    pub file_path: String,
    pub level: Option<String>,
    pub keyword: Option<String>,
    pub unit: Option<String>,
    pub kernel_only: Option<bool>,
    pub since: Option<String>,
    pub until: Option<String>,
}

// ── 命令：SSH 文件服务（SFTP）────────────────────

use crate::plugins::ssh::SshRuntime;
use crate::transfer::ssh_file_service::{
    sftp_chmod, sftp_delete, sftp_delete_batch, sftp_delete_recursive, sftp_list_dir, sftp_mkdir,
    sftp_new_file, sftp_rename, sftp_stat,
};

/// 解析父 Session 后从 SSH 插件自己的 typed registry 获取 runtime。
fn get_ssh_runtime(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<std::sync::Arc<SshRuntime>, String> {
    let parent_id = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        let parent_id = store
            .resolve_parent_id(session_id)
            .ok_or_else(|| store.session_not_found(session_id))?;
        if store
            .get_session(&parent_id)
            .is_some_and(|h| h.state == SessionState::Disconnected)
        {
            return Err("会话已断开".to_string());
        }
        parent_id
    };
    state
        .plugin::<crate::plugins::ssh::SshAdapter>(crate::plugins::ssh::PLUGIN_ID)
        .runtime(&parent_id)
        .ok_or_else(|| format!("会话 {} 不包含 SSH runtime（可能不是 SSH 连接）", parent_id))
}

/// SFTP 列出远程目录
#[tauri::command]
pub async fn sftp_list_dir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<Vec<crate::transfer::ssh_file_service::SftpEntry>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_list_dir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 获取文件信息
#[tauri::command]
pub async fn sftp_stat_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<crate::transfer::ssh_file_service::SftpFileInfo, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_stat(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 修改文件权限
#[tauri::command]
pub async fn sftp_chmod_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    mode: u32,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_chmod(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path, mode).await
}

/// SFTP 删除文件或目录
#[tauri::command]
pub async fn sftp_delete_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 重命名/移动文件或目录
#[tauri::command]
pub async fn sftp_rename_cmd(
    state: State<'_, AppState>,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_rename(
        &ssh_runtime.session,
        &ssh_runtime.sftp,
        &from_path,
        &to_path,
    )
    .await
}

/// SFTP 创建目录
#[tauri::command]
pub async fn sftp_mkdir_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_mkdir(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 创建空文件
#[tauri::command]
pub async fn sftp_new_file_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_new_file(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// SFTP 批量删除
#[tauri::command]
pub async fn sftp_delete_batch_cmd(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_batch(&ssh_runtime.session, &ssh_runtime.sftp, &paths).await
}

/// SFTP 递归删除目录（包括子内容）
#[tauri::command]
pub async fn sftp_delete_recursive_cmd(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    sftp_delete_recursive(&ssh_runtime.session, &ssh_runtime.sftp, &remote_path).await
}

/// 获取 SSH 会话的远程用户 home 目录。
///
/// 第一次真正使用文件服务时按需解析并缓存；基础 SSH 建连不执行额外远端命令。
#[tauri::command]
pub async fn get_ssh_home_dir(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    Ok(ssh_runtime
        .resolve_home_dir()
        .await
        .unwrap_or_else(|| "/".to_string()))
}

// ── Journald 日志查看器命令 ──────────────────────────

/// 启动 journald 实时流式追踪
///
/// 在远程 SSH 会话上打开 exec 通道，执行 `journalctl -o json -f`，
/// spawn tokio task 循环读取并为每条日志 emit `journald:entry` 事件。
#[tauri::command]
pub async fn start_journald_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    level: Option<String>,
    keyword: Option<String>,
    unit: Option<String>,
    kernel_only: Option<bool>,
) -> Result<(), String> {
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since: None,
        until: None,
    };
    crate::plugins::ssh::journald::start_journald_stream(
        &ssh_runtime.session,
        app,
        session_id,
        &filters,
    )
    .await
}

/// 停止 journald 实时追踪
///
/// 设置对应 session 的 cancel 标志，使 tokio 流式循环优雅退出。
#[tauri::command]
pub async fn stop_journald_stream(session_id: String) -> Result<(), String> {
    // 确认式停止：等待后端任务真正退出并释放注册表，
    // 保证返回后前端可立即重新开始（消除"已在运行中"窗口期）
    crate::plugins::ssh::journald::stop_journald_stream_confirm(&session_id).await;
    Ok(())
}

/// 查询 journald 历史日志（单次请求，支持游标分页）
#[tauri::command]
pub async fn journald_query_cmd(
    state: State<'_, AppState>,
    request: JournaldQueryRequest,
) -> Result<crate::plugins::ssh::journald::JournaldQueryResponse, String> {
    let JournaldQueryRequest {
        session_id,
        level,
        keyword,
        unit,
        kernel_only,
        since,
        until,
        cursor,
        limit,
    } = request;
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since,
        until,
    };
    let limit = limit.unwrap_or(100);
    let (entries, next_cursor) = crate::plugins::ssh::journald::journald_query(
        &ssh_runtime.session,
        &filters,
        cursor.as_deref(),
        limit,
    )
    .await?;
    let has_more = entries.len() >= limit;
    Ok(crate::plugins::ssh::journald::JournaldQueryResponse {
        entries,
        next_cursor,
        has_more,
    })
}

/// 启动 journald 日志导出
///
/// 在远程 SSH 会话上循环分页拉取所有匹配过滤条件的日志条目，
/// 序列化为 JSON 后写入指定文件路径。spawn tokio task 异步执行，
/// 通过事件 `journald:export-progress` / `journald:export-complete` /
/// `journald:export-error` 向前端报告进度。
#[tauri::command]
pub async fn start_journald_export(
    app: AppHandle,
    state: State<'_, AppState>,
    request: JournaldExportRequest,
) -> Result<(), String> {
    let JournaldExportRequest {
        session_id,
        file_path,
        level,
        keyword,
        unit,
        kernel_only,
        since,
        until,
    } = request;
    let ssh_runtime = get_ssh_runtime(&state, &session_id)?;
    let filters = crate::plugins::ssh::journald::JournaldQueryFilters {
        level,
        keyword,
        unit,
        kernel_only: kernel_only.unwrap_or(false),
        since,
        until,
    };
    crate::plugins::ssh::journald::start_journald_export(
        &ssh_runtime.session,
        app,
        session_id,
        &filters,
        file_path,
    )
    .await
}

/// 停止 journald 日志导出
///
/// 设置对应 session 的 cancel 标志，使导出循循环优雅退出。
#[tauri::command]
pub fn stop_journald_export(session_id: String) -> Result<(), String> {
    crate::plugins::ssh::journald::stop_journald_export(&session_id);
    Ok(())
}
