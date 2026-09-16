use super::error::RttError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RttPhase {
    Idle,
    OpeningProbe,
    AttachingTarget,
    LocatingControlBlock,
    Running,
    Faulted,
    Stopping,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RttBackendCapabilities {
    pub enumerate_channels: bool,
    pub channel_metadata: bool,
    pub locator: bool,
    pub direct_target_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttBackendDescriptor {
    pub kind: String,
    pub display_name: String,
    pub target: Option<String>,
    pub probe: Option<String>,
    pub control_block_address: Option<u64>,
    pub capabilities: RttBackendCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttChannelDirectionInfo {
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttChannelInfo {
    pub index: u32,
    pub name: Option<String>,
    pub up: Option<RttChannelDirectionInfo>,
    pub down: Option<RttChannelDirectionInfo>,
    pub metadata_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttSnapshot {
    pub phase: RttPhase,
    pub backend: Option<RttBackendDescriptor>,
    pub channels: Vec<RttChannelInfo>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub dropped_history_bytes: u64,
    pub dropped_history_chunks: u64,
    pub last_error: Option<RttError>,
}

impl Default for RttSnapshot {
    fn default() -> Self {
        Self {
            phase: RttPhase::Idle,
            backend: None,
            channels: Vec::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            dropped_history_bytes: 0,
            dropped_history_chunks: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RttReadChunk {
    pub channel_index: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StoredRttChunk {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub channel_index: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttChunkDto {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub channel_index: u32,
    pub data_b64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttHistoryResponse {
    pub chunks: Vec<RttChunkDto>,
    pub oldest_sequence: Option<u64>,
    pub newest_sequence: Option<u64>,
    pub dropped_chunks: u64,
    pub dropped_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttProbeInfo {
    pub selector: String,
    pub display_name: String,
    pub identifier: String,
    pub serial_number: Option<String>,
}
