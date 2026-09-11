use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::plugins::modbus::codec;
use crate::plugins::modbus::config::{ModbusConfig, ModbusMode};
use crate::plugins::modbus::data_model::ModbusDataModel;
use crate::transport::runtime::DataPlaneEvent;
use crate::transport::serial::open_serial;
use crate::transport::tcp::TcpListenerTransport;
use crate::transport::DataPlaneRuntime;

pub struct ModbusServer {
    config: ModbusConfig,
    pub model: Arc<ModbusDataModel>,
    running: Arc<AtomicBool>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
    serial_runtime: Mutex<Option<DataPlaneRuntime>>,
    listener: Mutex<Option<TcpListenerTransport>>,
}

impl ModbusServer {
    pub fn new(config: ModbusConfig) -> Result<Self, String> {
        config.validate()?;
        let (serial_runtime, listener) = match config.mode {
            ModbusMode::Rtu | ModbusMode::Ascii => {
                let driver = open_serial(&config.serial_port, &config.serial).map_err(|e| e.to_string())?;
                (Some(DataPlaneRuntime::spawn(Box::new(driver))), None)
            }
            ModbusMode::Tcp => {
                let listener = TcpListenerTransport::bind(&config.host, config.port, config.tcp.clone())
                    .map_err(|e| e.to_string())?;
                (None, Some(listener))
            }
        };
        Ok(Self {
            config,
            model: Arc::new(ModbusDataModel::default()),
            running: Arc::new(AtomicBool::new(false)),
            workers: Arc::new(Mutex::new(Vec::new())),
            serial_runtime: Mutex::new(serial_runtime),
            listener: Mutex::new(listener),
        })
    }

    pub fn start(&self) -> Result<(), String> {
        if self.running.swap(true, Ordering::AcqRel) { return Ok(()); }
        match self.config.mode {
            ModbusMode::Rtu | ModbusMode::Ascii => self.start_serial(),
            ModbusMode::Tcp => self.start_tcp(),
        }
    }

    fn start_serial(&self) -> Result<(), String> {
        let handle = self.serial_runtime.lock().map_err(|e|e.to_string())?
            .as_ref().ok_or("serial runtime unavailable")?.handle.clone();
        let events = handle.subscribe().map_err(|e|e.to_string())?;
        let running=self.running.clone();
        let config=self.config.clone();
        let model=self.model.clone();
        let worker=std::thread::spawn(move || {
            let mut buffer=Vec::new();
            while running.load(Ordering::Acquire) {
                let wait=if buffer.is_empty(){Duration::from_millis(50)}else{match config.mode{ModbusMode::Rtu=>config.rtu_frame_gap(),ModbusMode::Ascii=>Duration::from_millis(50),ModbusMode::Tcp=>unreachable!()}};
                match events.recv_timeout(wait) {
                    Ok(DataPlaneEvent::Closed(_))=>break,
                    Ok(DataPlaneEvent::Data(data))=>{
                        buffer.extend_from_slice(&data);
                        if config.mode==ModbusMode::Ascii {
                            while let Some(end)=buffer.windows(2).position(|w|w==b"\r\n") {
                                let frame:Vec<u8>=buffer.drain(..end+2).collect();
                                process_serial_frame(&handle,&config,&model,&frame);
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !buffer.is_empty() && config.mode==ModbusMode::Rtu=>{
                        let frame=std::mem::take(&mut buffer);
                        process_serial_frame(&handle,&config,&model,&frame);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
                }
            }
        });
        self.workers.lock().map_err(|e|e.to_string())?.push(worker);
        Ok(())
    }

    fn start_tcp(&self) -> Result<(), String> {
        let listener=self.listener.lock().map_err(|e|e.to_string())?.take().ok_or("TCP listener unavailable")?;
        let running=self.running.clone();
        let workers=self.workers.clone();
        let model=self.model.clone();
        let config=self.config.clone();
        let active=Arc::new(AtomicUsize::new(0));
        let listener_worker=std::thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok(Some((driver,_peer)))=>{
                        let max=config.server_max_clients;
                        if max>0 && active.load(Ordering::Acquire)>=max { continue; }
                        active.fetch_add(1,Ordering::AcqRel);
                        let running_peer=running.clone();
                        let model_peer=model.clone();
                        let config_peer=config.clone();
                        let active_peer=active.clone();
                        let peer=std::thread::spawn(move || {
                            run_tcp_peer(driver,running_peer,model_peer,config_peer);
                            active_peer.fetch_sub(1,Ordering::AcqRel);
                        });
                        if let Ok(mut list)=workers.lock(){list.push(peer);}
                    }
                    Ok(None)=>std::thread::sleep(Duration::from_millis(20)),
                    Err(error)=>{log::warn!("Modbus TCP accept failed: {error}");std::thread::sleep(Duration::from_millis(100));}
                }
            }
        });
        self.workers.lock().map_err(|e|e.to_string())?.push(listener_worker);
        Ok(())
    }

    pub fn shutdown(&self) {
        self.running.store(false,Ordering::Release);
        if let Ok(mut runtime)=self.serial_runtime.lock(){if let Some(runtime)=runtime.take(){runtime.join();}}
        let handles=if let Ok(mut workers)=self.workers.lock(){std::mem::take(&mut *workers)}else{Vec::new()};
        for handle in handles { let _=handle.join(); }
    }

    pub fn is_running(&self)->bool{self.running.load(Ordering::Acquire)}
}

fn process_serial_frame(handle:&crate::transport::DataPlaneHandle,config:&ModbusConfig,model:&ModbusDataModel,frame:&[u8]){
    let decoded=match config.mode {
        ModbusMode::Rtu=>codec::rtu::decode(frame),
        ModbusMode::Ascii=>codec::ascii::decode(frame),
        ModbusMode::Tcp=>return,
    };
    let (unit,pdu)=match decoded{Ok(v)=>v,Err(error)=>{log::debug!("Ignoring malformed Modbus serial frame: {error}");return;}};
    let broadcast=unit==0;
    if !broadcast && unit!=config.unit_id{return;}
    let request=match codec::decode_request(&pdu){Ok(v)=>v.request,Err(_)=>{if !broadcast{let response=exception_pdu(pdu.first().copied().unwrap_or(0),0x03);send_serial(handle,config,unit,&response);}return;}};
    if broadcast && !request.is_write(){return;}
    let response=execute_with_fault(config,model,&request);
    if broadcast{return;}
    if let Some(response)=response{send_serial(handle,config,unit,&response);}
}

fn send_serial(handle:&crate::transport::DataPlaneHandle,config:&ModbusConfig,unit:u8,pdu:&[u8]){
    let frame=match config.mode{ModbusMode::Rtu=>codec::rtu::encode(unit,pdu),ModbusMode::Ascii=>codec::ascii::encode(unit,pdu),ModbusMode::Tcp=>return};
    if let Ok(frame)=frame{let _=handle.write(&frame);}
}

fn run_tcp_peer(driver:crate::transport::tcp::TcpDriver,running:Arc<AtomicBool>,model:Arc<ModbusDataModel>,config:ModbusConfig){
    let runtime=DataPlaneRuntime::spawn(Box::new(driver));
    let handle=runtime.handle.clone();
    let events=match handle.subscribe(){Ok(rx)=>rx,Err(_)=>{runtime.join();return;}};
    let mut framer=codec::tcp::TcpFramer::default();
    while running.load(Ordering::Acquire) {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(DataPlaneEvent::Closed(_))=>break,
            Ok(DataPlaneEvent::Data(data))=>{
                let frames=match framer.push(&data){Ok(v)=>v,Err(_)=>break};
                for frame in frames {
                    let (tid,unit,pdu)=match codec::tcp::decode(&frame){Ok(v)=>v,Err(_)=>continue};
                    if unit!=config.unit_id {continue;}
                    let request=match codec::decode_request(&pdu){Ok(v)=>v.request,Err(_)=>{let response=exception_pdu(pdu.first().copied().unwrap_or(0),0x03);if let Ok(adu)=codec::tcp::encode(tid,unit,&response){let _=handle.write(&adu);}continue;}};
                    if let Some(response)=execute_with_fault(&config,&model,&request){if let Ok(adu)=codec::tcp::encode(tid,unit,&response){let _=handle.write(&adu);}}
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
        }
    }
    runtime.join();
}

fn execute_with_fault(config:&ModbusConfig,model:&ModbusDataModel,request:&codec::ModbusRequest)->Option<Vec<u8>>{
    if config.server_fault.no_response{return None;}
    if config.server_fault.delay_ms>0{std::thread::sleep(Duration::from_millis(config.server_fault.delay_ms.min(60_000)));}
    if let Some(code)=config.server_fault.exception_code{return Some(exception_pdu(request.function(),code));}
    Some(match model.execute(request){Ok(response)=>response,Err(code)=>exception_pdu(request.function(),code)})
}
fn exception_pdu(function:u8,code:u8)->Vec<u8>{vec![function|0x80,code]}
