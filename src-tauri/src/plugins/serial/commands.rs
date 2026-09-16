//! Serial application-layer Session connector.

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::kernel::plugin_adapter::ProtocolAdapter;
use crate::kernel::session_store::SessionCreateOptions;
use crate::plugin_application::{
    create_on_data_callback, ConnectSessionRequest, SessionConnectFuture,
};
use crate::session::DisconnectInfo;
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

/// 串口会话连接（新架构：SerialAdapter → Channel → SessionStore）
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
        ..
    } = request;
    let adapter =
        state.plugin::<crate::plugins::serial::SerialAdapter>(crate::plugins::serial::PLUGIN_ID);
    let conn = adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|error| error.to_string())?;

    let session_name = name.unwrap_or_default();
    let data_mode = params
        .get("data_mode")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .to_string();
    let encoding = params
        .get("encoding")
        .and_then(Value::as_str)
        .unwrap_or("utf-8")
        .to_string();
    let log_tx = state.log_engine.lock().map_err(|e| e.to_string())?.sender();
    let on_data = create_on_data_callback(&app, log_tx, data_mode.clone(), encoding);

    let app_disconnect = app.clone();
    let on_disconnect: Box<dyn Fn(String, DisconnectInfo) + Send> =
        Box::new(move |session_id, info| {
            if let Ok(mut store) = app_disconnect.state::<AppState>().session_store.lock() {
                store.mark_disconnected(&session_id);
            }
            let _ = app_disconnect.emit(
                "session-disconnected",
                serde_json::json!({
                    "session_id": session_id,
                    "reason": &info.reason,
                    "disconnect_info": &info,
                }),
            );
        });

    let transfer_enabled = transfer_enabled.unwrap_or(true);
    let transfer_protocol = transfer_protocol.unwrap_or_else(|| "ymodem".into());
    let send_bar_enabled = send_bar_enabled.unwrap_or(true);
    let sid = {
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.create_session(
            SessionCreateOptions {
                name: session_name.clone(),
                plugin_id: crate::plugins::serial::PLUGIN_ID.into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled,
                transfer_protocol: Some(transfer_protocol.clone()),
                send_bar_enabled,
                id_override: session_id,
            },
            conn,
            on_data,
            on_disconnect,
            app.clone(),
        )?
    };

    let virtual_endpoints = match adapter.runtime(&sid) {
        Some(runtime) => runtime
            .initialize_virtual_ports(&app, &sid, &params)
            .unwrap_or_else(|error| {
                log::warn!("Serial 虚拟串口 capability 初始化失败 (session={sid}): {error}");
                let _ = app.emit(
                    "virtual-port-failed",
                    serde_json::json!({
                        "session_id": sid,
                        "kind": "create_failed",
                        "reason": error,
                    }),
                );
                Vec::new()
            }),
        None => {
            log::warn!("Serial runtime 未注册 (session={sid})");
            Vec::new()
        }
    };

    let (actual_name, actual_params, connected_at) = {
        let store = state.session_store.lock().map_err(|e| e.to_string())?;
        store
            .get_session(&sid)
            .map(|handle| {
                (
                    handle.name.clone(),
                    handle.params.clone(),
                    handle.connected_at,
                )
            })
            .unwrap_or((session_name, params.clone(), None))
    };

    log::info!(
        "会话已连接: {} @ {} (data_mode={})",
        actual_name,
        endpoint,
        data_mode
    );
    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": sid,
            "endpoint": endpoint,
            "connection_type": crate::plugins::serial::PLUGIN_ID,
            "plugin_id": crate::plugins::serial::PLUGIN_ID,
            "name": actual_name,
            "params": actual_params,
            "connected_at": connected_at,
            "transfer_enabled": transfer_enabled,
            "transfer_protocol": transfer_protocol,
            "send_bar_enabled": send_bar_enabled,
            "virtual_endpoints": virtual_endpoints,
        }),
    );
    Ok(sid)
}
