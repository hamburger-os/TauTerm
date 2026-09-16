pub mod backend;
pub mod commands;
pub mod config;
pub mod error;
pub mod model;
pub mod runtime;
pub mod worker;

pub const PLUGIN_ID: &str = "rtt";

use crate::kernel::plugin_adapter::{SessionAttach, SessionService};
use crate::kernel::plugin_runtime::SessionRuntimeRegistry;
use crate::kernel::session_store::{ContainerSessionCreateOptions, ContainerSessionRuntime};
use crate::plugin_application::{
    unchanged_session_config, ConnectSessionRequest, SessionConnectFuture, SessionConfigHandler,
};
use crate::AppState;
use error::RttError;
use runtime::RttRuntime;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct RttPlugin {
    runtimes: SessionRuntimeRegistry<RttRuntime>,
}

impl RttPlugin {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<RttRuntime>> {
        self.runtimes.get(session_id)
    }
}

struct RuntimeAttach {
    runtime: Arc<RttRuntime>,
    runtimes: SessionRuntimeRegistry<RttRuntime>,
}

impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }

    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}

fn validate_session_config(params: &Value) -> Result<(), String> {
    config::RttConfig::from_params(params)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn default_session_name(params: &Value, _endpoint: &str) -> Result<String, String> {
    validate_session_config(params)?;
    Ok("RTT 调试助手".to_string())
}

pub(crate) fn session_config_handler() -> SessionConfigHandler {
    SessionConfigHandler {
        validate: Some(validate_session_config),
        prepare: unchanged_session_config,
        default_name: Some(default_session_name),
        sanitize_saved: None,
        delete: None,
    }
}

pub(crate) fn session_connector(
    app: AppHandle,
    request: ConnectSessionRequest,
) -> SessionConnectFuture {
    Box::pin(async move {
        let state: State<'_, AppState> = app.state();
        connect_session(app.clone(), state, request).await
    })
}

async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let ConnectSessionRequest {
        endpoint,
        params,
        name,
        session_id,
        ..
    } = request;
    let config = config::RttConfig::from_params(&params).map_err(|error| error.to_string())?;
    let plugin = state.plugin::<RttPlugin>(PLUGIN_ID);
    let runtime = Arc::new(RttRuntime::new(config));
    let session_name = name.unwrap_or_else(|| "RTT 调试助手".to_string());

    let new_session_id = {
        let mut store = state.session_store.lock().map_err(|error| error.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: PLUGIN_ID.into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: false,
                id_override: session_id,
            },
            ContainerSessionRuntime {
                service: Some(runtime.clone() as Arc<dyn SessionService>),
                file_transfer: None,
                channel_factory: None,
                io: None,
                attachment: Some(Arc::new(RuntimeAttach {
                    runtime: runtime.clone(),
                    runtimes: plugin.runtimes.clone(),
                })),
                teardown_delay: std::time::Duration::ZERO,
            },
        )?
    };

    let start_runtime = runtime.clone();
    let start_app = app.clone();
    let start_session_id = new_session_id.clone();
    let start_result = tokio::task::spawn_blocking(move || {
        start_runtime.start(start_app, &start_session_id)
    })
    .await
    .map_err(|error| format!("RTT worker 启动任务失败: {error}"))?;

    if let Err(error) = start_result {
        cleanup_failed_session(app.clone(), new_session_id.clone()).await;
        return Err(format_rtt_connect_error(error));
    }

    let connected_at = {
        let store = state.session_store.lock().map_err(|error| error.to_string())?;
        store
            .get_session(&new_session_id)
            .and_then(|handle| handle.connected_at)
    };
    let _ = app.emit(
        "session-connected",
        json!({
            "session_id": new_session_id,
            "endpoint": endpoint,
            "connection_type": PLUGIN_ID,
            "plugin_id": PLUGIN_ID,
            "name": session_name,
            "params": params,
            "connected_at": connected_at,
            "transfer_enabled": false,
            "transfer_protocol": Value::Null,
            "send_bar_enabled": false,
        }),
    );
    Ok(new_session_id)
}

async fn cleanup_failed_session(app: AppHandle, session_id: String) {
    let _ = tokio::task::spawn_blocking(move || {
        let state: State<'_, AppState> = app.state();
        if let Ok(mut store) = state.session_store.lock() {
            let _ = store.close_session(&session_id);
        }
    })
    .await;
}

fn format_rtt_connect_error(error: RttError) -> String {
    format!("{}: {}", error.code.as_str(), error.message)
}
