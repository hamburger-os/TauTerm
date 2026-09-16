mod jlink_existing;
mod probe_rs;

use super::config::{RttBackendKind, RttConfig};
use super::error::RttError;
use super::model::{RttBackendDescriptor, RttChannelInfo, RttProbeInfo, RttReadChunk};

pub trait RttBackend {
    fn descriptor(&self) -> RttBackendDescriptor;
    fn channels(&self) -> &[RttChannelInfo];
    fn poll(&mut self, output: &mut Vec<RttReadChunk>) -> Result<(), RttError>;
    fn write(&mut self, channel_index: u32, data: &[u8]) -> Result<usize, RttError>;
    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError>;
    fn shutdown(&mut self) {}
}

pub fn open_backend(config: &RttConfig) -> Result<Box<dyn RttBackend>, RttError> {
    match config.backend {
        RttBackendKind::ProbeRs => Ok(Box::new(probe_rs::ProbeRsRttBackend::open(config)?)),
        RttBackendKind::JlinkExisting => Ok(Box::new(
            jlink_existing::JlinkExistingRttBackend::open(config)?,
        )),
    }
}

pub fn list_probes() -> Vec<RttProbeInfo> {
    probe_rs::list_probes()
}
