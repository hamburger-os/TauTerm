mod jlink_existing;
mod probe_rs;

use super::config::{RttBackendKind, RttConfig};
use super::error::RttError;
use super::model::{RttBackendDescriptor, RttChannelInfo, RttProbeInfo, RttReadChunk};
use crate::embedded_debug::runtime::EmbeddedDebugManager;

pub trait RttBackend {
    fn descriptor(&self) -> RttBackendDescriptor;
    fn channels(&self) -> &[RttChannelInfo];
    fn poll(&mut self, output: &mut Vec<RttReadChunk>) -> Result<(), RttError>;
    fn write(&mut self, channel_index: u32, data: &[u8]) -> Result<usize, RttError>;
    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError>;
    fn shutdown(&mut self) {}
}

pub(super) fn open_backend(
    config: &RttConfig,
    embedded_debug: &EmbeddedDebugManager,
    session_id: &str,
) -> Result<Box<dyn RttBackend>, RttError> {
    match config.backend {
        RttBackendKind::ProbeRs => Ok(Box::new(probe_rs::ProbeRsRttBackend::open(
            config,
            embedded_debug,
            session_id,
        )?)),
        RttBackendKind::JlinkExisting => Ok(Box::new(
            jlink_existing::JlinkExistingRttBackend::open(config)?,
        )),
    }
}

pub fn list_probes() -> Vec<RttProbeInfo> {
    probe_rs::list_probes()
}
