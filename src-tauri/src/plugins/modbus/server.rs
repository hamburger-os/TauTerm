use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::plugins::modbus::capability::request_supported_on_mode;
use crate::plugins::modbus::client::{
    TransactionHistoryBatch, TransactionRecord, TransactionResult, TransactionStatus,
};
use crate::plugins::modbus::codec;
use crate::plugins::modbus::config::{
    validate_fault, ModbusEndpointConfig, ModbusMode, ServerFaultConfig, ValidatedModbusConfig,
};
use crate::plugins::modbus::data_model::ModbusDataModel;
use crate::transport::runtime::DataPlaneEvent;
use crate::transport::serial::open_serial;
use crate::transport::tcp::TcpListenerTransport;
use crate::transport::DataPlaneRuntime;

const SERVER_HISTORY_LIMIT: usize = 1000;
const HISTORY_QUERY_LIMIT: usize = 500;

type ServerHistory = Arc<Mutex<VecDeque<TransactionRecord>>>;

pub struct ModbusServer {
    config: ValidatedModbusConfig,
    pub model: Arc<ModbusDataModel>,
    fault: Arc<RwLock<ServerFaultConfig>>,
    history: ServerHistory,
    next_history_sequence: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
    serial_runtime: Mutex<Option<DataPlaneRuntime>>,
    listener: Mutex<Option<TcpListenerTransport>>,
}

impl ModbusServer {
    pub fn new(config: ValidatedModbusConfig) -> Result<Self, String> {
        let server = config
            .server()
            .ok_or("ModbusServer requires server runtime configuration")?;
        let fault = Arc::new(RwLock::new(server.fault.clone()));
        let (serial_runtime, listener) = match &config.endpoint {
            ModbusEndpointConfig::Serial {
                port, transport, ..
            } => {
                let driver = open_serial(port, transport).map_err(|e| e.to_string())?;
                (Some(DataPlaneRuntime::spawn(Box::new(driver))), None)
            }
            ModbusEndpointConfig::Tcp {
                host,
                port,
                transport,
            } => {
                let listener = TcpListenerTransport::bind(host, *port, transport.clone())
                    .map_err(|e| e.to_string())?;
                (None, Some(listener))
            }
        };
        Ok(Self {
            config,
            model: Arc::new(ModbusDataModel::default()),
            fault,
            history: Arc::new(Mutex::new(VecDeque::with_capacity(SERVER_HISTORY_LIMIT))),
            next_history_sequence: Arc::new(AtomicU64::new(1)),
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
        let result = match self.config.mode() {
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
        let sequence = self.next_history_sequence.clone();
        let worker = std::thread::spawn(move || match config.mode() {
            ModbusMode::Rtu => run_rtu_server(
                &handle, &events, &running, &config, &model, &fault, &history, &sequence,
            ),
            ModbusMode::Ascii => run_ascii_server(
                &handle, &events, &running, &config, &model, &fault, &history, &sequence,
            ),
            ModbusMode::Tcp => unreachable!(),
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
        let sequence = self.next_history_sequence.clone();
        let config = self.config.clone();
        let max_clients = self
            .config
            .server()
            .expect("server role checked by constructor")
            .max_clients;
        let active = Arc::new(AtomicUsize::new(0));
        let listener_worker = std::thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                if let Ok(mut list) = workers.lock() {
                    reap_finished_workers(&mut list);
                }
                match listener.accept() {
                    Ok(Some((driver, _peer))) => {
                        if max_clients > 0 && active.load(Ordering::Acquire) >= max_clients {
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let running_peer = running.clone();
                        let model_peer = model.clone();
                        let fault_peer = fault.clone();
                        let history_peer = history.clone();
                        let sequence_peer = sequence.clone();
                        let config_peer = config.clone();
                        let active_peer = active.clone();
                        let peer = std::thread::spawn(move || {
                            run_tcp_peer(
                                driver,
                                running_peer,
                                model_peer,
                                fault_peer,
                                history_peer,
                                sequence_peer,
                                config_peer,
                            );
                            active_peer.fetch_sub(1, Ordering::AcqRel);
                        });
                        if let Ok(mut list) = workers.lock() {
                            reap_finished_workers(&mut list);
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
        validate_fault(&fault)?;
        *self.fault.write().map_err(|e| e.to_string())? = fault;
        Ok(())
    }

    pub fn fault(&self) -> ServerFaultConfig {
        self.fault.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn history_since(&self, after_sequence: u64, limit: usize) -> TransactionHistoryBatch {
        let history = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let latest_sequence = history
            .back()
            .map_or(after_sequence, |record| record.sequence);
        let limit = limit.clamp(1, HISTORY_QUERY_LIMIT);
        let records = if after_sequence == 0 {
            let mut recent: Vec<_> = history.iter().rev().take(limit).cloned().collect();
            recent.reverse();
            recent
        } else {
            history
                .iter()
                .filter(|record| record.sequence > after_sequence)
                .take(limit)
                .cloned()
                .collect()
        };
        TransactionHistoryBatch {
            records,
            latest_sequence,
        }
    }

    pub fn last_result(&self) -> Option<TransactionResult> {
        self.history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .back()
            .map(|record| record.result.clone())
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

fn reap_finished_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let handle = workers.remove(index);
            let _ = handle.join();
        } else {
            index += 1;
        }
    }
}

fn run_ascii_server(
    handle: &crate::transport::DataPlaneHandle,
    events: &std::sync::mpsc::Receiver<DataPlaneEvent>,
    running: &AtomicBool,
    config: &ValidatedModbusConfig,
    model: &ModbusDataModel,
    fault: &RwLock<ServerFaultConfig>,
    history: &Mutex<VecDeque<TransactionRecord>>,
    sequence: &AtomicU64,
) {
    let mut framer = codec::ascii::AsciiFramer::default();
    while running.load(Ordering::Acquire) {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(DataPlaneEvent::Closed(_)) => break,
            Ok(DataPlaneEvent::Data(data)) => {
                for frame in framer.push(&data) {
                    process_serial_frame(handle, config, model, fault, history, sequence, &frame);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn run_rtu_server(
    handle: &crate::transport::DataPlaneHandle,
    events: &std::sync::mpsc::Receiver<DataPlaneEvent>,
    running: &AtomicBool,
    config: &ValidatedModbusConfig,
    model: &ModbusDataModel,
    fault: &RwLock<ServerFaultConfig>,
    history: &Mutex<VecDeque<TransactionRecord>>,
    sequence: &AtomicU64,
) {
    let mut buffer = Vec::new();
    let inter_char = config.rtu_inter_char_gap();
    let frame_gap = config.rtu_frame_gap();
    let frame_tail = frame_gap.saturating_sub(inter_char);
    while running.load(Ordering::Acquire) {
        let wait = if buffer.is_empty() {
            Duration::from_millis(50)
        } else {
            inter_char
        };
        match events.recv_timeout(wait) {
            Ok(DataPlaneEvent::Closed(_)) => break,
            Ok(DataPlaneEvent::Data(data)) => buffer.extend_from_slice(&data),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if buffer.is_empty() => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                match events.recv_timeout(frame_tail) {
                    Ok(DataPlaneEvent::Data(data)) => {
                        buffer.extend_from_slice(&data);
                        log::debug!("Discarding malformed Modbus RTU frame after t1.5 gap");
                        buffer.clear();
                    }
                    Ok(DataPlaneEvent::Closed(_)) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        let frame = std::mem::take(&mut buffer);
                        process_serial_frame(
                            handle, config, model, fault, history, sequence, &frame,
                        );
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn process_serial_frame(
    handle: &crate::transport::DataPlaneHandle,
    config: &ValidatedModbusConfig,
    model: &ModbusDataModel,
    fault: &RwLock<ServerFaultConfig>,
    history: &Mutex<VecDeque<TransactionRecord>>,
    sequence: &AtomicU64,
    frame: &[u8],
) {
    let started = Instant::now();
    let decoded = match config.mode() {
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
            let (raw_tx, delivery_error) =
                send_serial_response(handle, config, unit, &response_pdu);
            record_server(
                history,
                sequence,
                TransactionResult {
                    timestamp_ms: now_ms(),
                    status: if delivery_error.is_some() {
                        TransactionStatus::TransportError
                    } else {
                        TransactionStatus::ProtocolError
                    },
                    function: pdu.first().copied().unwrap_or(0),
                    transaction_id: None,
                    unit_id: unit,
                    latency_ms: started.elapsed().as_millis(),
                    exception_code: Some(0x03),
                    raw_tx,
                    raw_rx: frame.to_vec(),
                    response_pdu,
                    semantic_response: None,
                    message: Some(append_delivery_error(message, delivery_error)),
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
            sequence,
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
                semantic_response: None,
                message: execution.message,
                write_outcome_unknown: false,
                attempt: 0,
            },
        );
        return;
    }

    let mut status = execution.status;
    let mut message = execution.message;
    let (raw_tx, response_pdu) = match execution.response {
        Some(response_pdu) => {
            let (raw_tx, delivery_error) =
                send_serial_response(handle, config, unit, &response_pdu);
            if let Some(error) = delivery_error {
                status = TransactionStatus::TransportError;
                message = Some(append_delivery_error(
                    message.unwrap_or_else(|| "request processed".into()),
                    Some(error),
                ));
            }
            (raw_tx, response_pdu)
        }
        None => (Vec::new(), Vec::new()),
    };
    record_server(
        history,
        sequence,
        TransactionResult {
            timestamp_ms: now_ms(),
            status,
            function: request.function(),
            transaction_id: None,
            unit_id: unit,
            latency_ms: started.elapsed().as_millis(),
            exception_code: execution.exception_code,
            raw_tx,
            raw_rx: frame.to_vec(),
            response_pdu,
            semantic_response: None,
            message,
            write_outcome_unknown: false,
            attempt: 0,
        },
    );
}

fn send_serial_response(
    handle: &crate::transport::DataPlaneHandle,
    config: &ValidatedModbusConfig,
    unit: u8,
    pdu: &[u8],
) -> (Vec<u8>, Option<String>) {
    let encoded = match config.mode() {
        ModbusMode::Rtu => codec::rtu::encode(unit, pdu),
        ModbusMode::Ascii => codec::ascii::encode(unit, pdu),
        ModbusMode::Tcp => return (Vec::new(), Some("invalid serial response mode".into())),
    };
    match encoded {
        Ok(frame) => match handle.write(&frame) {
            Ok(()) => (frame, None),
            Err(error) => (frame, Some(error.to_string())),
        },
        Err(error) => (Vec::new(), Some(error)),
    }
}

fn run_tcp_peer(
    driver: crate::transport::tcp::TcpDriver,
    running: Arc<AtomicBool>,
    model: Arc<ModbusDataModel>,
    fault: Arc<RwLock<ServerFaultConfig>>,
    history: ServerHistory,
    sequence: Arc<AtomicU64>,
    config: ValidatedModbusConfig,
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
                    Err(error) => {
                        log::debug!("Closing malformed Modbus TCP peer: {error}");
                        break;
                    }
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
                            let (raw_tx, delivery_error) =
                                send_tcp_response(&handle, tid, unit, &response_pdu);
                            record_server(
                                &history,
                                &sequence,
                                TransactionResult {
                                    timestamp_ms: now_ms(),
                                    status: if delivery_error.is_some() {
                                        TransactionStatus::TransportError
                                    } else {
                                        TransactionStatus::ProtocolError
                                    },
                                    function: pdu.first().copied().unwrap_or(0),
                                    transaction_id: Some(tid),
                                    unit_id: unit,
                                    latency_ms: started.elapsed().as_millis(),
                                    exception_code: Some(0x03),
                                    raw_tx,
                                    raw_rx: frame,
                                    response_pdu,
                                    semantic_response: None,
                                    message: Some(append_delivery_error(message, delivery_error)),
                                    write_outcome_unknown: false,
                                    attempt: 0,
                                },
                            );
                            continue;
                        }
                    };
                    let execution = if !request_supported_on_mode(ModbusMode::Tcp, &request) {
                        ServerExecution {
                            response: Some(exception_pdu(request.function(), 0x01)),
                            status: TransactionStatus::ModbusException,
                            exception_code: Some(0x01),
                            message: Some(
                                "serial-line-only function is unavailable on Modbus TCP".into(),
                            ),
                        }
                    } else {
                        execute_with_fault(&fault, &model, &request)
                    };
                    let mut status = execution.status;
                    let mut message = execution.message;
                    let (raw_tx, response_pdu) = match execution.response {
                        Some(response_pdu) => {
                            let (raw_tx, delivery_error) =
                                send_tcp_response(&handle, tid, unit, &response_pdu);
                            if let Some(error) = delivery_error {
                                status = TransactionStatus::TransportError;
                                message = Some(append_delivery_error(
                                    message.unwrap_or_else(|| "request processed".into()),
                                    Some(error),
                                ));
                            }
                            (raw_tx, response_pdu)
                        }
                        None => (Vec::new(), Vec::new()),
                    };
                    record_server(
                        &history,
                        &sequence,
                        TransactionResult {
                            timestamp_ms: now_ms(),
                            status,
                            function: request.function(),
                            transaction_id: Some(tid),
                            unit_id: unit,
                            latency_ms: started.elapsed().as_millis(),
                            exception_code: execution.exception_code,
                            raw_tx,
                            raw_rx: frame,
                            response_pdu,
                            semantic_response: None,
                            message,
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
    drop(events);
    runtime.join();
}

fn send_tcp_response(
    handle: &crate::transport::DataPlaneHandle,
    tid: u16,
    unit: u8,
    pdu: &[u8],
) -> (Vec<u8>, Option<String>) {
    match codec::tcp::encode(tid, unit, pdu) {
        Ok(frame) => match handle.write(&frame) {
            Ok(()) => (frame, None),
            Err(error) => (frame, Some(error.to_string())),
        },
        Err(error) => (Vec::new(), Some(error)),
    }
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
        std::thread::sleep(Duration::from_millis(fault.delay_ms));
    }

    if fault.no_response {
        let exception_code = model.execute(request).err();
        return ServerExecution {
            response: None,
            status: TransactionStatus::FaultInjected,
            exception_code,
            message: Some("response suppressed by server fault injection".into()),
        };
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
    ServerExecution {
        response: Some(response),
        status,
        exception_code,
        message: None,
    }
}

fn append_delivery_error(message: String, delivery_error: Option<String>) -> String {
    match delivery_error {
        Some(error) => format!("{message}; response delivery failed: {error}"),
        None => message,
    }
}

fn record_server(
    history: &Mutex<VecDeque<TransactionRecord>>,
    sequence: &AtomicU64,
    result: TransactionResult,
) {
    let sequence = sequence.fetch_add(1, Ordering::Relaxed);
    let mut history = history.lock().unwrap_or_else(|error| error.into_inner());
    if history.len() >= SERVER_HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(TransactionRecord { sequence, result });
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
    use crate::plugins::modbus::codec::RegisterReadArea;

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
            &codec::ModbusRequest::WriteSingleRegister {
                address: 7,
                value: 42,
            },
        );
        assert!(execution.response.is_none());
        assert!(matches!(execution.status, TransactionStatus::FaultInjected));
        assert_eq!(model.snapshot().holding_registers, vec![(7, 42)]);
    }

    #[test]
    fn no_response_has_priority_over_forced_exception() {
        let model = ModbusDataModel::default();
        model.set_holding_register(7, 1);
        let fault = RwLock::new(ServerFaultConfig {
            no_response: true,
            delay_ms: 0,
            exception_code: Some(0x04),
        });
        let execution = execute_with_fault(
            &fault,
            &model,
            &codec::ModbusRequest::WriteSingleRegister {
                address: 7,
                value: 42,
            },
        );
        assert!(execution.response.is_none());
        assert_eq!(model.snapshot().holding_registers, vec![(7, 42)]);
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
            &codec::ModbusRequest::WriteSingleRegister {
                address: 7,
                value: 42,
            },
        );
        assert_eq!(execution.exception_code, Some(0x04));
        assert_eq!(model.snapshot().holding_registers, vec![(7, 1)]);
    }

    #[test]
    fn tcp_server_uses_shared_transport_capability() {
        assert!(!request_supported_on_mode(
            ModbusMode::Tcp,
            &codec::ModbusRequest::GetCommEventLog
        ));
        assert!(request_supported_on_mode(
            ModbusMode::Tcp,
            &codec::ModbusRequest::ReadRegisters {
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            }
        ));
    }

    #[test]
    fn finished_worker_handles_are_reaped() {
        let mut workers = vec![std::thread::spawn(|| {})];
        while !workers[0].is_finished() {
            std::thread::yield_now();
        }
        reap_finished_workers(&mut workers);
        assert!(workers.is_empty());
    }
}
