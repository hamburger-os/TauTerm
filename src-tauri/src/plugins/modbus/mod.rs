pub mod capability;
pub mod client;
pub mod codec;
pub mod config;
pub mod data_model;
pub mod polling;
pub mod response;
pub mod server;
pub mod value;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::commands::ConnectSessionRequest;
use crate::kernel::plugin_adapter::ContentType;
use crate::kernel::plugin_adapter::{
    ProtocolAdapter, ProtocolConnection, SessionAttach, SessionService,
};
use crate::kernel::session_store::{ContainerSessionCreateOptions, SessionStore};
use crate::session::SessionError;
use crate::transport::runtime::DataPlaneRuntime;
use crate::transport::serial::open_serial;
use crate::transport::tcp::connect_tcp;
use crate::AppState;

use client::{ModbusClient, TransactionHistoryBatch, TransactionResult, TransactionStatus};
use codec::ModbusRequest;
use config::{
    validate_fault, ModbusConfig, ModbusEndpointConfig, ModbusMode, ModbusRole, ServerFaultConfig,
    ValidatedModbusConfig,
};
use data_model::DataModelSnapshot;
use polling::{WatchRow, WatchScheduler, WatchValue};
use server::ModbusServer;

fn runtime_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<ModbusRuntime>>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<ModbusRuntime>>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn runtime(session_id: &str) -> Option<Arc<ModbusRuntime>> {
    runtime_registry().lock().ok()?.get(session_id).cloned()
}

struct RuntimeAttach {
    runtime: Arc<ModbusRuntime>,
}

impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        if let Ok(mut map) = runtime_registry().lock() {
            map.insert(session_id.to_string(), self.runtime.clone());
        }
    }

    fn on_detached(&self, session_id: &str) {
        if let Ok(mut map) = runtime_registry().lock() {
            map.remove(session_id);
        }
    }
}

pub struct ModbusRuntime {
    pub config: ValidatedModbusConfig,
    pub client: Option<Arc<ModbusClient>>,
    pub server: Option<Arc<ModbusServer>>,
    pub watch: Option<Arc<WatchScheduler>>,
}

impl SessionService for ModbusRuntime {
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

    pub fn runtime(&self, session_id: &str) -> Option<Arc<ModbusRuntime>> {
        runtime(session_id)
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for ModbusAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let wire_config: ModbusConfig = serde_json::from_value(params.clone())
            .map_err(|error| SessionError::Other(format!("Modbus 配置解析失败: {error}")))?;
        let config = wire_config.validated().map_err(SessionError::Other)?;
        let initial_watch_rows =
            parse_watch_rows(params, config.mode()).map_err(SessionError::Other)?;
        let initial_server_model = parse_server_model(params).map_err(SessionError::Other)?;

        let (client, server, watch) = match config.role() {
            ModbusRole::Client => {
                let runtime = match &config.endpoint {
                    ModbusEndpointConfig::Serial {
                        port, transport, ..
                    } => {
                        let driver = open_serial(port, transport).map_err(|error| {
                            SessionError::ConnectionFailed {
                                reason: error.to_string(),
                            }
                        })?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                    ModbusEndpointConfig::Tcp {
                        host,
                        port,
                        transport,
                    } => {
                        let driver = connect_tcp(host, *port, transport).map_err(|error| {
                            SessionError::ConnectionFailed {
                                reason: error.to_string(),
                            }
                        })?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                };
                let client = Arc::new(
                    ModbusClient::new(config.clone(), runtime).map_err(SessionError::Other)?,
                );
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

        let runtime = Arc::new(ModbusRuntime {
            config,
            client,
            server,
            watch,
        });
        Ok(ProtocolConnection {
            data_plane: None,
            service: Some(runtime.clone()),
            file_transfer: None,
            channel_factory: None,
            on_attached: Some(Arc::new(RuntimeAttach { runtime })),
            teardown_delay: std::time::Duration::ZERO,
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Custom
    }
}

fn parse_watch_rows(params: &Value, mode: ModbusMode) -> Result<Vec<WatchRow>, String> {
    let Some(value) = params.get("watch_rows") else {
        return Ok(Vec::new());
    };
    let rows: Vec<WatchRow> = serde_json::from_value(value.clone())
        .map_err(|error| format!("watch_rows 无效: {error}"))?;
    WatchScheduler::validate_rows(&rows, mode)?;
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
    let session_name = name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Modbus 调试助手".to_string());

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
            crate::kernel::session_store::ContainerSessionRuntime {
                service: conn.service,
                file_transfer: conn.file_transfer,
                channel_factory: conn.channel_factory,
                io: None,
                attachment: conn.on_attached,
                teardown_delay: conn.teardown_delay,
            },
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

fn with_modbus<T>(
    _state: &State<'_, AppState>,
    session_id: &str,
    function: impl FnOnce(&ModbusRuntime) -> Result<T, String>,
) -> Result<T, String> {
    let modbus = runtime(session_id).ok_or_else(|| format!("Modbus 会话 {session_id} 未连接"))?;
    function(&modbus)
}

fn persist_param_if_saved_transactional<F>(
    app: &AppHandle,
    session_id: &str,
    key: &str,
    value: Value,
    post_commit: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    let path = SessionStore::sessions_file_path(app)?;
    let saved = SessionStore::load_from_disk(&path)?;
    if saved.iter().any(|session| session.id == session_id) {
        SessionStore::set_config_param_on_disk_transactional(
            app,
            session_id,
            key,
            value,
            post_commit,
        )
    } else {
        post_commit()
    }
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
    let not_found = store.session_not_found(session_id);
    let handle = store.get_session_mut(session_id).ok_or(not_found)?;
    let params = handle
        .params
        .as_object_mut()
        .ok_or("Modbus Session params 必须是 JSON object")?;
    params.insert(key.to_string(), value);
    Ok(())
}

fn default_wait_response() -> bool {
    true
}

fn default_quiet_period_ms() -> u64 {
    50
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModbusOperation {
    Request {
        unit_id: u8,
        request: ModbusRequest,
    },
    RawAdu {
        data: Vec<u8>,
        #[serde(default = "default_wait_response")]
        wait_response: bool,
        #[serde(default = "default_quiet_period_ms")]
        quiet_period_ms: u64,
    },
}

#[tauri::command]
pub fn modbus_execute(
    state: State<'_, AppState>,
    session_id: String,
    operation: ModbusOperation,
) -> Result<TransactionResult, String> {
    with_modbus(&state, &session_id, |side| {
        let client = side
            .client
            .as_ref()
            .ok_or("Modbus server 会话不能发起 client transaction")?;
        Ok(match operation {
            ModbusOperation::Request { unit_id, request } => client.execute(unit_id, request),
            ModbusOperation::RawAdu {
                data,
                wait_response,
                quiet_period_ms,
            } => client.execute_raw_adu(data, wait_response, quiet_period_ms),
        })
    })
}

#[derive(Serialize)]
pub struct ModbusStatus {
    pub role: ModbusRole,
    pub mode: ModbusMode,
    pub running: bool,
    pub default_unit_id: u8,
    pub transactions: TransactionHistoryBatch,
    pub watch_rows: Vec<WatchRow>,
    pub watch_running: bool,
    pub watch_enabled: usize,
    pub watch_total: usize,
    pub last_status: Option<TransactionStatus>,
    pub last_unit_id: Option<u8>,
    pub last_latency_ms: Option<u128>,
    pub server_fault: Option<ServerFaultConfig>,
}

#[tauri::command]
pub fn modbus_status(
    state: State<'_, AppState>,
    session_id: String,
    after_sequence: Option<u64>,
    transaction_limit: Option<usize>,
    include_watch_rows: Option<bool>,
) -> Result<ModbusStatus, String> {
    with_modbus(&state, &session_id, |side| {
        let watch = side.watch.as_ref();
        let (watch_enabled, watch_total) = watch.map_or((0, 0), |watch| watch.counts());
        let rows = if include_watch_rows.unwrap_or(true) {
            watch.map_or_else(Vec::new, |watch| watch.rows())
        } else {
            Vec::new()
        };
        let cursor = after_sequence.unwrap_or(0);
        let limit = transaction_limit.unwrap_or(if after_sequence.is_some() { 250 } else { 1 });
        let transactions = if let Some(client) = &side.client {
            client.history_since(cursor, limit)
        } else {
            side.server
                .as_ref()
                .ok_or("Modbus runtime has no client or server")?
                .history_since(cursor, limit)
        };
        let last = if let Some(client) = &side.client {
            client.last_result()
        } else {
            side.server.as_ref().and_then(|server| server.last_result())
        };
        Ok(ModbusStatus {
            role: side.config.role(),
            mode: side.config.mode(),
            running: side.client.is_some()
                || side
                    .server
                    .as_ref()
                    .is_some_and(|server| server.is_running()),
            default_unit_id: side.config.unit_id,
            transactions,
            watch_rows: rows,
            watch_running: watch.is_some_and(|watch| watch.is_running()),
            watch_enabled,
            watch_total,
            last_status: last.as_ref().map(|result| result.status),
            last_unit_id: last.as_ref().map(|result| result.unit_id),
            last_latency_ms: last.as_ref().map(|result| result.latency_ms),
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
    let (mode, watch, previous_rows) = with_modbus(&state, &session_id, |side| {
        let watch = side
            .watch
            .as_ref()
            .ok_or("watch table requires client role")?
            .clone();
        Ok((side.config.mode(), watch.clone(), watch.rows()))
    })?;
    WatchScheduler::validate_rows(&rows, mode)?;
    let value = serde_json::to_value(&rows).map_err(|error| error.to_string())?;
    persist_param_if_saved_transactional(
        &app,
        &session_id,
        "watch_rows",
        value.clone(),
        || {
            watch.set_rows(rows)?;
            if let Err(error) = set_runtime_param(&state, &session_id, "watch_rows", value) {
                let _ = watch.set_rows(previous_rows);
                return Err(error);
            }
            Ok(())
        },
    )
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

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerArea {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
}

fn set_snapshot_point(
    snapshot: &mut DataModelSnapshot,
    area: ServerArea,
    address: u16,
    value: u16,
) {
    fn set<T: Copy>(points: &mut Vec<(u16, T)>, address: u16, value: T) {
        match points.binary_search_by_key(&address, |(current, _)| *current) {
            Ok(index) => points[index].1 = value,
            Err(index) => points.insert(index, (address, value)),
        }
    }

    match area {
        ServerArea::Coil => set(&mut snapshot.coils, address, value != 0),
        ServerArea::DiscreteInput => set(&mut snapshot.discrete_inputs, address, value != 0),
        ServerArea::HoldingRegister => set(&mut snapshot.holding_registers, address, value),
        ServerArea::InputRegister => set(&mut snapshot.input_registers, address, value),
    }
}

fn apply_server_point(server: &ModbusServer, area: ServerArea, address: u16, value: u16) {
    match area {
        ServerArea::Coil => server.model.set_coil(address, value != 0),
        ServerArea::DiscreteInput => server.model.set_discrete_input(address, value != 0),
        ServerArea::HoldingRegister => server.model.set_holding_register(address, value),
        ServerArea::InputRegister => server.model.set_input_register(address, value),
    }
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
    if area.is_some() == fault.is_some() {
        return Err("provide exactly one of area or fault".into());
    }

    let server = with_modbus(&state, &session_id, |side| {
        side.server
            .as_ref()
            .ok_or("server data model requires server role")
            .cloned()
    })?;

    if let Some(area) = area {
        let address = address.ok_or("address is required when area is set")?;
        let value = value.ok_or("value is required when area is set")?;
        let mut snapshot = server.model.snapshot();
        set_snapshot_point(&mut snapshot, area, address, value);
        let persisted = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
        persist_param_if_saved_transactional(
            &app,
            &session_id,
            "server_model",
            persisted.clone(),
            || set_runtime_param(&state, &session_id, "server_model", persisted),
        )?;
        apply_server_point(&server, area, address, value);
        return Ok(());
    }

    let fault = fault.expect("exclusive area/fault check");
    validate_fault(&fault)?;
    let persisted = serde_json::to_value(&fault).map_err(|error| error.to_string())?;
    persist_param_if_saved_transactional(
        &app,
        &session_id,
        "server_fault",
        persisted.clone(),
        || set_runtime_param(&state, &session_id, "server_fault", persisted),
    )?;
    server.set_fault(fault)
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
