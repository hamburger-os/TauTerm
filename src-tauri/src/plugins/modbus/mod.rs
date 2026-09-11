pub mod client;
pub mod codec;
pub mod config;
pub mod data_model;
pub mod polling;
pub mod server;
pub mod value;

use std::any::Any;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::channel::error::SessionError;
use crate::commands::ConnectSessionRequest;
use crate::kernel::plugin_adapter::{
    ContentType, IoStrategy, ProtocolAdapter, ProtocolConnection, SideChannel,
};
use crate::kernel::session_store::ContainerSessionCreateOptions;
use crate::transport::runtime::DataPlaneRuntime;
use crate::transport::serial::open_serial;
use crate::transport::tcp::connect_tcp;
use crate::AppState;

use client::{ModbusClient, TransactionResult};
use codec::ModbusRequest;
use config::{ModbusConfig, ModbusMode, ModbusRole};
use polling::{WatchRow, WatchScheduler, WatchValue};
use server::ModbusServer;

pub struct ModbusSideChannel {
    pub config: ModbusConfig,
    pub client: Option<Arc<ModbusClient>>,
    pub server: Option<Arc<ModbusServer>>,
    pub watch: Option<Arc<WatchScheduler>>,
}

impl SideChannel for ModbusSideChannel {
    fn as_any(&self) -> &dyn Any { self }
    fn shutdown(&self) {
        if let Some(watch)=&self.watch { watch.stop(); }
        if let Some(client)=&self.client { client.shutdown(); }
        if let Some(server)=&self.server { server.shutdown(); }
    }
}

pub struct ModbusAdapter;
impl ModbusAdapter { pub fn new()->Self{Self} }

#[async_trait::async_trait]
impl ProtocolAdapter for ModbusAdapter {
    async fn connect(&self,_endpoint:&str,params:&Value)->Result<ProtocolConnection,SessionError>{
        let config:ModbusConfig=serde_json::from_value(params.clone()).map_err(|e|SessionError::Other(format!("Modbus 配置解析失败: {e}")))?;
        config.validate().map_err(SessionError::Other)?;
        let (client,server,watch)=match config.role {
            ModbusRole::Client=>{
                let runtime=match config.mode {
                    ModbusMode::Rtu|ModbusMode::Ascii=>{
                        let driver=open_serial(&config.serial_port,&config.serial).map_err(|e|SessionError::ConnectionFailed{reason:e.to_string()})?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                    ModbusMode::Tcp=>{
                        let driver=connect_tcp(&config.host,config.port,&config.tcp).map_err(|e|SessionError::ConnectionFailed{reason:e.to_string()})?;
                        DataPlaneRuntime::spawn(Box::new(driver))
                    }
                };
                let client=Arc::new(ModbusClient::new(config.clone(),runtime));
                let watch=Arc::new(WatchScheduler::new(client.clone()));
                (Some(client),None,Some(watch))
            }
            ModbusRole::Server=>{
                let server=Arc::new(ModbusServer::new(config.clone()).map_err(|reason|SessionError::ConnectionFailed{reason})?);
                server.start().map_err(SessionError::Other)?;
                (None,Some(server),None)
            }
        };
        Ok(ProtocolConnection{channel:None,comm_handle:None,side_channel:Some(Arc::new(ModbusSideChannel{config,client,server,watch})),channel_factory:None,teardown_delay:std::time::Duration::ZERO})
    }
    fn content_type(&self)->ContentType{ContentType::Custom}
    fn io_strategy(&self)->IoStrategy{IoStrategy::Sync}
}

pub async fn connect_session(app:AppHandle,state:State<'_,AppState>,request:ConnectSessionRequest)->Result<String,String>{
    let ConnectSessionRequest{endpoint,params,name,session_id,..}=request;
    let conn=state.modbus_adapter.connect(&endpoint,&params).await.map_err(|e|e.to_string())?;
    let side=conn.side_channel.ok_or("Modbus adapter returned no runtime")?;
    let config=side.as_any().downcast_ref::<ModbusSideChannel>().ok_or("Modbus runtime type mismatch")?.config.clone();
    let session_name=name.filter(|n|!n.trim().is_empty()).unwrap_or_else(||match config.mode{ModbusMode::Tcp=>format!("Modbus TCP {}:{}",config.host,config.port),ModbusMode::Rtu=>format!("Modbus RTU {}",config.serial_port),ModbusMode::Ascii=>format!("Modbus ASCII {}",config.serial_port)});
    let sid={let mut store=state.session_store.lock().map_err(|e|e.to_string())?;store.create_container_session(ContainerSessionCreateOptions{name:session_name.clone(),plugin_id:"modbus".into(),endpoint:endpoint.clone(),params:params.clone(),transfer_enabled:false,transfer_protocol:None,send_bar_enabled:false,id_override:session_id},Some(side),None,None)?};
    let _=app.emit("session-connected",serde_json::json!({"session_id":sid,"plugin_id":"modbus","connection_type":"modbus","content_type":"custom","endpoint":endpoint,"name":session_name,"params":params,"send_bar_enabled":false,"transfer_enabled":false}));
    Ok(sid)
}

fn runtime(state:&State<'_,AppState>,session_id:&str)->Result<Arc<dyn SideChannel>,String>{
    let store=state.session_store.lock().map_err(|e|e.to_string())?;
    store.get_side_channel(session_id).ok_or_else(||format!("Modbus 会话 {session_id} 未连接"))
}
fn with_modbus<T>(state:&State<'_,AppState>,session_id:&str,f:impl FnOnce(&ModbusSideChannel)->Result<T,String>)->Result<T,String>{let arc=runtime(state,session_id)?;let side=arc.as_any().downcast_ref::<ModbusSideChannel>().ok_or("会话不是 Modbus")?;f(side)}

#[tauri::command]
pub fn modbus_execute(state:State<'_,AppState>,session_id:String,request:ModbusRequest)->Result<TransactionResult,String>{
    with_modbus(&state,&session_id,|side|side.client.as_ref().map(|client|client.execute(request)).ok_or("Modbus server 会话不能发起 client transaction".into()))
}

#[derive(Serialize)]
pub struct ModbusStatus { pub role:ModbusRole,pub mode:ModbusMode,pub running:bool,pub unit_id:u8 }
#[tauri::command]
pub fn modbus_status(state:State<'_,AppState>,session_id:String)->Result<ModbusStatus,String>{with_modbus(&state,&session_id,|side|Ok(ModbusStatus{role:side.config.role,mode:side.config.mode,running:side.client.is_some()||side.server.as_ref().is_some_and(|s|s.is_running()),unit_id:side.config.unit_id}))}

#[tauri::command]
pub fn modbus_watch_set(state:State<'_,AppState>,session_id:String,rows:Vec<WatchRow>)->Result<(),String>{with_modbus(&state,&session_id,|side|side.watch.as_ref().ok_or("watch table requires client role")?.set_rows(rows))}
#[tauri::command]
pub fn modbus_watch_start(state:State<'_,AppState>,session_id:String)->Result<(),String>{with_modbus(&state,&session_id,|side|{side.watch.as_ref().ok_or("watch table requires client role")?.start();Ok(())})}
#[tauri::command]
pub fn modbus_watch_stop(state:State<'_,AppState>,session_id:String)->Result<(),String>{with_modbus(&state,&session_id,|side|{side.watch.as_ref().ok_or("watch table requires client role")?.stop();Ok(())})}
#[tauri::command]
pub fn modbus_watch_values(state:State<'_,AppState>,session_id:String)->Result<Vec<WatchValue>,String>{with_modbus(&state,&session_id,|side|Ok(side.watch.as_ref().ok_or("watch table requires client role")?.values()))}

#[derive(serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ServerArea{Coil,DiscreteInput,HoldingRegister,InputRegister}
#[tauri::command]
pub fn modbus_server_set_value(state:State<'_,AppState>,session_id:String,area:ServerArea,address:u16,value:u16)->Result<(),String>{with_modbus(&state,&session_id,|side|{let server=side.server.as_ref().ok_or("server data model requires server role")?;match area{ServerArea::Coil=>server.model.set_coil(address,value!=0),ServerArea::DiscreteInput=>server.model.set_discrete_input(address,value!=0),ServerArea::HoldingRegister=>server.model.set_holding_register(address,value),ServerArea::InputRegister=>server.model.set_input_register(address,value)}Ok(())})}
#[tauri::command]
pub fn modbus_server_snapshot(state:State<'_,AppState>,session_id:String)->Result<data_model::DataModelSnapshot,String>{with_modbus(&state,&session_id,|side|Ok(side.server.as_ref().ok_or("server data model requires server role")?.model.snapshot()))}
