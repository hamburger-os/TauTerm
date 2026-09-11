use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::plugins::modbus::client::{ModbusClient, TransactionResult, TransactionStatus};
use crate::plugins::modbus::codec::ModbusRequest;
use crate::plugins::modbus::value::{decode_register_bytes, ValueFormat};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRow {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub request: ModbusRequest,
    #[serde(default="default_period")]
    pub period_ms: u64,
    #[serde(default)]
    pub format: Option<ValueFormat>,
}
fn default_period()->u64{1000}

#[derive(Debug, Clone, Serialize)]
pub struct WatchValue {
    pub row_id: String,
    pub status: String,
    pub value: Option<serde_json::Value>,
    pub raw: Vec<u8>,
    pub latency_ms: u128,
    pub message: Option<String>,
    pub updated_at_ms: u64,
}

pub struct WatchScheduler {
    client: Arc<ModbusClient>,
    rows: Arc<Mutex<Vec<WatchRow>>>,
    values: Arc<Mutex<HashMap<String,WatchValue>>>,
    running: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatchScheduler {
    pub fn new(client:Arc<ModbusClient>)->Self{Self{client,rows:Arc::new(Mutex::new(Vec::new())),values:Arc::new(Mutex::new(HashMap::new())),running:Arc::new(AtomicBool::new(false)),worker:Mutex::new(None)}}
    pub fn set_rows(&self,rows:Vec<WatchRow>)->Result<(),String>{for row in &rows{if row.period_ms<20||row.period_ms>86_400_000{return Err(format!("watch row {} period must be 20..=86400000 ms",row.id));}}*self.rows.lock().map_err(|e|e.to_string())?=rows;Ok(())}
    pub fn rows(&self)->Vec<WatchRow>{self.rows.lock().unwrap_or_else(|e|e.into_inner()).clone()}
    pub fn values(&self)->Vec<WatchValue>{self.values.lock().unwrap_or_else(|e|e.into_inner()).values().cloned().collect()}
    pub fn start(&self){if self.running.swap(true,Ordering::AcqRel){return;}let running=self.running.clone();let rows=self.rows.clone();let values=self.values.clone();let client=self.client.clone();let worker=std::thread::spawn(move||{let mut next:HashMap<String,Instant>=HashMap::new();while running.load(Ordering::Acquire){let snapshot=rows.lock().unwrap_or_else(|e|e.into_inner()).clone();let now=Instant::now();for row in snapshot.into_iter().filter(|r|r.enabled){let due=next.get(&row.id).copied().unwrap_or(now);if now<due{continue;} // no backlog: schedule from completion/current time, never from missed ticks
let result=client.execute(row.request.clone());let value=watch_value(&row,&result);values.lock().unwrap_or_else(|e|e.into_inner()).insert(row.id.clone(),value);next.insert(row.id.clone(),Instant::now()+Duration::from_millis(row.period_ms));}std::thread::sleep(Duration::from_millis(10));}});*self.worker.lock().unwrap_or_else(|e|e.into_inner())=Some(worker);}
    pub fn stop(&self){self.running.store(false,Ordering::Release);if let Some(worker)=self.worker.lock().unwrap_or_else(|e|e.into_inner()).take(){let _=worker.join();}}
}
impl Drop for WatchScheduler{fn drop(&mut self){self.running.store(false,Ordering::Release);}}

fn watch_value(row:&WatchRow,result:&TransactionResult)->WatchValue{
    let raw=if matches!(result.status,TransactionStatus::Success)&&!result.response_pdu.is_empty(){extract_data(&result.response_pdu)}else{Vec::new()};
    let value=row.format.as_ref().and_then(|format|decode_register_bytes(&raw,format).ok());
    WatchValue{row_id:row.id.clone(),status:format!("{:?}",result.status).to_ascii_lowercase(),value,raw,latency_ms:result.latency_ms,message:result.message.clone(),updated_at_ms:chrono::Utc::now().timestamp_millis().max(0) as u64}
}
fn extract_data(pdu:&[u8])->Vec<u8>{if pdu.len()>=2&&matches!(pdu[0],0x01|0x02|0x03|0x04|0x17){let count=pdu[1] as usize;if pdu.len()>=2+count{return pdu[2..2+count].to_vec();}}pdu.get(1..).unwrap_or_default().to_vec()}
