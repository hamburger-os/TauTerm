use super::RttBackend;
use crate::embedded_debug::firmware_artifact::{FirmwareArtifact, FirmwareArtifactError};
use crate::embedded_debug::probe_runtime::{
    list_probes as list_debug_probes, DebugProbeConfig, DebugProbeOpenError, DebugProbeRuntime,
    DebugWireProtocol,
};
use crate::plugins::rtt::config::{RttConfig, RttLocator, RttWireProtocol};
use crate::plugins::rtt::error::{RttError, RttErrorCode};
use crate::plugins::rtt::model::{
    RttBackendCapabilities, RttBackendDescriptor, RttChannelDirectionInfo, RttChannelInfo,
    RttProbeInfo, RttReadChunk,
};
use probe_rs::rtt::{
    find_rtt_control_block_in_raw_file, try_attach_to_rtt, Error as ProbeRttError, Rtt,
    ScanRegion,
};
use std::collections::BTreeMap;
use std::time::Duration;

const CHANNEL_REFRESH_TIMEOUT_CAP: Duration = Duration::from_secs(2);

pub struct ProbeRsRttBackend {
    probe: DebugProbeRuntime,
    rtt: Rtt,
    core_index: usize,
    region: ScanRegion,
    refresh_timeout: Duration,
    channels: Vec<RttChannelInfo>,
}

pub fn list_probes() -> Vec<RttProbeInfo> {
    list_debug_probes()
        .into_iter()
        .map(|info| RttProbeInfo {
            selector: info.selector,
            display_name: info.display_name,
            identifier: info.identifier,
            serial_number: info.serial_number,
        })
        .collect()
}

impl ProbeRsRttBackend {
    pub fn open(config: &RttConfig) -> Result<Self, RttError> {
        let target = config
            .target
            .clone()
            .ok_or_else(|| RttError::invalid_config("ProbeRs 模式缺少目标芯片"))?;
        let wire_protocol = match config.wire_protocol {
            RttWireProtocol::Swd => DebugWireProtocol::Swd,
            RttWireProtocol::Jtag => DebugWireProtocol::Jtag,
        };
        let mut probe = DebugProbeRuntime::open(&DebugProbeConfig {
            selector: config.probe_selector.clone(),
            target,
            wire_protocol,
            speed_khz: config.speed_khz,
        })
        .map_err(map_probe_open_error)?;

        let region = resolve_scan_region(config)?;
        let mut core = probe.session_mut().core(config.core_index).map_err(|error| {
            RttError::new(
                RttErrorCode::CoreNotFound,
                format!("无法打开 CPU Core {}: {error}", config.core_index),
            )
        })?;
        let mut rtt = try_attach_to_rtt(&mut core, config.attach_timeout, &region)
            .map_err(map_rtt_attach_error)?;
        drop(core);
        let channels = collect_channels(&mut rtt)?;

        Ok(Self {
            probe,
            rtt,
            core_index: config.core_index,
            region,
            refresh_timeout: config.attach_timeout.min(CHANNEL_REFRESH_TIMEOUT_CAP),
            channels,
        })
    }
}
fn resolve_scan_region(config: &RttConfig) -> Result<ScanRegion, RttError> {
    match &config.locator {
        RttLocator::Auto => {
            let Some(path) = config.firmware_path.as_deref() else {
                return Ok(ScanRegion::Ram);
            };
            let artifact = FirmwareArtifact::load(path).map_err(map_firmware_artifact_error)?;
            match find_rtt_control_block_in_raw_file(artifact.bytes()).map_err(|error| {
                RttError::new(
                    RttErrorCode::FirmwareArtifactInvalid,
                    format!("无法解析固件符号文件 {path}: {error}"),
                )
            })? {
                Some(address) => Ok(ScanRegion::Exact(address)),
                None => Ok(ScanRegion::Ram),
            }
        }
        RttLocator::Exact(address) => Ok(ScanRegion::Exact(*address)),
        RttLocator::Ranges(ranges) => Ok(ScanRegion::Ranges(ranges.clone())),
    }
}

fn map_firmware_artifact_error(error: FirmwareArtifactError) -> RttError {
    RttError::new(RttErrorCode::FirmwareFileUnavailable, error.to_string())
}

fn collect_channels(rtt: &mut Rtt) -> Result<Vec<RttChannelInfo>, RttError> {
    let mut entries: BTreeMap<u32, RttChannelInfo> = BTreeMap::new();
    for channel in rtt.up_channels().iter() {
        let index = u32::try_from(channel.number())
            .map_err(|_| RttError::backend("RTT Channel index 超出 u32"))?;
        let entry = entries.entry(index).or_insert_with(|| RttChannelInfo {
            index,
            name: channel.name().map(str::to_owned),
            up: None,
            down: None,
            metadata_complete: true,
        });
        if entry.name.is_none() {
            entry.name = channel.name().map(str::to_owned);
        }
        entry.up = Some(RttChannelDirectionInfo {
            buffer_size: Some(channel.buffer_size()),
        });
    }
    for channel in rtt.down_channels().iter() {
        let index = u32::try_from(channel.number())
            .map_err(|_| RttError::backend("RTT Channel index 超出 u32"))?;
        let entry = entries.entry(index).or_insert_with(|| RttChannelInfo {
            index,
            name: channel.name().map(str::to_owned),
            up: None,
            down: None,
            metadata_complete: true,
        });
        if entry.name.is_none() {
            entry.name = channel.name().map(str::to_owned);
        }
        entry.down = Some(RttChannelDirectionInfo {
            buffer_size: Some(channel.buffer_size()),
        });
    }
    Ok(entries.into_values().collect())
}

impl RttBackend for ProbeRsRttBackend {
    fn descriptor(&self) -> RttBackendDescriptor {
        RttBackendDescriptor {
            kind: "probe_rs".into(),
            display_name: "ProbeRs".into(),
            target: Some(self.probe.target().to_string()),
            probe: Some(self.probe.probe_label().to_string()),
            control_block_address: Some(self.rtt.ptr()),
            capabilities: RttBackendCapabilities {
                enumerate_channels: true,
                channel_metadata: true,
                locator: true,
                direct_target_control: true,
            },
        }
    }

    fn channels(&self) -> &[RttChannelInfo] {
        &self.channels
    }

    fn poll(&mut self, output: &mut Vec<RttReadChunk>) -> Result<(), RttError> {
        let mut core = self.probe.session_mut().core(self.core_index).map_err(|error| {
            RttError::new(
                RttErrorCode::ProbeDisconnected,
                format!("RTT 轮询时无法访问 CPU Core: {error}"),
            )
        })?;
        let mut buffer = [0u8; 4 * 1024];
        for channel in self.rtt.up_channels().iter_mut() {
            for _ in 0..4 {
                let count = channel
                    .read(&mut core, &mut buffer)
                    .map_err(map_rtt_io_error)?;
                if count == 0 {
                    break;
                }
                output.push(RttReadChunk {
                    channel_index: channel.number() as u32,
                    data: buffer[..count].to_vec(),
                });
                if count < buffer.len() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn write(&mut self, channel_index: u32, data: &[u8]) -> Result<usize, RttError> {
        let mut core = self.probe.session_mut().core(self.core_index).map_err(|error| {
            RttError::new(
                RttErrorCode::ProbeDisconnected,
                format!("RTT 写入时无法访问 CPU Core: {error}"),
            )
        })?;
        let channel = self
            .rtt
            .down_channel(channel_index as usize)
            .ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttChannelNotFound,
                    format!("RTT Down Channel {channel_index} 不存在"),
                )
            })?;
        channel.write(&mut core, data).map_err(map_rtt_io_error)
    }

    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
        let mut core = self.probe.session_mut().core(self.core_index).map_err(|error| {
            RttError::new(
                RttErrorCode::ProbeDisconnected,
                format!("刷新 RTT Channel 时无法访问 CPU Core: {error}"),
            )
        })?;
        let mut refreshed = try_attach_to_rtt(&mut core, self.refresh_timeout, &self.region)
            .map_err(map_rtt_attach_error)?;
        drop(core);
        let channels = collect_channels(&mut refreshed)?;
        self.rtt = refreshed;
        self.channels = channels;
        Ok(self.channels.clone())
    }
}

fn map_probe_open_error(error: DebugProbeOpenError) -> RttError {
    let code = match error {
        DebugProbeOpenError::NotFound | DebugProbeOpenError::Open(_) => RttErrorCode::ProbeNotFound,
        DebugProbeOpenError::Ambiguous => RttErrorCode::ProbeAmbiguous,
        DebugProbeOpenError::Busy(_) => RttErrorCode::ProbeBusy,
        DebugProbeOpenError::PermissionDenied(_) => RttErrorCode::ProbePermissionDenied,
        DebugProbeOpenError::UnsupportedWireProtocol { .. } => {
            RttErrorCode::UnsupportedWireProtocol
        }
        DebugProbeOpenError::InvalidSelector(_) => RttErrorCode::InvalidConfig,
        DebugProbeOpenError::ConfigureSpeed { .. }
        | DebugProbeOpenError::TargetAttach { .. } => RttErrorCode::TargetAttachFailed,
    };
    RttError::new(code, error.to_string())
}
fn map_rtt_attach_error(error: ProbeRttError) -> RttError {
    match error {
        ProbeRttError::ControlBlockNotFound | ProbeRttError::NoControlBlockLocation => {
            RttError::new(
                RttErrorCode::RttControlBlockNotFound,
                "已连接调试探针，但尚未检测到 RTT Control Block",
            )
        }
        ProbeRttError::MultipleControlBlocksFound(addresses) => RttError::new(
            RttErrorCode::RttMultipleControlBlocks,
            format!("检测到多个 RTT Control Block: {addresses:#x?}"),
        ),
        ProbeRttError::ControlBlockCorrupted(message) => RttError::new(
            RttErrorCode::RttInvalidControlBlock,
            format!("RTT Control Block 无效: {message}"),
        ),
        other => RttError::new(
            RttErrorCode::RttNotInitialized,
            format!("初始化 RTT 失败: {other}"),
        ),
    }
}

fn map_rtt_io_error(error: ProbeRttError) -> RttError {
    match error {
        ProbeRttError::MissingChannel(index) => RttError::new(
            RttErrorCode::RttChannelNotFound,
            format!("RTT Channel {index} 不存在"),
        ),
        other => RttError::new(
            RttErrorCode::ProbeDisconnected,
            format!("RTT 与目标通信失败: {other}"),
        ),
    }
}
