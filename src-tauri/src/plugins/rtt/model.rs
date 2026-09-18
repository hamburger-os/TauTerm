use super::error::RttError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RttPhase {
    Idle,
    OpeningBackend,
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
    pub control_block_address: Option<String>,
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
    /// Monotonic process-local runtime identity. Every reconnect gets a new generation.
    pub generation: u64,
    pub phase: RttPhase,
    pub backend: Option<RttBackendDescriptor>,
    pub channels: Vec<RttChannelInfo>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub dropped_history_bytes: u64,
    pub dropped_history_chunks: u64,
    /// Loss in the bounded Auto Reply/Lua receive queue. Raw history/logging are independent.
    pub dropped_automation_bytes: u64,
    pub dropped_automation_chunks: u64,
    /// Loss in the best-effort WebView presentation queue only. History/recording are independent.
    pub dropped_presentation_bytes: u64,
    pub dropped_presentation_chunks: u64,
    pub last_error: Option<RttError>,
}

impl Default for RttSnapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: RttPhase::Idle,
            backend: None,
            channels: Vec::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            dropped_history_bytes: 0,
            dropped_history_chunks: 0,
            dropped_automation_bytes: 0,
            dropped_automation_chunks: 0,
            dropped_presentation_bytes: 0,
            dropped_presentation_chunks: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RttReadChunk {
    pub channel_index: u32,
    pub data: Vec<u8>,
}

/// Canonical acquisition record. Sequence/timestamp/offset are assigned as soon as bytes leave the
/// backend, before presentation batching, so cross-channel ordering is never reconstructed later.
#[derive(Debug, Clone)]
pub struct StoredRttChunk {
    pub generation: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub channel_index: u32,
    pub channel_offset: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttChunkDto {
    pub generation: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub channel_index: u32,
    pub channel_offset: u64,
    pub data_b64: String,
}

impl RttChunkDto {
    pub fn from_stored(chunk: &StoredRttChunk) -> Self {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
        Self {
            generation: chunk.generation,
            sequence: chunk.sequence,
            timestamp_ms: chunk.timestamp_ms,
            channel_index: chunk.channel_index,
            channel_offset: chunk.channel_offset,
            data_b64: BASE64.encode(&chunk.data),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RttHistoryResponse {
    pub generation: u64,
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
