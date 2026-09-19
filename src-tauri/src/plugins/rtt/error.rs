use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RttErrorCode {
    ProbeNotFound,
    ProbeAmbiguous,
    ProbeBusy,
    ProbePermissionDenied,
    TargetNotFound,
    TargetAttachFailed,
    UnsupportedWireProtocol,
    CoreNotFound,
    FirmwareFileUnavailable,
    FirmwareArtifactInvalid,
    RttNotInitialized,
    RttControlBlockNotFound,
    RttMultipleControlBlocks,
    RttInvalidControlBlock,
    RttChannelNotFound,
    RttWriteFailed,
    RttWriteQueueFull,
    RttWriteTimeout,
    JlinkServerUnavailable,
    JlinkChannelConfigFailed,
    TargetDisconnected,
    ProbeDisconnected,
    SchedulerBusy,
    OperationTimeout,
    OperationOutcomeUnknown,
    InvalidConfig,
    BackendFault,
    Cancelled,
}

impl RttErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProbeNotFound => "probe_not_found",
            Self::ProbeAmbiguous => "probe_ambiguous",
            Self::ProbeBusy => "probe_busy",
            Self::ProbePermissionDenied => "probe_permission_denied",
            Self::TargetNotFound => "target_not_found",
            Self::TargetAttachFailed => "target_attach_failed",
            Self::UnsupportedWireProtocol => "unsupported_wire_protocol",
            Self::CoreNotFound => "core_not_found",
            Self::FirmwareFileUnavailable => "firmware_file_unavailable",
            Self::FirmwareArtifactInvalid => "firmware_artifact_invalid",
            Self::RttNotInitialized => "rtt_not_initialized",
            Self::RttControlBlockNotFound => "rtt_control_block_not_found",
            Self::RttMultipleControlBlocks => "rtt_multiple_control_blocks",
            Self::RttInvalidControlBlock => "rtt_invalid_control_block",
            Self::RttChannelNotFound => "rtt_channel_not_found",
            Self::RttWriteFailed => "rtt_write_failed",
            Self::RttWriteQueueFull => "rtt_write_queue_full",
            Self::RttWriteTimeout => "rtt_write_timeout",
            Self::JlinkServerUnavailable => "jlink_server_unavailable",
            Self::JlinkChannelConfigFailed => "jlink_channel_config_failed",
            Self::TargetDisconnected => "target_disconnected",
            Self::ProbeDisconnected => "probe_disconnected",
            Self::SchedulerBusy => "scheduler_busy",
            Self::OperationTimeout => "operation_timeout",
            Self::OperationOutcomeUnknown => "operation_outcome_unknown",
            Self::InvalidConfig => "invalid_config",
            Self::BackendFault => "backend_fault",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct RttError {
    pub code: RttErrorCode,
    pub message: String,
}

impl RttError {
    pub fn new(code: RttErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(RttErrorCode::InvalidConfig, message)
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::new(RttErrorCode::BackendFault, message)
    }

    pub fn is_transient_runtime_pressure(&self) -> bool {
        matches!(
            self.code,
            RttErrorCode::SchedulerBusy | RttErrorCode::OperationTimeout
        )
    }

    pub fn has_indeterminate_outcome(&self) -> bool {
        self.code == RttErrorCode::OperationOutcomeUnknown
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RttCommandError {
    pub code: String,
    pub message: String,
}

impl From<RttError> for RttCommandError {
    fn from(value: RttError) -> Self {
        Self {
            code: value.code.as_str().to_string(),
            message: value.message,
        }
    }
}
