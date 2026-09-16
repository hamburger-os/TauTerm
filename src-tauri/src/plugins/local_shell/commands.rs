//! Local Shell application-layer Session connector.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::plugin_adapter::ChannelOpenMode;
use crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};
use crate::plugin_application::{
    create_terminal_sub_channel, ConnectSessionRequest, SessionConnectFuture,
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

/// Local Shell 会话连接（LocalShellAdapter → PTY Channel → SessionStore）。
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    mut request: ConnectSessionRequest,
) -> Result<String, String> {
    if request
        .name
        .as_deref()
        .is_none_or(|name| name.trim().is_empty())
    {
        request.name = Some(
            crate::plugins::local_shell::LocalShellAdapter::default_session_name(&request.params)?,
        );
    }
    let initial_mode = if request.initial_elevated {
        ChannelOpenMode::Elevated
    } else {
        ChannelOpenMode::Standard
    };
    let mut conn = state
        .plugin::<crate::plugins::local_shell::LocalShellAdapter>(
            crate::plugins::local_shell::PLUGIN_ID,
        )
        .connect_with_mode(&request.params, initial_mode)
        .await
        .map_err(|e| e.to_string())?;
    let first_channel = conn
        .data_plane
        .take()
        .ok_or("Local Shell 连接缺少 DataPlane")?;
    let factory = conn
        .channel_factory
        .take()
        .ok_or("Local Shell 连接缺少子终端工厂")?;
    let session_name = request.name.clone().unwrap_or_else(|| "Shell".into());
    let parent_id = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "local-shell".into(),
                endpoint: request.endpoint.clone(),
                params: request.params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: request.send_bar_enabled.unwrap_or(false),
                id_override: request.session_id.clone(),
            },
            ContainerSessionRuntime {
                service: None,
                file_transfer: None,
                channel_factory: Some(factory),
                io: None,
                attachment: None,
                teardown_delay: std::time::Duration::ZERO,
            },
        )?
    };

    let channel_id = create_terminal_sub_channel(
        &app,
        &state,
        &parent_id,
        first_channel,
        initial_mode == ChannelOpenMode::Elevated,
        true,
    )
    .await
    .inspect_err(|error| {
        log::error!("Local Shell 首个子会话创建失败: {error}");
        if let Ok(mut store) = state.session_store.lock() {
            if let Err(cleanup_error) = store.close_session(&parent_id) {
                log::warn!(
                    "Local Shell 首个子会话失败后的父会话清理也失败: {}",
                    cleanup_error
                );
            }
        }
    })?;

    let connected_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": parent_id,
            "endpoint": request.endpoint,
            "connection_type": "local-shell",
            "plugin_id": "local-shell",
            "name": session_name,
            "params": request.params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "send_bar_enabled": false,
            "is_container": true,
        }),
    );
    log::info!(
        "Local Shell 父会话已连接: {} (child: {})",
        parent_id,
        channel_id
    );
    Ok(parent_id)
}
