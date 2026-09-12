use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::plugins::modbus::codec::{self, AduMode, ModbusRequest};
use crate::plugins::modbus::config::{ModbusMode, ValidatedModbusConfig};
use crate::transport::{DataPlaneEvent, DataPlaneRuntime, DataPlaneSubscription};

const HISTORY_LIMIT: usize = 1000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatus {
    Success,
    Broadcast,
    ModbusException,
    ProtocolError,
    MalformedResponse,
    Timeout,
    TransportError,
    Cancelled,
    FaultInjected,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionResult {
    pub timestamp_ms: u64,
    pub status: TransactionStatus,
    pub function: u8,
    pub transaction_id: Option<u16>,
    pub unit_id: u8,
    pub latency_ms: u128,
    pub exception_code: Option<u8>,
    pub raw_tx: Vec<u8>,
    pub raw_rx: Vec<u8>,
    pub response_pdu: Vec<u8>,
    pub message: Option<String>,
    pub write_outcome_unknown: bool,
    pub attempt: u8,
}

struct TransactionFailure {
    status: TransactionStatus,
    function: u8,
    transaction_id: Option<u16>,
    started: Instant,
    raw_tx: Vec<u8>,
    raw_rx: Vec<u8>,
    message: String,
    write_outcome_unknown: bool,
    attempt: u8,
}

pub struct ModbusClient {
    config: ValidatedModbusConfig,
    runtime: Mutex<Option<DataPlaneRuntime>>,
    events: Mutex<Option<DataPlaneSubscription>>,
    transaction_guard: Mutex<()>,
    next_transaction_id: AtomicU16,
    history: Mutex<VecDeque<TransactionResult>>,
}

impl ModbusClient {
    pub fn new(config: ValidatedModbusConfig, runtime: DataPlaneRuntime) -> Result<Self, String> {
        if config.client().is_none() {
            return Err("ModbusClient requires client runtime configuration".into());
        }
        Ok(Self {
            config,
            runtime: Mutex::new(Some(runtime)),
            events: Mutex::new(None),
            transaction_guard: Mutex::new(()),
            next_transaction_id: AtomicU16::new(1),
            history: Mutex::new(VecDeque::with_capacity(HISTORY_LIMIT)),
        })
    }

    pub fn execute(&self, request: ModbusRequest) -> TransactionResult {
        let _guard = self
            .transaction_guard
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let client_config = self
            .config
            .client()
            .expect("client role checked by constructor");
        let max_retries = if request.is_write() && !client_config.retry_writes {
            0
        } else {
            client_config.read_retries
        };
        let mut attempt = 0u8;
        let result = loop {
            let result = self.execute_once(&request, attempt);
            let retryable = matches!(
                result.status,
                TransactionStatus::Timeout | TransactionStatus::TransportError
            ) && attempt < max_retries;
            if !retryable {
                break result;
            }
            attempt = attempt.saturating_add(1);
        };
        self.record(result.clone());
        result
    }

    pub fn execute_raw_adu(
        &self,
        data: Vec<u8>,
        wait_response: bool,
        quiet_period_ms: u64,
    ) -> TransactionResult {
        let _guard = self
            .transaction_guard
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let started = Instant::now();
        let handle = {
            let runtime = self
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match runtime.as_ref() {
                Some(runtime) => runtime.handle.clone(),
                None => {
                    let result = self.failure(TransactionFailure {
                        status: TransactionStatus::Cancelled,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data,
                        raw_rx: Vec::new(),
                        message: "client is closed".into(),
                        write_outcome_unknown: false,
                        attempt: 0,
                    });
                    self.record(result.clone());
                    return result;
                }
            }
        };
        let mut events_guard = self
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if events_guard.is_none() {
            match handle.subscribe() {
                Ok(events) => *events_guard = Some(events),
                Err(error) => {
                    let result = self.failure(TransactionFailure {
                        status: TransactionStatus::TransportError,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data,
                        raw_rx: Vec::new(),
                        message: error.to_string(),
                        write_outcome_unknown: false,
                        attempt: 0,
                    });
                    self.record(result.clone());
                    return result;
                }
            }
        }
        let events = events_guard.as_ref().expect("receiver initialized");
        if let Err(error) = handle.write(&data) {
            let result = self.failure(TransactionFailure {
                status: TransactionStatus::TransportError,
                function: 0,
                transaction_id: None,
                started,
                raw_tx: data,
                raw_rx: Vec::new(),
                message: error.to_string(),
                write_outcome_unknown: false,
                attempt: 0,
            });
            self.record(result.clone());
            return result;
        }
        if !wait_response {
            let result = TransactionResult {
                timestamp_ms: now_ms(),
                status: TransactionStatus::Success,
                function: 0,
                transaction_id: None,
                unit_id: self.config.unit_id,
                latency_ms: started.elapsed().as_millis(),
                exception_code: None,
                raw_tx: data,
                raw_rx: Vec::new(),
                response_pdu: Vec::new(),
                message: Some("exact Raw ADU sent without response validation".into()),
                write_outcome_unknown: false,
                attempt: 0,
            };
            self.record(result.clone());
            return result;
        }

        let response_timeout_ms = self
            .config
            .client()
            .expect("client role checked by constructor")
            .response_timeout_ms;
        let deadline = Instant::now() + Duration::from_millis(response_timeout_ms);
        let quiet = Duration::from_millis(quiet_period_ms.clamp(1, 1000));
        let mut raw_rx = Vec::new();
        let result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break if raw_rx.is_empty() {
                    self.failure(TransactionFailure {
                        status: TransactionStatus::Timeout,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data.clone(),
                        raw_rx,
                        message: "Raw ADU response timeout".into(),
                        write_outcome_unknown: false,
                        attempt: 0,
                    })
                } else {
                    raw_success(self.config.unit_id, started, data.clone(), raw_rx)
                };
            }
            let wait = if raw_rx.is_empty() {
                remaining
            } else {
                remaining.min(quiet)
            };
            match events.recv_timeout(wait) {
                Ok(DataPlaneEvent::Data(chunk)) => raw_rx.extend_from_slice(&chunk),
                Ok(DataPlaneEvent::Closed(info)) if raw_rx.is_empty() => {
                    break self.failure(TransactionFailure {
                        status: TransactionStatus::TransportError,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data.clone(),
                        raw_rx,
                        message: info.reason,
                        write_outcome_unknown: false,
                        attempt: 0,
                    });
                }
                Ok(DataPlaneEvent::Closed(info)) => {
                    let mut result =
                        raw_success(self.config.unit_id, started, data.clone(), raw_rx);
                    result.message = Some(format!(
                        "Raw ADU response is unvalidated; transport closed: {}",
                        info.reason
                    ));
                    break result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) if raw_rx.is_empty() => {
                    break self.failure(TransactionFailure {
                        status: TransactionStatus::Timeout,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data.clone(),
                        raw_rx,
                        message: "Raw ADU response timeout".into(),
                        write_outcome_unknown: false,
                        attempt: 0,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break raw_success(self.config.unit_id, started, data.clone(), raw_rx);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break self.failure(TransactionFailure {
                        status: TransactionStatus::TransportError,
                        function: 0,
                        transaction_id: None,
                        started,
                        raw_tx: data.clone(),
                        raw_rx,
                        message: "transport event stream closed".into(),
                        write_outcome_unknown: false,
                        attempt: 0,
                    });
                }
            }
        };
        self.record(result.clone());
        result
    }

    fn execute_once(&self, request: &ModbusRequest, attempt: u8) -> TransactionResult {
        let started = Instant::now();
        let function = request.function();
        let current_mode = self.config.mode();
        let transaction_id = if current_mode == ModbusMode::Tcp {
            Some(self.next_transaction_id.fetch_add(1, Ordering::Relaxed))
        } else {
            None
        };
        if !request_supported_on_mode(current_mode, request) {
            return self.failure(TransactionFailure {
                status: TransactionStatus::ProtocolError,
                function,
                transaction_id,
                started,
                raw_tx: Vec::new(),
                raw_rx: Vec::new(),
                message:
                    "function is defined for Modbus serial line and is not available on Modbus TCP"
                        .into(),
                write_outcome_unknown: false,
                attempt,
            });
        }
        let pdu = match codec::encode_request(request) {
            Ok(pdu) => pdu,
            Err(message) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::ProtocolError,
                    function,
                    transaction_id,
                    started,
                    raw_tx: Vec::new(),
                    raw_rx: Vec::new(),
                    message,
                    write_outcome_unknown: false,
                    attempt,
                });
            }
        };
        if self.config.unit_id == 0 && current_mode != ModbusMode::Tcp && !request.is_write() {
            return self.failure(TransactionFailure {
                status: TransactionStatus::ProtocolError,
                function,
                transaction_id,
                started,
                raw_tx: Vec::new(),
                raw_rx: Vec::new(),
                message: "broadcast address 0 is write-only".into(),
                write_outcome_unknown: false,
                attempt,
            });
        }
        let tx = match codec::encode_adu(
            mode(current_mode),
            self.config.unit_id,
            transaction_id.unwrap_or(0),
            &pdu,
        ) {
            Ok(frame) => frame,
            Err(message) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::ProtocolError,
                    function,
                    transaction_id,
                    started,
                    raw_tx: Vec::new(),
                    raw_rx: Vec::new(),
                    message,
                    write_outcome_unknown: false,
                    attempt,
                });
            }
        };

        let handle = {
            let runtime = self
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match runtime.as_ref() {
                Some(runtime) => runtime.handle.clone(),
                None => {
                    return self.failure(TransactionFailure {
                        status: TransactionStatus::Cancelled,
                        function,
                        transaction_id,
                        started,
                        raw_tx: tx,
                        raw_rx: Vec::new(),
                        message: "client is closed".into(),
                        write_outcome_unknown: false,
                        attempt,
                    });
                }
            }
        };
        let mut events_guard = self
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if events_guard.is_none() {
            match handle.subscribe() {
                Ok(receiver) => *events_guard = Some(receiver),
                Err(error) => {
                    return self.failure(TransactionFailure {
                        status: TransactionStatus::TransportError,
                        function,
                        transaction_id,
                        started,
                        raw_tx: tx,
                        raw_rx: Vec::new(),
                        message: error.to_string(),
                        write_outcome_unknown: request.is_write(),
                        attempt,
                    });
                }
            }
        }
        let events = events_guard.as_ref().expect("receiver initialized");
        if let Err(error) = handle.write(&tx) {
            return self.failure(TransactionFailure {
                status: TransactionStatus::TransportError,
                function,
                transaction_id,
                started,
                raw_tx: tx,
                raw_rx: Vec::new(),
                message: error.to_string(),
                write_outcome_unknown: request.is_write(),
                attempt,
            });
        }

        if self.config.unit_id == 0 && current_mode != ModbusMode::Tcp {
            return TransactionResult {
                timestamp_ms: now_ms(),
                status: TransactionStatus::Broadcast,
                function,
                transaction_id,
                unit_id: 0,
                latency_ms: started.elapsed().as_millis(),
                exception_code: None,
                raw_tx: tx,
                raw_rx: Vec::new(),
                response_pdu: Vec::new(),
                message: None,
                write_outcome_unknown: false,
                attempt,
            };
        }

        let response_timeout_ms = self
            .config
            .client()
            .expect("client role checked by constructor")
            .response_timeout_ms;
        let deadline = Instant::now() + Duration::from_millis(response_timeout_ms);
        let received = match current_mode {
            ModbusMode::Tcp => receive_tcp(
                events,
                deadline,
                transaction_id.unwrap_or(0),
                self.config.unit_id,
            ),
            ModbusMode::Ascii => receive_ascii(events, deadline, self.config.unit_id),
            ModbusMode::Rtu => receive_rtu(
                events,
                deadline,
                self.config.rtu_inter_char_gap(),
                self.config.rtu_frame_gap(),
                self.config.unit_id,
            ),
        };
        let (raw_rx, response_pdu) = match received {
            Ok(value) => value,
            Err(ReceiveError::Timeout) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::Timeout,
                    function,
                    transaction_id,
                    started,
                    raw_tx: tx,
                    raw_rx: Vec::new(),
                    message: "response timeout".into(),
                    write_outcome_unknown: request.is_write(),
                    attempt,
                });
            }
            Err(ReceiveError::Transport(message)) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::TransportError,
                    function,
                    transaction_id,
                    started,
                    raw_tx: tx,
                    raw_rx: Vec::new(),
                    message,
                    write_outcome_unknown: request.is_write(),
                    attempt,
                });
            }
            Err(ReceiveError::Malformed(message, raw)) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::MalformedResponse,
                    function,
                    transaction_id,
                    started,
                    raw_tx: tx,
                    raw_rx: raw,
                    message,
                    write_outcome_unknown: false,
                    attempt,
                });
            }
            Err(ReceiveError::Protocol(message, raw)) => {
                return self.failure(TransactionFailure {
                    status: TransactionStatus::ProtocolError,
                    function,
                    transaction_id,
                    started,
                    raw_tx: tx,
                    raw_rx: raw,
                    message,
                    write_outcome_unknown: false,
                    attempt,
                });
            }
        };
        match codec::validate_response(request, &response_pdu) {
            Ok(response) => TransactionResult {
                timestamp_ms: now_ms(),
                status: if response.exception.is_some() {
                    TransactionStatus::ModbusException
                } else {
                    TransactionStatus::Success
                },
                function,
                transaction_id,
                unit_id: self.config.unit_id,
                latency_ms: started.elapsed().as_millis(),
                exception_code: response.exception,
                raw_tx: tx,
                raw_rx,
                response_pdu,
                message: None,
                write_outcome_unknown: false,
                attempt,
            },
            Err(message) => self.failure(TransactionFailure {
                status: TransactionStatus::ProtocolError,
                function,
                transaction_id,
                started,
                raw_tx: tx,
                raw_rx,
                message,
                write_outcome_unknown: false,
                attempt,
            }),
        }
    }

    fn failure(&self, failure: TransactionFailure) -> TransactionResult {
        TransactionResult {
            timestamp_ms: now_ms(),
            status: failure.status,
            function: failure.function,
            transaction_id: failure.transaction_id,
            unit_id: self.config.unit_id,
            latency_ms: failure.started.elapsed().as_millis(),
            exception_code: None,
            raw_tx: failure.raw_tx,
            raw_rx: failure.raw_rx,
            response_pdu: Vec::new(),
            message: Some(failure.message),
            write_outcome_unknown: failure.write_outcome_unknown,
            attempt: failure.attempt,
        }
    }

    fn record(&self, result: TransactionResult) {
        let mut history = self
            .history
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if history.len() >= HISTORY_LIMIT {
            history.pop_front();
        }
        history.push_back(result);
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
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(runtime) = self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            runtime.join();
        }
    }
}

#[derive(Debug)]
enum ReceiveError {
    Timeout,
    Transport(String),
    Malformed(String, Vec<u8>),
    Protocol(String, Vec<u8>),
}

fn raw_success(
    unit_id: u8,
    started: Instant,
    raw_tx: Vec<u8>,
    raw_rx: Vec<u8>,
) -> TransactionResult {
    TransactionResult {
        timestamp_ms: now_ms(),
        status: TransactionStatus::Success,
        function: 0,
        transaction_id: None,
        unit_id,
        latency_ms: started.elapsed().as_millis(),
        exception_code: None,
        raw_tx,
        raw_rx,
        response_pdu: Vec::new(),
        message: Some("Raw ADU response is unvalidated".into()),
        write_outcome_unknown: false,
        attempt: 0,
    }
}

fn receive_tcp(
    events: &mpsc::Receiver<DataPlaneEvent>,
    deadline: Instant,
    transaction_id: u16,
    unit_id: u8,
) -> Result<(Vec<u8>, Vec<u8>), ReceiveError> {
    let mut framer = codec::tcp::TcpFramer::default();
    loop {
        let event = recv_until(events, deadline)?;
        match event {
            DataPlaneEvent::Closed(info) => return Err(ReceiveError::Transport(info.reason)),
            DataPlaneEvent::Data(data) => {
                let frames = framer
                    .push(&data)
                    .map_err(|error| ReceiveError::Malformed(error, data.clone()))?;
                for frame in frames {
                    let (tid, unit, pdu) = codec::tcp::decode(&frame)
                        .map_err(|error| ReceiveError::Malformed(error, frame.clone()))?;
                    if tid != transaction_id {
                        continue;
                    }
                    if unit != unit_id {
                        return Err(ReceiveError::Protocol(
                            format!("unit id mismatch: expected {unit_id}, got {unit}"),
                            frame,
                        ));
                    }
                    return Ok((frame, pdu));
                }
            }
        }
    }
}

fn receive_ascii(
    events: &mpsc::Receiver<DataPlaneEvent>,
    deadline: Instant,
    unit_id: u8,
) -> Result<(Vec<u8>, Vec<u8>), ReceiveError> {
    let mut raw = Vec::new();
    loop {
        match recv_until(events, deadline)? {
            DataPlaneEvent::Closed(info) => return Err(ReceiveError::Transport(info.reason)),
            DataPlaneEvent::Data(data) => {
                raw.extend_from_slice(&data);
                if let Some(end) = raw.windows(2).position(|window| window == b"\r\n") {
                    let frame = raw[..end + 2].to_vec();
                    let (unit, pdu) = codec::ascii::decode(&frame)
                        .map_err(|error| ReceiveError::Malformed(error, frame.clone()))?;
                    if unit != unit_id {
                        return Err(ReceiveError::Protocol(
                            format!("unit id mismatch: expected {unit_id}, got {unit}"),
                            frame,
                        ));
                    }
                    return Ok((frame, pdu));
                }
            }
        }
    }
}

fn receive_rtu(
    events: &mpsc::Receiver<DataPlaneEvent>,
    deadline: Instant,
    inter_char_gap: Duration,
    frame_gap: Duration,
    unit_id: u8,
) -> Result<(Vec<u8>, Vec<u8>), ReceiveError> {
    let mut raw = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReceiveError::Timeout);
        }
        let wait = if raw.is_empty() {
            remaining
        } else {
            remaining.min(inter_char_gap)
        };
        match events.recv_timeout(wait) {
            Ok(DataPlaneEvent::Closed(info)) => return Err(ReceiveError::Transport(info.reason)),
            Ok(DataPlaneEvent::Data(data)) => raw.extend_from_slice(&data),
            Err(mpsc::RecvTimeoutError::Timeout) if raw.is_empty() => {
                return Err(ReceiveError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let tail = frame_gap.saturating_sub(inter_char_gap);
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining < tail {
                    return Err(ReceiveError::Malformed(
                        "RTU response did not reach the required t3.5 frame boundary before timeout"
                            .into(),
                        raw,
                    ));
                }
                match events.recv_timeout(tail) {
                    Ok(DataPlaneEvent::Data(data)) => {
                        raw.extend_from_slice(&data);
                        return Err(ReceiveError::Malformed(
                            "RTU inter-character silence exceeded t1.5 before frame completion"
                                .into(),
                            raw,
                        ));
                    }
                    Ok(DataPlaneEvent::Closed(info)) => {
                        return Err(ReceiveError::Transport(info.reason))
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(ReceiveError::Transport(
                            "transport event stream closed".into(),
                        ))
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let (unit, pdu) = codec::rtu::decode(&raw)
                            .map_err(|error| ReceiveError::Malformed(error, raw.clone()))?;
                        if unit != unit_id {
                            return Err(ReceiveError::Protocol(
                                format!("unit id mismatch: expected {unit_id}, got {unit}"),
                                raw,
                            ));
                        }
                        return Ok((raw, pdu));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ReceiveError::Transport(
                    "transport event stream closed".into(),
                ))
            }
        }
    }
}

fn recv_until(
    events: &mpsc::Receiver<DataPlaneEvent>,
    deadline: Instant,
) -> Result<DataPlaneEvent, ReceiveError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ReceiveError::Timeout);
    }
    events.recv_timeout(remaining).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => ReceiveError::Timeout,
        mpsc::RecvTimeoutError::Disconnected => {
            ReceiveError::Transport("transport event stream closed".into())
        }
    })
}

fn request_supported_on_mode(mode: ModbusMode, request: &ModbusRequest) -> bool {
    if mode != ModbusMode::Tcp {
        return true;
    }
    !matches!(
        request,
        ModbusRequest::ReadExceptionStatus
            | ModbusRequest::Diagnostics { .. }
            | ModbusRequest::GetCommEventCounter
            | ModbusRequest::GetCommEventLog
            | ModbusRequest::ReportServerId
    )
}

fn mode(value: ModbusMode) -> AduMode {
    match value {
        ModbusMode::Rtu => AduMode::Rtu,
        ModbusMode::Ascii => AduMode::Ascii,
        ModbusMode::Tcp => AduMode::Tcp,
    }
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
    use crate::plugins::modbus::config::{ModbusConfig, ModbusRole};
    use crate::transport::{BlockingByteStream, ReadStatus, TransportError};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    struct IdleDriver {
        writes: Arc<AtomicUsize>,
    }

    impl BlockingByteStream for IdleDriver {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, _data: &[u8]) -> Result<(), TransportError> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn client_with(mut config: ModbusConfig) -> (ModbusClient, Arc<AtomicUsize>) {
        config.role = ModbusRole::Client;
        config.response_timeout_ms = 15;
        if matches!(config.mode, ModbusMode::Rtu | ModbusMode::Ascii)
            && config.serial_port.is_empty()
        {
            config.serial_port = "test".into();
        }
        let writes = Arc::new(AtomicUsize::new(0));
        let runtime = DataPlaneRuntime::spawn(Box::new(IdleDriver {
            writes: writes.clone(),
        }));
        (
            ModbusClient::new(config.validated().unwrap(), runtime).unwrap(),
            writes,
        )
    }

    #[test]
    fn read_timeout_retries_to_configured_budget() {
        let config = ModbusConfig {
            mode: ModbusMode::Tcp,
            read_retries: 1,
            ..Default::default()
        };
        let (client, writes) = client_with(config);
        let result = client.execute(ModbusRequest::ReadRegisters {
            area: RegisterReadArea::HoldingRegisters,
            address: 0,
            quantity: 1,
        });
        assert!(matches!(result.status, TransactionStatus::Timeout));
        assert_eq!(result.attempt, 1);
        assert_eq!(writes.load(Ordering::Relaxed), 2);
        client.shutdown();
    }

    #[test]
    fn write_timeout_is_not_retried_by_default_and_outcome_is_unknown() {
        let config = ModbusConfig {
            mode: ModbusMode::Tcp,
            read_retries: 3,
            retry_writes: false,
            ..Default::default()
        };
        let (client, writes) = client_with(config);
        let result = client.execute(ModbusRequest::WriteSingleRegister {
            address: 7,
            value: 42,
        });
        assert!(matches!(result.status, TransactionStatus::Timeout));
        assert_eq!(result.attempt, 0);
        assert!(result.write_outcome_unknown);
        assert_eq!(writes.load(Ordering::Relaxed), 1);
        client.shutdown();
    }

    #[test]
    fn serial_unit_zero_write_is_broadcast_and_read_is_rejected() {
        let config = ModbusConfig {
            mode: ModbusMode::Rtu,
            unit_id: 0,
            ..Default::default()
        };
        let (client, writes) = client_with(config);
        let write = client.execute(ModbusRequest::WriteSingleRegister {
            address: 7,
            value: 42,
        });
        assert!(matches!(write.status, TransactionStatus::Broadcast));
        assert!(!write.write_outcome_unknown);
        assert_eq!(writes.load(Ordering::Relaxed), 1);
        let read = client.execute(ModbusRequest::ReadRegisters {
            area: RegisterReadArea::HoldingRegisters,
            address: 0,
            quantity: 1,
        });
        assert!(matches!(read.status, TransactionStatus::ProtocolError));
        assert_eq!(writes.load(Ordering::Relaxed), 1);
        client.shutdown();
    }

    #[test]
    fn tcp_skips_stale_tid_and_accepts_current_frame_from_same_chunk() {
        let (tx, rx) = mpsc::channel();
        let stale = codec::tcp::encode(7, 1, &[0x03, 2, 0, 1]).unwrap();
        let current = codec::tcp::encode(8, 1, &[0x03, 2, 0, 2]).unwrap();
        let mut chunk = stale;
        chunk.extend_from_slice(&current);
        tx.send(DataPlaneEvent::Data(chunk)).unwrap();
        let (raw, pdu) = receive_tcp(&rx, Instant::now() + Duration::from_secs(1), 8, 1).unwrap();
        assert_eq!(raw, current);
        assert_eq!(pdu, vec![0x03, 2, 0, 2]);
    }

    #[test]
    fn serial_only_functions_are_rejected_for_tcp_core() {
        assert!(!request_supported_on_mode(
            ModbusMode::Tcp,
            &ModbusRequest::GetCommEventCounter
        ));
        assert!(request_supported_on_mode(
            ModbusMode::Tcp,
            &ModbusRequest::ReadRegisters {
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            }
        ));
    }
}
