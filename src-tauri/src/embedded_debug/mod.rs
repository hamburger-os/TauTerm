//! Shared embedded-debug ownership primitives.
//!
//! This layer owns vendor-neutral probe/target attachment mechanics that are already common to
//! RTT and the planned variable-observation workflows. Protocol-specific RTT, memory sampling and
//! trace decoding remain in their own modules.

pub mod firmware_artifact;
pub mod observation;
pub mod probe_runtime;
