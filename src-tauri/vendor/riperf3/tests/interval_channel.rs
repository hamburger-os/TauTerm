//! [TauTerm fork] `interval_channel` live-stream regression tests.
//!
//! Verifies the fork's per-interval channel: hosts must receive each interval
//! DURING the run (per-second cadence), not as one batch after
//! `run()` / `run_once()` returns. A host config without `-J` /
//! `--json-stream` (no collector) is the TauTerm shape, so these tests run
//! plain text mode with only `interval_channel` set.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use riperf3::{ClientBuilder, ServerBuilder, TransportProtocol};

mod common;

fn next_port() -> u16 {
    common::free_port()
}

/// Consumer thread: records the arrival time of each interval since `start`.
/// Exits when all senders are dropped; returns arrival timestamps.
fn spawn_consumer(
    rx: Receiver<riperf3::json_report::Interval>,
    start: Instant,
) -> std::thread::JoinHandle<Vec<Duration>> {
    std::thread::spawn(move || {
        let mut arrivals = Vec::new();
        while let Ok(_interval) = rx.recv() {
            arrivals.push(start.elapsed());
        }
        arrivals
    })
}

/// Live-ness assertions shared by all roles/protocols: intervals must start
/// arriving well before the run ends (first interval lands inside the run).
/// `report_len` additionally checks channel↔Report parity when the reporting
/// side's own Report carries intervals (channel set on the same side).
fn assert_live(arrivals: &[Duration], run_elapsed: Duration, report_len: Option<usize>) {
    assert!(
        arrivals.len() >= 2,
        "expected >=2 live intervals, got {} (run {run_elapsed:?})",
        arrivals.len()
    );
    assert!(
        arrivals[0] < run_elapsed.saturating_sub(Duration::from_millis(500)),
        "first interval arrived at {:?}, but run() completed at {:?} — \
         intervals were batched at the end, not streamed live",
        arrivals[0],
        run_elapsed
    );
    if let Some(report_len) = report_len {
        assert_eq!(
            arrivals.len(),
            report_len,
            "channel delivered {} intervals, final Report has {}",
            arrivals.len(),
            report_len
        );
    }
}

/// Client side, TCP: a 3s run must stream ~3 intervals to the channel while
/// `run()` is still executing.
#[tokio::test]
async fn client_intervals_arrive_live() {
    let port = next_port();
    let server = ServerBuilder::new()
        .port(Some(port))
        .one_off(true)
        .build()
        .unwrap();
    let server_task = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let client = ClientBuilder::new("127.0.0.1")
        .port(Some(port))
        .duration(3)
        .interval(1.0)
        .interval_channel(tx)
        .build()
        .unwrap();
    let report = client.run().await.expect("client run failed");
    let run_elapsed = start.elapsed();
    drop(client);
    let arrivals = consumer.join().unwrap();

    assert_live(&arrivals, run_elapsed, Some(report.intervals.len()));
    let _ = server_task.await;
}

/// Client side, UDP: same live-streaming guarantee over the UDP data path
/// (receiver measures jitter/loss per interval).
#[tokio::test]
async fn udp_client_intervals_arrive_live() {
    let port = next_port();
    let server = ServerBuilder::new()
        .port(Some(port))
        .one_off(true)
        .build()
        .unwrap();
    let server_task = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let client = ClientBuilder::new("127.0.0.1")
        .port(Some(port))
        .protocol(TransportProtocol::Udp)
        .bandwidth(1_000_000)
        .duration(3)
        .interval(1.0)
        .interval_channel(tx)
        .build()
        .unwrap();
    let report = client.run().await.expect("UDP client run failed");
    let run_elapsed = start.elapsed();
    drop(client);
    let arrivals = consumer.join().unwrap();

    assert_live(&arrivals, run_elapsed, Some(report.intervals.len()));
    let _ = server_task.await;
}

/// Quiet-mode smoke test: `quiet(true)` on both sides must not break the
/// live interval channel — the structured events are independent of the
/// console printers that quiet suppresses.
#[tokio::test]
async fn quiet_mode_keeps_live_channel() {
    let port = next_port();
    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let server = ServerBuilder::new()
        .port(Some(port))
        .one_off(true)
        .interval_channel(tx)
        .quiet(true)
        .build()
        .unwrap();
    let server_task = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client = ClientBuilder::new("127.0.0.1")
        .port(Some(port))
        .duration(3)
        .interval(1.0)
        .quiet(true)
        .build()
        .unwrap();
    let _ = client.run().await.expect("quiet client run failed");
    let _ = server_task.await.expect("quiet server run failed");
    let run_elapsed = start.elapsed();
    let arrivals = consumer.join().unwrap();

    // The client (no channel, no -J) has no interval list to cross-check
    // against — assert live-ness only.
    assert_live(&arrivals, run_elapsed, None);
}

/// Server side, TCP: the server's channel streams each interval while
/// `run()` (one_off) is still serving the client.
#[tokio::test]
async fn server_intervals_arrive_live() {
    let port = next_port();
    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let server = ServerBuilder::new()
        .port(Some(port))
        .one_off(true)
        .interval_channel(tx)
        .build()
        .unwrap();
    let server_task = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client = ClientBuilder::new("127.0.0.1")
        .port(Some(port))
        .duration(3)
        .interval(1.0)
        .build()
        .unwrap();
    let _ = client.run().await.expect("client run failed");
    let _ = server_task.await.expect("server run failed");
    let run_elapsed = start.elapsed();
    let arrivals = consumer.join().unwrap();

    // The server's Report is not exposed by run(); the client (no channel)
    // has no interval list to cross-check against — assert live-ness only.
    assert_live(&arrivals, run_elapsed, None);
}

// ---------------------------------------------------------------------------
// Official iperf3 binary interop (env-gated, #[ignore] by default)
// ---------------------------------------------------------------------------
//
// Wire-compatibility checks against the real iperf3 CLI. Start the official
// server first, then run one test at a time:
//
//   iperf3.exe -s -p 5210 -1
//   IPERF3_SERVER_PORT=5210 cargo test --test interval_channel \
//       official_iperf3_client_interop -- --ignored --nocapture
//
//   IPERF3_EXE="C:\\Programs\\iperf\\iperf3.exe" cargo test --test \
//       interval_channel official_iperf3_server_interop -- --ignored --nocapture

/// Fork client → official server: the channel must stream intervals live
/// during a test against the real iperf3 CLI.
#[tokio::test]
#[ignore = "requires an official iperf3 server: iperf3.exe -s -p 5210 -1"]
async fn official_iperf3_client_interop() {
    let port: u16 = std::env::var("IPERF3_SERVER_PORT")
        .unwrap_or_else(|_| "5210".into())
        .parse()
        .expect("IPERF3_SERVER_PORT must be a port number");

    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let client = ClientBuilder::new("127.0.0.1")
        .port(Some(port))
        .duration(3)
        .interval(1.0)
        .interval_channel(tx)
        .build()
        .unwrap();
    let report = client.run().await.expect("interop run failed");
    let run_elapsed = start.elapsed();
    drop(client);
    let arrivals = consumer.join().unwrap();

    assert_live(&arrivals, run_elapsed, Some(report.intervals.len()));
    assert!(
        report.end.sum_sent.map(|s| s.bytes).unwrap_or(0) > 0,
        "no data transferred against the official server"
    );
}

/// Official client → fork server (shell-driven): the test hosts the fork
/// server (with channel) on a FIXED port and waits up to 90s for the real
/// iperf3 CLI client, then asserts the server-side channel streamed live.
///
/// NOTE (Windows): the official client MUST be launched from a separate
/// shell, NOT spawned by this test process — a Cygwin iperf3 client spawned
/// by the very process hosting the riperf3 server hangs before its cookie
/// write (verified: in-process server + test-spawned client = hang; separate
/// shell or separate server process = works). Real users always launch the
/// client from their own shell, so this does not affect the library.
#[tokio::test]
#[ignore = "shell-driven: run `iperf3.exe -c 127.0.0.1 -p 5213 -t 3 -i 1` while this test runs"]
async fn official_iperf3_server_interop() {
    let port: u16 = 5213;

    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let consumer = spawn_consumer(rx, start);
    let server = ServerBuilder::new()
        .port(Some(port))
        .one_off(true)
        .interval_channel(tx)
        .build()
        .unwrap();
    println!(
        "[interop] fork server listening on {port} — run in another shell: \
         iperf3.exe -c 127.0.0.1 -p {port} -t 3 -i 1"
    );
    let server_result = tokio::time::timeout(Duration::from_secs(90), server.run()).await;
    drop(server);
    let run_elapsed = start.elapsed();
    let arrivals = consumer.join().unwrap();

    assert!(
        server_result.is_ok(),
        "no official client connected within 90s"
    );
    assert_live(&arrivals, run_elapsed, None);
}
