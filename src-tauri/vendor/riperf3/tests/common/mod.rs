//! Shared lib-test helpers (`mod common;` per test binary).
//!
//! [TauTerm fork] `free_port` was re-exported from the upstream workspace's
//! dev-only `riperf3-test-support` crate (#192), which is not published on
//! crates.io; vendored here as a port-0 bind probe.

#![allow(dead_code)] // each test binary uses a subset

use std::net::TcpListener;

/// Allocate a free port by binding to 0 and letting the OS pick, then
/// release it for the test's own bind. The window between release and
/// reuse is tiny and the OS hands distinct ephemeral ports to concurrent
/// probes — adequate for local loopback tests.
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("failed to allocate a free port")
        .local_addr()
        .expect("bound socket has no local addr")
        .port()
}
