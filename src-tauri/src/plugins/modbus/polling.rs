use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::plugins::modbus::client::{ModbusClient, TransactionResult, TransactionStatus};
use crate::plugins::modbus::codec::ModbusRequest;
use crate::plugins::modbus::value::{decode_register_bytes, ValueFormat};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchRow {
    pub id: String,
    pub enabled: bool,
    pub name: String,
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

    pub fn validate_rows(rows: &[WatchRow]) -> Result<(), String> {
        let mut ids = std::collections::HashSet::with_capacity(rows.len());
        for row in rows {
            if row.id.trim().is_empty() {
                return Err("watch row id cannot be empty".into());
            }
            if !ids.insert(row.id.as_str()) {
                return Err(format!("duplicate watch row id: {}", row.id));
            }
            if row.period_ms < 20 || row.period_ms > 86_400_000 {
                return Err(format!(
                    "watch row {} period must be 20..=86400000 ms",
                    row.id
                ));
            }
            match &row.request {
                ModbusRequest::ReadBits { function, .. } if matches!(function, 0x01 | 0x02) => {}
                ModbusRequest::ReadRegisters { function, .. }
                    if matches!(function, 0x03 | 0x04) => {}
                _ => {
                    return Err(format!(
                        "watch row {} must use read function 01/02/03/04",
                        row.id
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn set_rows(&self, rows: Vec<WatchRow>) -> Result<(), String> {
        Self::validate_rows(&rows)?;
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
                    let result = client.execute(row.request.clone());
                    let value = watch_value(&row, &result);
                    values
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.id.clone(), value);
                    // Schedule from completion/current time. Missed ticks are never accumulated.
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
    }
}

fn watch_value(row: &WatchRow, result: &TransactionResult) -> WatchValue {
    let raw =
        if matches!(result.status, TransactionStatus::Success) && !result.response_pdu.is_empty() {
            extract_data(&result.response_pdu)
        } else {
            Vec::new()
        };
    let value = row
        .format
        .as_ref()
        .and_then(|format| decode_register_bytes(&raw, format).ok());
    WatchValue {
        row_id: row.id.clone(),
        status: format!("{:?}", result.status).to_ascii_lowercase(),
        value,
        raw,
        latency_ms: result.latency_ms,
        message: result.message.clone(),
        updated_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
    }
}

fn extract_data(pdu: &[u8]) -> Vec<u8> {
    if pdu.len() >= 2 && matches!(pdu[0], 0x01 | 0x02 | 0x03 | 0x04 | 0x17) {
        let count = pdu[1] as usize;
        if pdu.len() >= 2 + count {
            return pdu[2..2 + count].to_vec();
        }
    }
    pdu.get(1..).unwrap_or_default().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, period_ms: u64) -> WatchRow {
        WatchRow {
            id: id.into(),
            enabled: true,
            name: id.into(),
            request: ModbusRequest::ReadRegisters {
                function: 3,
                address: 0,
                quantity: 1,
            },
            period_ms,
            format: None,
        }
    }

    #[test]
    fn watch_rows_reject_duplicate_ids_and_invalid_periods() {
        assert!(WatchScheduler::validate_rows(&[row("a", 19)]).is_err());
        assert!(WatchScheduler::validate_rows(&[row("a", 1000), row("a", 1000)]).is_err());
        assert!(WatchScheduler::validate_rows(&[row("a", 1000)]).is_ok());
    }

    #[test]
    fn watch_rows_reject_write_requests() {
        let mut invalid = row("write", 1000);
        invalid.request = ModbusRequest::WriteSingle {
            function: 6,
            address: 0,
            value: 1,
        };
        assert!(WatchScheduler::validate_rows(&[invalid]).is_err());
    }
}
