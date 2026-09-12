use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::plugins::modbus::capability::validate_client_unit_id;
use crate::plugins::modbus::client::{ModbusClient, TransactionResult, TransactionStatus};
use crate::plugins::modbus::codec::ModbusRequest;
use crate::plugins::modbus::config::ModbusMode;
use crate::plugins::modbus::response::SemanticResponse;
use crate::plugins::modbus::value::{decode_register_bytes, required_registers, ValueFormat};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchRow {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub unit_id: u8,
    pub request: ModbusRequest,
    #[serde(default = "default_period")]
    pub period_ms: u64,
    #[serde(default)]
    pub format: Option<ValueFormat>,
}

fn default_period() -> u64 {
    1000
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchValue {
    pub row_id: String,
    pub status: TransactionStatus,
    pub value: Option<serde_json::Value>,
    pub raw: Vec<u8>,
    pub latency_ms: u128,
    pub message: Option<String>,
    pub updated_at_ms: u64,
}

pub struct WatchScheduler {
    client: Arc<ModbusClient>,
    rows: Arc<Mutex<Vec<WatchRow>>>,
    values: Arc<Mutex<HashMap<String, WatchValue>>>,
    running: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatchScheduler {
    pub fn new(client: Arc<ModbusClient>) -> Self {
        Self {
            client,
            rows: Arc::new(Mutex::new(Vec::new())),
            values: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    pub fn validate_rows(rows: &[WatchRow], mode: ModbusMode) -> Result<(), String> {
        let mut ids = std::collections::HashSet::with_capacity(rows.len());
        for row in rows {
            if row.id.trim().is_empty() {
                return Err("watch row id cannot be empty".into());
            }
            if !ids.insert(row.id.as_str()) {
                return Err(format!("duplicate watch row id: {}", row.id));
            }
            validate_client_unit_id(mode, row.unit_id)
                .map_err(|error| format!("watch row {} invalid target: {error}", row.id))?;
            if row.period_ms < 20 || row.period_ms > 86_400_000 {
                return Err(format!(
                    "watch row {} period must be 20..=86400000 ms",
                    row.id
                ));
            }
            crate::plugins::modbus::codec::encode_request(&row.request)
                .map_err(|error| format!("watch row {} invalid request: {error}", row.id))?;
            match &row.request {
                ModbusRequest::ReadBits { .. } => {
                    if row.format.is_some() {
                        return Err(format!(
                            "watch row {} bit area must not use register value format",
                            row.id
                        ));
                    }
                }
                ModbusRequest::ReadRegisters { quantity, .. } => {
                    let format = row.format.as_ref().ok_or_else(|| {
                        format!("watch row {} register area requires a value format", row.id)
                    })?;
                    if let Some(required) = required_registers(format) {
                        if *quantity != required {
                            return Err(format!(
                                "watch row {} value format requires exactly {required} register(s)",
                                row.id
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "watch row {} must use a standard read area",
                        row.id
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn set_rows(&self, rows: Vec<WatchRow>) -> Result<(), String> {
        Self::validate_rows(&rows, self.client.mode())?;
        let retained_ids: std::collections::HashSet<_> =
            rows.iter().map(|row| row.id.as_str()).collect();
        self.values
            .lock()
            .map_err(|e| e.to_string())?
            .retain(|id, _| retained_ids.contains(id.as_str()));
        *self.rows.lock().map_err(|e| e.to_string())? = rows;
        Ok(())
    }

    pub fn rows(&self) -> Vec<WatchRow> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn values(&self) -> Vec<WatchValue> {
        let mut values: Vec<_> = self
            .values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        values.sort_by_key(|value| value.updated_at_ms);
        values
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn start(&self) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let running = self.running.clone();
        let rows = self.rows.clone();
        let values = self.values.clone();
        let client = self.client.clone();
        let worker = std::thread::spawn(move || {
            let mut next: HashMap<String, Instant> = HashMap::new();
            while running.load(Ordering::Acquire) {
                let snapshot = rows.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let live_ids: std::collections::HashSet<_> =
                    snapshot.iter().map(|row| row.id.clone()).collect();
                next.retain(|id, _| live_ids.contains(id));
                let now = Instant::now();
                for row in snapshot.into_iter().filter(|row| row.enabled) {
                    let due = next.get(&row.id).copied().unwrap_or(now);
                    if now < due {
                        continue;
                    }
                    let result = client.execute(row.unit_id, row.request.clone());
                    let value = watch_value(&row, &result);
                    values
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.id.clone(), value);
                    next.insert(
                        row.id.clone(),
                        Instant::now() + Duration::from_millis(row.period_ms),
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WatchScheduler {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        let worker = match self.worker.get_mut() {
            Ok(worker) => worker.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

fn watch_value(row: &WatchRow, result: &TransactionResult) -> WatchValue {
    let (raw, value) = if result.status != TransactionStatus::Success {
        (Vec::new(), None)
    } else {
        match result.semantic_response.as_ref() {
            Some(SemanticResponse::Bits { values }) => {
                let value = if values.len() == 1 {
                    values.first().copied().map(serde_json::Value::Bool)
                } else {
                    Some(serde_json::Value::Array(
                        values
                            .iter()
                            .copied()
                            .map(serde_json::Value::Bool)
                            .collect(),
                    ))
                };
                (values.iter().map(|value| u8::from(*value)).collect(), value)
            }
            Some(SemanticResponse::Registers { values }) => {
                let raw = values
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect::<Vec<_>>();
                let value = row
                    .format
                    .as_ref()
                    .and_then(|format| decode_register_bytes(&raw, format).ok());
                (raw, value)
            }
            _ => (Vec::new(), None),
        }
    };

    WatchValue {
        row_id: row.id.clone(),
        status: result.status,
        value,
        raw,
        latency_ms: result.latency_ms,
        message: result.message.clone(),
        updated_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::modbus::codec::{BitReadArea, RegisterReadArea};
    use crate::plugins::modbus::config::{ModbusConfig, ModbusRole};
    use crate::plugins::modbus::value::{ByteOrder, ValueType, WordOrder};
    use crate::transport::{BlockingByteStream, DataPlaneRuntime, ReadStatus, TransportError};

    struct IdleDriver;

    impl BlockingByteStream for IdleDriver {
        fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
            std::thread::sleep(Duration::from_millis(1));
            Ok(ReadStatus::Idle)
        }

        fn write_all(&mut self, _data: &[u8]) -> Result<(), TransportError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn register_format(value_type: ValueType) -> ValueFormat {
        ValueFormat {
            value_type,
            byte_order: ByteOrder::Big,
            word_order: WordOrder::Normal,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
            bit: None,
        }
    }

    fn row(id: &str, period_ms: u64) -> WatchRow {
        WatchRow {
            id: id.into(),
            enabled: true,
            name: id.into(),
            unit_id: 1,
            request: ModbusRequest::ReadRegisters {
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            },
            period_ms,
            format: Some(register_format(ValueType::UInt16)),
        }
    }

    fn client() -> Arc<ModbusClient> {
        let config = ModbusConfig {
            mode: ModbusMode::Rtu,
            role: ModbusRole::Client,
            serial_port: "test".into(),
            response_timeout_ms: 5,
            ..Default::default()
        };
        Arc::new(
            ModbusClient::new(
                config.validated().expect("valid polling test config"),
                DataPlaneRuntime::spawn(Box::new(IdleDriver)),
            )
            .expect("client runtime"),
        )
    }

    #[test]
    fn watch_rows_reject_duplicate_ids_and_invalid_periods() {
        assert!(WatchScheduler::validate_rows(&[row("a", 19)], ModbusMode::Rtu).is_err());
        assert!(
            WatchScheduler::validate_rows(&[row("a", 1000), row("a", 1000)], ModbusMode::Rtu)
                .is_err()
        );
        assert!(WatchScheduler::validate_rows(&[row("a", 1000)], ModbusMode::Rtu).is_ok());
    }

    #[test]
    fn watch_rows_reject_write_requests() {
        let mut invalid = row("write", 1000);
        invalid.request = ModbusRequest::WriteSingleRegister {
            address: 0,
            value: 1,
        };
        assert!(WatchScheduler::validate_rows(&[invalid], ModbusMode::Rtu).is_err());
    }

    #[test]
    fn watch_rows_reuse_protocol_quantity_validation() {
        let mut bits = row("bits", 1000);
        bits.request = ModbusRequest::ReadBits {
            area: BitReadArea::Coils,
            address: 0,
            quantity: 2001,
        };
        bits.format = None;
        assert!(WatchScheduler::validate_rows(&[bits], ModbusMode::Rtu).is_err());

        let mut registers = row("registers", 1000);
        registers.request = ModbusRequest::ReadRegisters {
            area: RegisterReadArea::HoldingRegisters,
            address: 0,
            quantity: 126,
        };
        registers.format = Some(register_format(ValueType::Hex));
        assert!(WatchScheduler::validate_rows(&[registers], ModbusMode::Rtu).is_err());
    }

    #[test]
    fn bit_rows_do_not_accept_register_value_format() {
        let mut invalid = row("bit", 1000);
        invalid.request = ModbusRequest::ReadBits {
            area: BitReadArea::Coils,
            address: 0,
            quantity: 1,
        };
        assert!(WatchScheduler::validate_rows(&[invalid.clone()], ModbusMode::Rtu).is_err());
        invalid.format = None;
        assert!(WatchScheduler::validate_rows(&[invalid], ModbusMode::Rtu).is_ok());
    }

    #[test]
    fn fixed_scalar_width_must_match_request_quantity() {
        let mut invalid = row("float", 1000);
        invalid.format = Some(register_format(ValueType::Float32));
        assert!(WatchScheduler::validate_rows(&[invalid.clone()], ModbusMode::Rtu).is_err());
        invalid.request = ModbusRequest::ReadRegisters {
            area: RegisterReadArea::HoldingRegisters,
            address: 0,
            quantity: 2,
        };
        assert!(WatchScheduler::validate_rows(&[invalid], ModbusMode::Rtu).is_ok());
    }

    #[test]
    fn serial_watch_rejects_reserved_unit() {
        let mut invalid = row("unit", 1000);
        invalid.unit_id = 248;
        assert!(WatchScheduler::validate_rows(&[invalid], ModbusMode::Rtu).is_err());
    }

    #[test]
    fn watch_runtime_stays_running_until_explicit_stop() {
        let client = client();
        let scheduler = WatchScheduler::new(client.clone());
        scheduler.set_rows(vec![row("live", 20)]).unwrap();
        scheduler.start();
        std::thread::sleep(Duration::from_millis(30));
        assert!(scheduler.is_running());
        scheduler.stop();
        assert!(!scheduler.is_running());
        client.shutdown();
    }
}
