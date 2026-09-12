pub mod client;
pub mod codec;
pub mod config;
pub mod data_model;
pub mod polling;
pub mod server;
pub mod value;

use std::any::Any;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::commands::ConnectSessionRequest;
use crate::kernel::plugin_adapter::ContentType;
use crate::kernel::plugin_adapter::{ProtocolAdapter, ProtocolConnection, SideChannel};
use crate::kernel::session_store::{ContainerSessionCreateOptions, SessionStore};
use crate::session::SessionError;
use crate::transport::runtime::DataPlaneRuntime;
use crate::transport::serial::open_serial;
use crate::transport::tcp::connect_tcp;
use crate::AppState;

use client::{ModbusClient, TransactionResult};
use codec::ModbusRequest;
use config::{ModbusConfig, ModbusMode, ModbusRole, ServerFaultConfig};
use data_model::DataModelSnapshot;
use polling::{WatchRow, WatchScheduler, WatchValue};
use server::ModbusServer;

pub struct ModbusSideChannel {
    pub config: ModbusConfig,
    pub client: Option<Arc<ModbusClient>>,
    pub server: Option<Arc<ModbusServer>>,
    pub watch: Option<Arc<WatchScheduler>>,
}

impl SideChannel for ModbusSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        if let Some(watch) = &self.watch {
            watch.stop();
        }
        if let Some(client) = &self.client {
            client.shutdown();
        }
        if let Some(server) = &self.server {
            server.shutdown();
        }
    }
}

pub struct ModbusAdapter;

impl ModbusAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for ModbusAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let config: ModbusConfig = serde_json::from_value(params.clone())
            .map_err(|error| SessionError::Other(format!("Modbus 配置解析失败: {error}")))?;
        config.validate().map_err(SessionError::Other)?;
        let initial_watch_rows = parse_watch_rows(params).map_err(SessionError::Other)?;
        let initial_server_model = parse_server_model(params).map_err(SessionError::Other)?;

        let (client, server, watch) = match config.role {
            ModbusRole::Client => {
                let runtime = match config.mode {
                    ModbusMode::Rtu | ModbusMode::Ascii => {
                        let driver = open_serial(&config.serial_port, &config.serial).map_err(
                            |error| SessionError::ConnectionFailed {
                                reason: error.to_string(),
                            },
                        )?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                    ModbusMode::Tcp => {
                        let driver = connect_tcp(&config.host, config.port, &config.tcp)
                            .map_err(|error| SessionError::ConnectionFailed {
                                reason: error.to_string(),
                            })?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                };
                let client = Arc::new(ModbusClient::new(config.clone(), runtime));
                let watch = Arc::new(WatchScheduler::new(client.clone()));
                watch
                    .set_rows(initial_watch_rows)
                    .map_err(SessionError::Other)?;
                (Some(client), None, Some(watch))
            }
            ModbusRole::Server => {
                let server = Arc::new(
                    ModbusServer::new(config.clone())
                        .map_err(|reason| SessionError::ConnectionFailed { reason })?,
                );
                if let Some(snapshot) = initial_server_model {
                    apply_server_snapshot(&server, snapshot);
                }
                server.start().map_err(SessionError::Other)?;
                (None, Some(server), None)
            }
        };

        Ok(ProtocolConnection {
            data_plane: None,
            side_channel: Some(Arc::new(ModbusSideChannel {
                config,
                client,
                server,
                watch,
            })),
            channel_factory: None,
            on_attached: None,
            teardown_delay: std::time::Duration::ZERO,
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Custom
    }
}

fn parse_watch_rows(params: &Value) -> Result<Vec<WatchRow>, String> {
    let Some(value) = params.get("watch_rows") else {
        return Ok(Vec::new());
    };
    let rows: Vec<WatchRow> =
        serde_json::from_value(value.clone()).map_err(|error| format!("watch_rows 无效: {error}"))?;
    WatchScheduler::validate_rows(&rows)?;
    Ok(rows)
}

fn parse_server_model(params: &Value) -> Result<Option<DataModelSnapshot>, String> {
    params
        .get("server_model")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("server_model 无效: {error}"))
}

fn apply_server_snapshot(server: &ModbusServer, snapshot: DataModelSnapshot) {
    for (address, value) in snapshot.coils {
        server.model.set_coil(address, value);
    }
    for (address, value) in snapshot.discrete_inputs {
        server.model.set_discrete_input(address, value);
    }
    for (address, value) in snapshot.holding_registers {
        server.model.set_holding_register(address, value);
    }
    for (address, value) in snapshot.input_registers {
        server.model.set_input_register(address, value);
    }
}

pub async fn connect_session(
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
    let conn = state
        .modbus_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|error| error.to_string())?;
    let side = conn
        .side_channel
        .ok_or("Modbus adapter returned no runtime")?;
    let config = side
        .as_any()
        .downcast_ref::<ModbusSideChannel>()
        .ok_or("Modbus runtime type mismatch")?
        .config
        .clone();
    let session_name = name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match config.mode {
            ModbusMode::Tcp => format!("Modbus TCP {}:{}", config.host, config.port),
            ModbusMode::Rtu => format!("Modbus RTU {}", config.serial_port),
            ModbusMode::Ascii => format!("Modbus ASCII {}", config.serial_port),
        });

    let session_id = {
        let mut store = state
            .session_store
            .lock()
            .map_err(|error| error.to_string())?;
        store.create_container_session(
            ContainerSessionCreateOptions {
                name: session_name.clone(),
                plugin_id: "modbus".into(),
                endpoint: endpoint.clone(),
                params: params.clone(),
                transfer_enabled: false,
                transfer_protocol: None,
                send_bar_enabled: false,
                id_override: session_id,
            },
            Some(side),
            None,
            None,
        )?
    };

    let _ = app.emit(
        "session-connected",
        serde_json::json!({
            "session_id": session_id,
            "plugin_id": "modbus",
            "connection_type": "modbus",
            "content_type": "custom",
            "endpoint": endpoint,
            "name": session_name,
            "params": params,
            "send_bar_enabled": false,
            "transfer_enabled": false
        }),
    );
    Ok(session_id)
}

fn runtime(state: &State<'_, AppState>, session_id: &str) -> Result<Arc<dyn SideChannel>, String> {
    let store = state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?;
    store
        .get_side_channel(session_id)
        .ok_or_else(|| format!("Modbus 会话 {session_id} 未连接"))
}

fn with_modbus<T>(
    state: &State<'_, AppState>,
    session_id: &str,
    function: impl FnOnce(&ModbusSideChannel) -> Result<T, String>,
) -> Result<T, String> {
    let side = runtime(state, session_id)?;
    let modbus = side
        .as_any()
        .downcast_ref::<ModbusSideChannel>()
        .ok_or("会话不是 Modbus")?;
    function(modbus)
}

fn persist_param_if_saved(
    app: &AppHandle,
    session_id: &str,
    key: &str,
    value: Value,
) -> Result<(), String> {
    let path = SessionStore::sessions_file_path(app)?;
    let saved = SessionStore::load_from_disk(&path)?;
    if !saved.iter().any(|session| session.id == session_id) {
        return Ok(());
    }
    SessionStore::set_config_param_on_disk_transactional(
        app,
        session_id,
        key,
        value,
        || Ok(()),
    )
}

fn set_runtime_param(
    state: &State<'_, AppState>,
    session_id: &str,
    key: &str,
    value: Value,
) -> Result<(), String> {
    let mut store = state
        .session_store
        .lock()
        .map_err(|error| error.to_string())?;
    let handle = store
        .get_session_mut(session_id)
        .ok_or_else(|| store.session_not_found(session_id))?;
    let params = handle
        .params
        .as_object_mut()
        .ok_or("Modbus Session params 必须是 JSON object")?;
    params.insert(key.to_string(), value);
    Ok(())
}

#[derive(Deserialize)]
struct RawAduOperation {
    data: Vec<u8>,
    #[serde(default = "default_wait_response")]
    wait_response: bool,
    #[serde(default = "default_quiet_period_ms")]
    quiet_period_ms: u64,
}

fn default_wait_response() -> bool {
    true
}

fn default_quiet_period_ms() -> u64 {
    50
}

#[tauri::command]
pub fn modbus_execute(
    state: State<'_, AppState>,
    session_id: String,
    request: Value,
) -> Result<TransactionResult, String> {
    with_modbus(&state, &session_id, |side| {
        let client = side
            .client
            .as_ref()
            .ok_or("Modbus server 会话不能发起 client transaction")?;
        if request.get("kind").and_then(Value::as_str) == Some("raw_adu") {
            let raw: RawAduOperation = serde_json::from_value(request)
                .map_err(|error| format!("Raw ADU 参数无效: {error}"))?;
            return Ok(client.execute_raw_adu(raw.data, raw.wait_response, raw.quiet_period_ms));
        }
        let request: ModbusRequest =
            serde_json::from_value(request).map_err(|error| format!("Modbus 请求无效: {error}"))?;
        Ok(client.execute(request))
    })
}

#[derive(Serialize)]
pub struct ModbusStatus {
    pub role: ModbusRole,
    pub mode: ModbusMode,
    pub running: bool,
    pub unit_id: u8,
    pub transactions: Vec<TransactionResult>,
    pub watch_rows: Vec<WatchRow>,
    pub server_fault: Option<ServerFaultConfig>,
}

#[tauri::command]
pub fn modbus_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ModbusStatus, String> {
    with_modbus(&state, &session_id, |side| {
        Ok(ModbusStatus {
            role: side.config.role,
            mode: side.config.mode,
            running: side.client.is_some()
                || side
                    .server
                    .as_ref()
                    .is_some_and(|server| server.is_running()),
            unit_id: side.config.unit_id,
            transactions: if let Some(client) = &side.client {
                client.history()
            } else {
                side.server
                    .as_ref()
                    .map_or_else(Vec::new, |server| server.history())
            },
            watch_rows: side
                .watch
                .as_ref()
                .map_or_else(Vec::new, |watch| watch.rows()),
            server_fault: side.server.as_ref().map(|server| server.fault()),
        })
    })
}

#[tauri::command]
pub fn modbus_watch_set(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    rows: Vec<WatchRow>,
) -> Result<(), String> {
    WatchScheduler::validate_rows(&rows)?;
    let value = serde_json::to_value(&rows).map_err(|error| error.to_string())?;
    persist_param_if_saved(&app, &session_id, "watch_rows", value.clone())?;
    with_modbus(&state, &session_id, |side| {
        side.watch
            .as_ref()
            .ok_or("watch table requires client role")?
            .set_rows(rows)
    })?;
    set_runtime_param(&state, &session_id, "watch_rows", value)
}

#[tauri::command]
pub fn modbus_watch_start(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    with_modbus(&state, &session_id, |side| {
        side.watch
            .as_ref()
            .ok_or("watch table requires client role")?
            .start();
        Ok(())
    })
}

#[tauri::command]
pub fn modbus_watch_stop(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    with_modbus(&state, &session_id, |side| {
        side.watch
            .as_ref()
            .ok_or("watch table requires client role")?
            .stop();
        Ok(())
    })
}

#[tauri::command]
pub fn modbus_watch_values(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<WatchValue>, String> {
    with_modbus(&state, &session_id, |side| {
        Ok(side
            .watch
            .as_ref()
            .ok_or("watch table requires client role")?
            .values())
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerArea {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
}

#[tauri::command]
pub fn modbus_server_set_value(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    area: Option<ServerArea>,
    address: Option<u16>,
    value: Option<u16>,
    fault: Option<ServerFaultConfig>,
) -> Result<(), String> {
    let has_area = area.is_some();
    let has_fault = fault.is_some();
    if !has_area && !has_fault {
        return Err("either area or fault must be provided".into());
    }

    let (snapshot, persisted_fault) = with_modbus(&state, &session_id, |side| {
        let server = side
            .server
            .as_ref()
            .ok_or("server data model requires server role")?;
        if let Some(fault) = fault {
            server.set_fault(fault)?;
        }
        if let Some(area) = area {
            let address = address.ok_or("address is required when area is set")?;
            let value = value.ok_or("value is required when area is set")?;
            match area {
                ServerArea::Coil => server.model.set_coil(address, value != 0),
                ServerArea::DiscreteInput => server.model.set_discrete_input(address, value != 0),
                ServerArea::HoldingRegister => server.model.set_holding_register(address, value),
                ServerArea::InputRegister => server.model.set_input_register(address, value),
            }
        }
        Ok((
            has_area.then(|| server.model.snapshot()),
            has_fault.then(|| server.fault()),
        ))
    })?;

    if let Some(snapshot) = snapshot {
        let value = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
        persist_param_if_saved(&app, &session_id, "server_model", value.clone())?;
        set_runtime_param(&state, &session_id, "server_model", value)?;
    }
    if let Some(fault) = persisted_fault {
        let value = serde_json::to_value(fault).map_err(|error| error.to_string())?;
        persist_param_if_saved(&app, &session_id, "server_fault", value.clone())?;
        set_runtime_param(&state, &session_id, "server_fault", value)?;
    }
    Ok(())
}

#[tauri::command]
pub fn modbus_server_snapshot(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<DataModelSnapshot, String> {
    with_modbus(&state, &session_id, |side| {
        Ok(side
            .server
            .as_ref()
            .ok_or("server data model requires server role")?
            .model
            .snapshot())
    })
}