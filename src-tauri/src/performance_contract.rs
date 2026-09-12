//! Ignored performance and soak contracts.
//!
//! These tests are intentionally excluded from the normal unit-test gate. Dedicated workflows run
//! them in release mode and preserve JSON output as artifacts so regressions can be compared over
//! time without making hosted-runner noise a hard release threshold.

use crate::kernel::persistence::atomic_write;
use crate::transport::{BlockingByteStream, DataPlaneRuntime, ReadStatus, TransportError};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct MemoryStream {
    written: Arc<AtomicU64>,
}

impl BlockingByteStream for MemoryStream {
    fn read(&mut self, _buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(ReadStatus::Idle)
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.written.fetch_add(data.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[derive(Serialize)]
struct PerformanceResult {
    schema_version: u32,
    io_payload_bytes: u64,
    io_elapsed_ms: u128,
    io_mib_per_sec: f64,
    atomic_writes: u64,
    atomic_write_elapsed_ms: u128,
    atomic_writes_per_sec: f64,
}

fn output_path(file: &str) -> PathBuf {
    std::env::var_os("TAUTERM_PERF_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(file))
}

#[test]
#[ignore = "performance contract: run from the dedicated performance workflow"]
fn performance_contract_runtime_io_and_atomic_persistence() {
    const TOTAL_BYTES: u64 = 32 * 1024 * 1024;
    const CHUNK_BYTES: usize = 64 * 1024;

    let written = Arc::new(AtomicU64::new(0));
    let runtime = DataPlaneRuntime::spawn(Box::new(MemoryStream {
        written: written.clone(),
    }));
    let io = runtime.handle.clone();

    let payload = vec![0xA5; CHUNK_BYTES];
    let start = Instant::now();
    let chunks = TOTAL_BYTES as usize / CHUNK_BYTES;
    for _ in 0..chunks {
        io.write(&payload).unwrap();
    }
    let tx_bytes = io.tx_bytes();
    runtime.join();
    let io_elapsed = start.elapsed();
    assert_eq!(written.load(Ordering::Relaxed), TOTAL_BYTES);
    assert_eq!(tx_bytes, TOTAL_BYTES);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("performance-state.json");
    let snapshot = vec![b'x'; 16 * 1024];
    const WRITES: u64 = 200;
    let persist_start = Instant::now();
    for _ in 0..WRITES {
        atomic_write(&path, &snapshot).unwrap();
    }
    let persist_elapsed = persist_start.elapsed();

    let io_secs = io_elapsed.as_secs_f64().max(f64::EPSILON);
    let persist_secs = persist_elapsed.as_secs_f64().max(f64::EPSILON);
    let result = PerformanceResult {
        schema_version: 2,
        io_payload_bytes: TOTAL_BYTES,
        io_elapsed_ms: io_elapsed.as_millis(),
        io_mib_per_sec: TOTAL_BYTES as f64 / (1024.0 * 1024.0) / io_secs,
        atomic_writes: WRITES,
        atomic_write_elapsed_ms: persist_elapsed.as_millis(),
        atomic_writes_per_sec: WRITES as f64 / persist_secs,
    };
    let json = serde_json::to_vec_pretty(&result).unwrap();
    let output = output_path("tauterm-performance-contract.json");
    atomic_write(&output, &json).unwrap();
    println!(
        "TAUTERM_PERFORMANCE_RESULT={}",
        String::from_utf8_lossy(&json)
    );
}

#[derive(Serialize)]
struct SoakResult {
    schema_version: u32,
    duration_seconds: u64,
    iterations: u64,
    payload_bytes: u64,
}

#[test]
#[ignore = "soak contract: run from the dedicated reliability workflow"]
fn reliability_soak_repeated_io_lifecycle() {
    let seconds = std::env::var("TAUTERM_SOAK_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(5, 28_800);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut iterations = 0_u64;
    let mut payload_bytes = 0_u64;

    while Instant::now() < deadline {
        let written = Arc::new(AtomicU64::new(0));
        let runtime = DataPlaneRuntime::spawn(Box::new(MemoryStream {
            written: written.clone(),
        }));
        let io = runtime.handle.clone();

        let payload = vec![0x5A; 4096];
        io.write(&payload).unwrap();
        let tx_bytes = io.tx_bytes();
        runtime.join();
        assert_eq!(written.load(Ordering::Relaxed), payload.len() as u64);
        assert_eq!(tx_bytes, payload.len() as u64);
        iterations += 1;
        payload_bytes += payload.len() as u64;
    }

    assert!(iterations > 0);
    let result = SoakResult {
        schema_version: 2,
        duration_seconds: seconds,
        iterations,
        payload_bytes,
    };
    let json = serde_json::to_vec_pretty(&result).unwrap();
    let output = std::env::var_os("TAUTERM_SOAK_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("tauterm-soak-contract.json"));
    atomic_write(&output, &json).unwrap();
    println!("TAUTERM_SOAK_RESULT={}", String::from_utf8_lossy(&json));
}
