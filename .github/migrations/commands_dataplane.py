from pathlib import Path
import re

P = Path('src-tauri/src/commands.rs')
s = P.read_text()

# Imports: commands no longer knows Channel/IoLoop implementation details.
s = s.replace('use crate::channel::io_loop::{IoLoopCmd, IoLoopContext};\nuse crate::channel::DisconnectInfo;\n', 'use crate::session::{DisconnectInfo, SessionDataPlane, SessionIo};\nuse crate::transport::DataPlaneRuntime;\n')
s = s.replace('    ChannelKind, ChannelOpenMode, ProtocolAdapter, TransferProtocolType,\n', '    ChannelOpenMode, ProtocolAdapter, TransferProtocolType,\n')
s = s.replace('    ContainerSessionCreateOptions, IoTaskHandle, SessionCreateOptions, SessionState, SessionStore,\n', '    ContainerSessionCreateOptions, SessionCreateOptions, SessionState, SessionStore,\n')

# Adapter capability logs no longer expose execution strategy.
s = re.sub(r'\n\s*let io_strategy = state\.(serial_adapter|telnet_adapter|ssh_adapter)\.io_strategy\(\);', '', s)
s = s.replace('"串口连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",\n        content_type,\n        io_strategy,\n        transfer_protocols', '"串口连接: content_type={:?}, transfer_protocols={:?}",\n        content_type,\n        transfer_protocols')
s = s.replace('"Telnet 连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",\n        content_type,\n        io_strategy,\n        transfer_protocols', '"Telnet 连接: content_type={:?}, transfer_protocols={:?}",\n        content_type,\n        transfer_protocols')
s = s.replace('"SSH 连接: content_type={:?}, io_strategy={:?}, transfer_protocols={:?}",\n        content_type,\n        io_strategy,\n        transfer_protocols_list', '"SSH 连接: content_type={:?}, transfer_protocols={:?}",\n        content_type,\n        transfer_protocols_list')

# Local Shell and SSH first terminal are already DataPlaneRuntime values.
s = s.replace('''    let first_channel = conn\n        .channel\n        .take()\n        .ok_or("Local Shell 连接缺少 PTY channel")?;''', '''    let first_channel = conn\n        .data_plane\n        .take()\n        .ok_or("Local Shell 连接缺少 DataPlane")?;''')
old_ssh = '''    // 分离 channel（第一个 PTY，作为通道 0 的 I/O）\n    let channel_for_ch0 = match conn.channel {\n        Some(ChannelKind::Async(ch)) => ChannelKind::Async(ch),\n        Some(crate::kernel::plugin_adapter::ChannelKind::Sync(_)) => {\n            return Err("SSH 连接期望 Async channel".to_string());\n        }\n        None => {\n            return Err("SSH 连接缺少 I/O channel".to_string());\n        }\n    };'''
new_ssh = '''    // 第一个 SSH PTY 已由 transport async bridge 收敛为普通 DataPlaneRuntime。\n    let channel_for_ch0 = conn\n        .data_plane\n        .ok_or_else(|| "SSH 连接缺少 DataPlane".to_string())?;'''
if old_ssh not in s:
    raise RuntimeError('missing SSH first-channel anchor')
s = s.replace(old_ssh, new_ssh)

# Terminal child registration becomes DataPlane-native.
start = s.index('async fn create_terminal_sub_channel(\n')
end = s.index('fn terminal_sub_channel_connected_payload(', start)
new_fn = r'''async fn create_terminal_sub_channel(
    app: &tauri::AppHandle,
    app_state: &AppState,
    parent_id: &str,
    runtime: DataPlaneRuntime,
    elevated: bool,
    announce_connected: bool,
) -> Result<String, String> {
    let (
        endpoint,
        plugin_id,
        params,
        data_mode,
        encoding,
        send_bar_enabled_val,
        file_service_enabled,
        journald_enabled,
        file_service_protocol,
    ) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        let active_children = handle
            .sub_connections
            .iter()
            .filter(|child| child.state != SessionState::Disconnected)
            .count();
        if active_children >= 32 {
            return Err("每个父会话最多允许 32 个活动终端".to_string());
        }
        (
            handle.endpoint.clone(),
            handle.plugin_id.clone(),
            handle.params.clone(),
            handle
                .params
                .get("data_mode")
                .and_then(Value::as_str)
                .unwrap_or("text")
                .to_string(),
            handle
                .params
                .get("encoding")
                .and_then(Value::as_str)
                .unwrap_or("utf-8")
                .to_string(),
            handle.send_bar_enabled,
            handle
                .params
                .get("file_service_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            handle
                .params
                .get("journald_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            handle
                .params
                .get("file_service_protocol")
                .and_then(Value::as_str)
                .unwrap_or("sftp")
                .to_string(),
        )
    };

    let channel_id = uuid::Uuid::new_v4().to_string();
    let log_tx = app_state.log_engine.lock().map_err(|e| e.to_string())?.sender();
    let on_data = create_on_data_callback(app, log_tx, data_mode, encoding.clone(), None);

    let app_disconnect = app.clone();
    let pid = parent_id.to_string();
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> = Box::new(move |channel_id, info| {
        let (parent_disconnected, retain_history) = {
            if let Ok(mut store) = app_disconnect.state::<AppState>().session_store.lock() {
                store.mark_sub_disconnected(&pid, &channel_id, info.retain_terminal);
                let (no_live_children, has_other_history) = store
                    .get_session(&pid)
                    .map(|handle| {
                        let no_live = handle.channel_factory.is_some()
                            && handle
                                .sub_connections
                                .iter()
                                .all(|child| child.state == SessionState::Disconnected);
                        let history = handle.sub_connections.iter().any(|child| {
                            child.id != channel_id
                                && child.state == SessionState::Disconnected
                                && child.retain_terminal
                        });
                        (no_live, history)
                    })
                    .unwrap_or((false, false));
                let retain = info.retain_terminal || has_other_history;
                if no_live_children {
                    let _ = store.close_session(&pid);
                    if !retain {
                        store.reset_child_counter(&pid);
                    }
                }
                (no_live_children, retain)
            } else {
                (false, info.retain_terminal)
            }
        };
        let _ = app_disconnect.emit(
            "channel-closed",
            serde_json::json!({
                "channel_id": channel_id,
                "parent_id": pid,
                "disconnect_info": &info,
            }),
        );
        if parent_disconnected {
            let parent_info = if retain_history {
                DisconnectInfo::remote_eof("所有活动终端已关闭")
            } else {
                info.clone()
            };
            let _ = app_disconnect.emit(
                "session-disconnected",
                serde_json::json!({
                    "session_id": pid,
                    "reason": &parent_info.reason,
                    "disconnect_info": &parent_info,
                }),
            );
        }
    });

    let io = Arc::new(SessionIo::new(
        Some(runtime.handle.clone()),
        None,
        encoding,
    ));
    let data_plane = SessionDataPlane::attach(runtime, channel_id.clone(), on_data, on_disconnect)
        .map_err(|e| e.to_string())?;
    let stats_cancel_flag = Arc::new(AtomicBool::new(false));
    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    SessionStore::spawn_stats_collector(
        app.clone(),
        channel_id.clone(),
        io.clone(),
        connected_at,
        stats_cancel_flag.clone(),
    );

    let (actual_index, channel_name) = {
        let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(parent_id);
        let handle = store.get_session_mut(parent_id).ok_or(not_found)?;
        if handle.state != SessionState::Connected {
            stats_cancel_flag.store(true, Ordering::SeqCst);
            data_plane.request_shutdown();
            return Err("父会话已断开，无法创建子连接".to_string());
        }
        let active_children = handle
            .sub_connections
            .iter()
            .filter(|child| child.state != SessionState::Disconnected)
            .count();
        if active_children >= 32 {
            stats_cancel_flag.store(true, Ordering::SeqCst);
            data_plane.request_shutdown();
            return Err("每个父会话最多允许 32 个活动终端".to_string());
        }
        let actual_idx = handle.next_child_index;
        handle.next_child_index = handle.next_child_index.saturating_add(1);
        let prefix = handle
            .channel_factory
            .as_ref()
            .map(|factory| factory.child_name_prefix())
            .unwrap_or("Channel");
        let actual_name = format!("{} {}", prefix, actual_idx + 1);
        let mut sub = crate::kernel::session_store::SubConnection::new(
            channel_id.clone(),
            actual_name.clone(),
            data_plane,
            io,
            actual_idx,
            elevated,
        );
        sub.connected_at = connected_at;
        sub.stats_cancel_flag = Some(stats_cancel_flag);
        handle.sub_connections.push(sub);
        (actual_idx, actual_name)
    };

    if announce_connected {
        let _ = app.emit(
            "session-connected",
            serde_json::json!({
                "session_id": channel_id,
                "endpoint": endpoint,
                "connection_type": plugin_id,
                "plugin_id": plugin_id,
                "name": channel_name,
                "params": params,
                "connected_at": connected_at,
                "transfer_enabled": false,
                "send_bar_enabled": send_bar_enabled_val,
                "parent_id": parent_id,
                "channel_index": actual_index,
                "elevated": elevated,
                "file_service_enabled": file_service_enabled,
                "file_service_protocol": file_service_protocol,
                "journald_enabled": journald_enabled,
            }),
        );
    }
    Ok(channel_id)
}

'''
s = s[:start] + new_fn + s[end:]

# Network container no longer receives a CommHandle capability.
s = s.replace('''            conn.side_channel.clone(),
            None,
            conn.comm_handle.clone(),''', '''            conn.side_channel.clone(),
            None,
            None,''')

# write_data uses SessionIo as the single send/encoding capability.
old = '''        (encoding, data_mode, store.get_comm_handle_for(&session_id))
    };
    // 文本路径：委托会话 CommHandle::send_text（单一转码策略点）；
    // 字节路径或会话无 comm_handle（SSH 容器）：直接写入原样透传
    let data_out = match (transcode, comm) {
        (true, Some(handle)) => handle.send_text(&data).map_err(|e| e.to_string())?,
        (true, None) | (false, _) => {
            let store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.write(&session_id, &data)?;
            data
        }
    };'''
new = '''        (encoding, data_mode, store.get_io_for(&session_id))
    };
    let io = comm.ok_or_else(|| format!("会话 {} 没有可写 I/O 能力", session_id))?;
    let data_out = if transcode {
        io.send_text(&data).map_err(|e| e.to_string())?
    } else {
        io.send(&data).map_err(|e| e.to_string())?;
        data
    };'''
if old not in s:
    raise RuntimeError('missing write_data anchor')
s = s.replace(old, new)

# resize is a terminal capability on SessionIo, not an IoLoop command.
start = s.index('#[tauri::command]\npub fn resize_pty(')
end = s.index('// ═══════════════════════════════════════════════════════════════\n// TFTP', start)
resize = r'''#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let io = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_io_for(&session_id)
            .ok_or_else(|| store.session_not_found(&session_id))?
    };
    io.resize_terminal(cols, rows).map_err(|e| e.to_string())
}

'''
s = s[:start] + resize + s[end:]

P.write_text(s)

# SSH child factory has one stale return type from the old abstraction.
p = Path('src-tauri/src/plugins/ssh/mod.rs')
ssh = p.read_text().replace(
    'async fn open_channel(&self, mode: ChannelOpenMode) -> Result<ChannelKind, SessionError>',
    'async fn open_channel(&self, mode: ChannelOpenMode) -> Result<DataPlaneRuntime, SessionError>',
)
p.write_text(ssh)

print('commands migrated to DataPlane')
