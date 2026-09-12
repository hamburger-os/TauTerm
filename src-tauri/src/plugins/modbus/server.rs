use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::plugins::modbus::client::{TransactionResult, TransactionStatus};
use crate::plugins::modbus::codec;
use crate::plugins::modbus::config::{ModbusConfig, ModbusMode, ServerFaultConfig};
use crate::plugins::modbus::data_model::ModbusDataModel;
use crate::transport::runtime::DataPlaneEvent;
use crate::transport::serial::open_serial;
use crate::transport::tcp::TcpListenerTransport;
use crate::transport::DataPlaneRuntime;

const SERVER_HISTORY_LIMIT: usize = 1000;

pub struct ModbusServer {
    config: ModbusConfig,
    pub model: Arc<ModbusDataModel>,
    fault: Arc<RwLock<ServerFaultConfig>>,
    history: Arc<Mutex<VecDeque<TransactionResult>>>,
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
                let driver =
                    open_serial(&config.serial_port, &config.serial).map_err(|e| e.to_string())?;
                (Some(DataPlaneRuntime::spawn(Box::new(driver))), None)
            }
            ModbusMode::Tcp => {
                let listener =
                    TcpListenerTransport::bind(&config.host, config.port, config.tcp.clone())
                        .map_err(|e| e.to_string())?;
                (None, Some(listener))
            }
        };
        let fault = Arc::new(RwLock::new(config.server_fault.clone()));
        Ok(Self {
            config,
            model: Arc::new(ModbusDataModel::default()),
            fault,
            history: Arc::new(Mutex::new(VecDeque::with_capacity(SERVER_HISTORY_LIMIT))),
            running: Arc::new(AtomicBool::new(false)),
            workers: Arc::new(Mutex::new(Vec::new())),
            serial_runtime: Mutex::new(serial_runtime),
            listener: Mutex::new(listener),
        })
    }

    pub fn start(&self) -> Result<(), String> {
        if self.running.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = match self.config.mode {
            ModbusMode::Rtu | ModbusMode::Ascii => self.start_serial(),
            ModbusMode::Tcp => self.start_tcp(),
        };
        if result.is_err() {
            self.running.store(false, Ordering::Release);
        }
        result
    }

    fn start_serial(&self) -> Result<(), String> {
        let handle = self
            .serial_runtime
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .ok_or("serial runtime unavailable")?
            .handle
            .clone();
        let events = handle.subscribe().map_err(|e| e.to_string())?;
        let running = self.running.clone();
        let config = self.config.clone();
        let model = self.model.clone();
        let fault = self.fault.clone();
        let history = self.history.clone();
        let worker = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            while running.load(Ordering::Acquire) {
                let wait = if buffer.is_empty() {
                    Duration::from_millis(50)
                } else {
                    match config.mode {
                        ModbusMode::Rtu => config.rtu_frame_gap(),
                        ModbusMode::Ascii => Duration::from_millis(50),
                        ModbusMode::Tcp => unreachable!(),
                    }
                };
                match events.recv_timeout(wait) {
                    Ok(DataPlaneEvent::Closed(_)) => break,
                    Ok(DataPlaneEvent::Data(data)) => {
                        buffer.extend_from_slice(&data);
                        if config.mode == ModbusMode::Ascii {
                            while let Some(end) = buffer.windows(2).position(|w| w == b"\r\n") {
                                let frame: Vec<u8> = buffer.drain(..end + 2).collect();
                                process_serial_frame(
                                    &handle, &config, &model, &fault, &history, &frame,
                                );
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        if !buffer.is_empty() && config.mode == ModbusMode::Rtu =>
                    {
                        let frame = std::mem::take(&mut buffer);
                        process_serial_frame(
                            &handle, &config, &model, &fault, &history, &frame,
                        );
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        self.workers.lock().map_err(|e| e.to_string())?.push(worker);
        Ok(())
    }

    fn start_tcp(&self) -> Result<(), String> {
        let listener = self
            .listener
            .lock()
            .map_err(|e| e.to_string())?
            .take()
            .ok_or("TCP listener unavailable")?;
        let running = self.running.clone();
        let workers = self.workers.clone();
        let model = self.model.clone();
        let fault = self.fault.clone();
        let history = self.history.clone();
        let config = self.config.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let listener_worker = std::thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok(Some((driver, _peer))) => {
                        let max = config.server_max_clients;
                        if max > 0 && active.load(Ordering::Acquire) >= max {
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let running_peer = running.clone();
                        let model_peer = model.clone();
                        let fault_peer = fault.clone();
                        let history_peer = history.clone();
                        let config_peer = config.clone();
                        let active_peer = active.clone();
                        let peer = std::thread::spawn(move || {
                            run_tcp_peer(
                                driver,
                                running_peer,
                                model_peer,
                                fault_peer,
                                history_peer,
                                config_peer,
                            );
                            active_peer.fetch_sub(1, Ordering::AcqRel);
                        });
                        if let Ok(mut list) = workers.lock() {
                            list.push(peer);
                        }
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(error) => {
                        log::warn!("Modbus TCP accept failed: {error}");
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
        self.workers
            .lock()
            .map_err(|e| e.to_string())?
            .push(listener_worker);
        Ok(())
    }

    pub fn set_fault(&self, fault: ServerFaultConfig) -> Result<(), String> {
        if fault.delay_ms > 60_000 {
            return Err("fault delay_ms must be <= 60000".into());
        }
        if let Some(code) = fault.exception_code {
            if !(1..=11).contains(&code) {
                return Err("fault exception_code must be 1..=11".into());
            }
        }
        *self.fault.write().map_err(|e| e.to_string())? = fault;
        Ok(())
    }

    pub fn fault(&self) -> ServerFaultConfig {
        self.fault.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn history(&self) -> Vec<TransactionResult> {
        self.history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .rev()
            .cloned()
            .collect()
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Release);
        if let Ok(mut runtime) = self.serial_runtime.lock() {
            if let Some(runtime) = runtime.take() {
                runtime.join();
            }
        }
        let handles = if let Ok(mut workers) = self.workers.lock() {
            std::mem::take(&mut *workers)
        } else {
            Vec::new()
        };
        for handle in handles {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

fn process_serial_frame(
    handle: &crate::transport::DataPlaneHandle,
    config: &ModbusConfig,
    model: &ModbusDataModel,
    fault: &RwLock<ServerFaultConfig>,
    history: &Mutex<VecDeque<TransactionResult>>,
    frame: &[u8],
) {
    let started = Instant::now();
    let decoded = match config.mode {
        ModbusMode::Rtu => codec::rtu::decode(frame),
        ModbusMode::Ascii => codec::ascii::decode(frame),
        ModbusMode::Tcp => return,
    };
    let (unit, pdu) = match decoded {
        Ok(value) => value,
        Err(error) => {
            log::debug!("Ignoring malformed Modbus serial frame: {error}");
            return;
        }
    };
    let broadcast = unit == 0;
    if !broadcast && unit != config.unit_id {
        return;
    }
    let request = match codec::decode_request(&pdu) {
        Ok(value) => value.request,
        Err(message) => {
            if broadcast {
                return;
            }
            let response_pdu = exception_pdu(pdu.first().copied().unwrap_or(0), 0x03);
            let raw_tx = encode_serial(config, unit, &response_pdu)
                .and_then(|frame| handle.write(&frame).ok().map(|_| frame))
                .unwrap_or_default();
            record_server(
                history,
                TransactionResult {
                    timestamp_ms: now_ms(),
                    status: TransactionStatus::ProtocolError,
                    function: pdu.first().copied().unwrap_or(0),
                    transaction_id: None,
                    unit_id: unit,
                    latency_ms: started.elapsed().as_millis(),
                    exception_code: Some(0x03),
                    raw_tx,
                    raw_rx: frame.to_vec(),
                    response_pdu,
                    message: Some(message),
                    write_outcome_unknown: false,
                    attempt: 0,
                },
            );
            return;
        }
    };
    if broadcast && !request.is_write() {
        return;
    }
    let execution = execute_with_fault(fault, model, &request);
    if broadcast {
        record_server(
            history,
            TransactionResult {
                timestamp_ms: now_ms(),
                status: TransactionStatus::Broadcast,
                function: request.function(),
                transaction_id: None,
                unit_id: 0,
                latency_ms: started.elapsed().as_millis(),
                exception_code: execution.exception_code,
                raw_tx: Vec::new(),
                raw_rx: frame.to_vec(),
                response_pdu: Vec::new(),
                message: execution.message,
                write_outcome_unknown: false,
                attempt: 0,
            },
        );
        return;
    }
    let (raw_tx, response_pdu) = match execution.response {
        Some(response_pdu) => {
            let raw_tx = encode_serial(config, unit, &response_pdu)
                .and_then(|frame| handle.write(&frame).ok().map(|_| frame))
                .unwrap_or_default();
            (raw_tx, response_pdu)
        }
        None => (Vec::new(), Vec::new()),
    };
    record_server(
        history,
        TransactionResult {
            timestamp_ms: now_ms(),
            status: execution.status,
            function: request.function(),
            transaction_id: None,
            unit_id: unit,
            latency_ms: started.elapsed().as_millis(),
            exception_code: execution.exception_code,
            raw_tx,
            raw_rx: frame.to_vec(),
            response_pdu,
            message: execution.message,
            write_outcome_unknown: false,
            attempt: 0,
        },
    );
}

fn encode_serial(config: &ModbusConfig, unit: u8, pdu: &[u8]) -> Option<Vec<u8>> {
    match config.mode {
        ModbusMode::Rtu => codec::rtu::encode(unit, pdu).ok(),
        ModbusMode::Ascii => codec::ascii::encode(unit, pdu).ok(),
        ModbusMode::Tcp => None,
    }
}

fn run_tcp_peer(
    driver: crate::transport::tcp::TcpDriver,
    running: Arc<AtomicBool>,
    model: Arc<ModbusDataModel>,
    fault: Arc<RwLock<ServerFaultConfig>>,
    history: Arc<Mutex<VecDeque<TransactionResult>>>,
    config: ModbusConfig,
) {
    let runtime = DataPlaneRuntime::spawn(Box::new(driver));
    let handle = runtime.handle.clone();
    let events = match handle.subscribe() {
        Ok(receiver) => receiver,
        Err(_) => {
            runtime.join();
            return;
        }
    };
    let mut framer = codec::tcp::TcpFramer::default();
    while running.load(Ordering::Acquire) {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(DataPlaneEvent::Closed(_)) => break,
            Ok(DataPlaneEvent::Data(data)) => {
                let frames = match framer.push(&data) {
                    Ok(frames) => frames,
                    Err(_) => break,
                };
                for frame in frames {
                    let started = Instant::now();
                    let (tid, unit, pdu) = match codec::tcp::decode(&frame) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    if unit != config.unit_id {
                        continue;
                    }
                    let request = match codec::decode_request(&pdu) {
                        Ok(value) => value.request,
                        Err(message) => {
                            let response_pdu =
                                exception_pdu(pdu.first().copied().unwrap_or(0), 0x03);
                            let raw_tx = codec::tcp::encode(tid, unit, &response_pdu)
                                .ok()
                                .and_then(|adu| handle.write(&adu).ok().map(|_| adu))
                                .unwrap_or_default();
                            record_server(
                                &history,
                                TransactionResult {
                                    timestamp_ms: now_ms(),
                                    status: TransactionStatus::ProtocolError,
                                    function: pdu.first().copied().unwrap_or(0),
                                    transaction_id: Some(tid),
                                    unit_id: unit,
                                    latency_ms: started.elapsed().as_millis(),
                                    exception_code: Some(0x03),
                                    raw_tx,
                                    raw_rx: frame,
                                    response_pdu,
                                    message: Some(message),
                                    write_outcome_unknown: false,
                                    attempt: 0,
                                },
                            );
                            continue;
                        }
                    };
                    let execution = execute_with_fault(&fault, &model, &request);
                    let (raw_tx, response_pdu) = match execution.response {
                        Some(response_pdu) => {
                            let raw_tx = codec::tcp::encode(tid, unit, &response_pdu)
                                .ok()
                                .and_then(|adu| handle.write(&adu).ok().map(|_| adu))
                                .unwrap_or_default();
                            (raw_tx, response_pdu)
                        }
                        None => (Vec::new(), Vec::new()),
                    };
                    record_server(
                        &history,
                        TransactionResult {
                            timestamp_ms: now_ms(),
                            status: execution.status,
                            function: request.function(),
                            transaction_id: Some(tid),
                            unit_id: unit,
                            latency_ms: started.elapsed().as_millis(),
                            exception_code: execution.exception_code,
                            raw_tx,
                            raw_rx: frame,
                            response_pdu,
                            message: execution.message,
                            write_outcome_unknown: false,
                            attempt: 0,
                        },
                    );
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    runtime.join();
}

struct ServerExecution {
    response: Option<Vec<u8>>,
    status: TransactionStatus,
    exception_code: Option<u8>,
    message: Option<String>,
}

fn execute_with_fault(
    fault: &RwLock<ServerFaultConfig>,
    model: &ModbusDataModel,
    request: &codec::ModbusRequest,
) -> ServerExecution {
    let fault = fault.read().unwrap_or_else(|e| e.into_inner()).clone();
    if fault.delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(fault.delay_ms.min(60_000)));
    }
    if let Some(code) = fault.exception_code {
        return ServerExecution {
            response: Some(exception_pdu(request.function(), code)),
            status: TransactionStatus::ModbusException,
            exception_code: Some(code),
            message: Some("forced server exception".into()),
        };
    }

    let (response, status, exception_code) = match model.execute(request) {
        Ok(response) => (response, TransactionStatus::Success, None),
        Err(code) => (
            exception_pdu(request.function(), code),
            TransactionStatus::ModbusException,
            Some(code),
        ),
    };
    if fault.no_response {
        // The request is still executed. Suppressing only the response makes write-timeout tests
        // exercise the real "outcome unknown" condition seen by a client.
        return ServerExecution {
            response: None,
            status: TransactionStatus::FaultInjected,
            exception_code,
            message: Some("response suppressed by server fault injection".into()),
        };
    }
    ServerExecution {
        response: Some(response),
        status,
        exception_code,
        message: None,
    }
}

fn record_server(history: &Mutex<VecDeque<TransactionResult>>, result: TransactionResult) {
    let mut history = history.lock().unwrap_or_else(|error| error.into_inner());
    if history.len() >= SERVER_HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(result);
}

fn exception_pdu(function: u8, code: u8) -> Vec<u8> {
    vec![function | 0x80, code]
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_response_fault_executes_write_but_suppresses_response() {
        let model = ModbusDataModel::default();
        model.set_holding_register(7, 1);
        let fault = RwLock::new(ServerFaultConfig {
            no_response: true,
            delay_ms: 0,
            exception_code: None,
        });
        let execution = execute_with_fault(
            &fault,
            &model,
            &codec::ModbusRequest::WriteSingle {
                function: 0x06,
                address: 7,
                value: 42,
            },
        );
        assert!(execution.response.is_none());
        assert!(matches!(execution.status, TransactionStatus::FaultInjected));
        let snapshot = model.snapshot();
        assert_eq!(snapshot.holding_registers, vec![(7, 42)]);
    }

    #[test]
    fn forced_exception_does_not_apply_write() {
        let model = ModbusDataModel::default();
        model.set_holding_register(7, 1);
        let fault = RwLock::new(ServerFaultConfig {
            no_response: false,
            delay_ms: 0,
            exception_code: Some(0x04),
        });
        let execution = execute_with_fault(
            &fault,
            &model,
            &codec::ModbusRequest::WriteSingle {
                function: 0x06,
                address: 7,
                value: 42,
            },
        );
        assert_eq!(execution.exception_code, Some(0x04));
        assert_eq!(model.snapshot().holding_registers, vec![(7, 1)]);
    }
}