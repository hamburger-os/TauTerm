use super::RttBackend;
use crate::embedded_debug::firmware_artifact::{FirmwareArtifact, FirmwareArtifactError};
use crate::embedded_debug::probe_runtime::{
    list_probes as list_debug_probes, DebugProbeConfig, DebugProbeOpenError, DebugWireProtocol,
};
use crate::embedded_debug::runtime::{
    DebugServiceLease, DebugTargetRuntimeError, EmbeddedDebugManager,
};
use crate::plugins::rtt::config::{RttConfig, RttLocator, RttWireProtocol};
use crate::plugins::rtt::error::{RttError, RttErrorCode};
use crate::plugins::rtt::model::{
    RttBackendCapabilities, RttBackendDescriptor, RttChannelDirectionInfo, RttChannelInfo,
    RttProbeInfo, RttReadChunk,
};
use probe_rs::rtt::{
    find_rtt_control_block_in_raw_file, try_attach_to_rtt, Error as ProbeRttError, Rtt, ScanRegion,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CHANNEL_REFRESH_TIMEOUT_CAP: Duration = Duration::from_secs(2);
const TARGET_OPERATION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_UP_CHANNELS_PER_POLL: usize = 4;
const MAX_READS_PER_UP_CHANNEL: usize = 4;

pub struct ProbeRsRttBackend {
    service: DebugServiceLease,
    rtt: Arc<Mutex<Rtt>>,
    control_block_address: u64,
    core_index: usize,
    region: ScanRegion,
    refresh_timeout: Duration,
    channels: Vec<RttChannelInfo>,
    poll_cursor: usize,
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
    pub(super) fn open(
        config: &RttConfig,
        embedded_debug: &EmbeddedDebugManager,
        session_id: &str,
    ) -> Result<Self, RttError> {
        let target_name = config
            .target
            .clone()
            .ok_or_else(|| RttError::invalid_config("ProbeRs 模式缺少目标芯片"))?;
        let wire_protocol = match config.wire_protocol {
            RttWireProtocol::Swd => DebugWireProtocol::Swd,
            RttWireProtocol::Jtag => DebugWireProtocol::Jtag,
        };
        let target = embedded_debug
            .acquire_target(&DebugProbeConfig {
                selector: config.probe_selector.clone(),
                target: target_name,
                wire_protocol,
                speed_khz: config.speed_khz,
            })
            .map_err(map_target_runtime_error)?;
        let service = target
            .acquire_service("rtt")
            .map_err(map_target_runtime_error)?;
        drop(target);

        let descriptor = service.descriptor();
        log::info!(
            "RTT native target ready: session={}, probe={}, target={}, core={}, wire={:?}, speed_khz={}",
            session_id,
            descriptor.probe_label,
            descriptor.target,
            config.core_index,
            config.wire_protocol,
            config
                .speed_khz
                .map(|value| value.to_string())
                .unwrap_or_else(|| "auto".to_string())
        );

        let region = resolve_scan_region(config, session_id)?;
        let core_index = config.core_index;
        let attach_timeout = config.attach_timeout;
        let attach_region = region.clone();
        let (rtt, channels) = service
            .execute(
                attach_timeout.saturating_add(Duration::from_secs(1)),
                move |probe| {
                    let mut core = probe.session_mut().core(core_index).map_err(|error| {
                        RttError::new(
                            RttErrorCode::CoreNotFound,
                            format!("无法打开 CPU Core {core_index}: {error}"),
                        )
                    })?;
                    let mut rtt = try_attach_to_rtt(&mut core, attach_timeout, &attach_region)
                        .map_err(map_rtt_attach_error)?;
                    drop(core);
                    let channels = collect_channels(&mut rtt)?;
                    Ok((rtt, channels))
                },
            )
            .map_err(map_target_runtime_error)??;
        let control_block_address = rtt.ptr();
        log::info!(
            "RTT attach succeeded: session={}, control_block=0x{:X}, channels={}, metadata={}",
            session_id,
            control_block_address,
            channels.len(),
            channel_summary(&channels)
        );

        Ok(Self {
            service,
            rtt: Arc::new(Mutex::new(rtt)),
            control_block_address,
            core_index,
            region,
            refresh_timeout: config.attach_timeout.min(CHANNEL_REFRESH_TIMEOUT_CAP),
            channels,
            poll_cursor: 0,
        })
    }
}

fn resolve_scan_region(config: &RttConfig, session_id: &str) -> Result<ScanRegion, RttError> {
    match &config.locator {
        RttLocator::Auto => {
            let Some(path) = config.firmware_path.as_deref() else {
                log::info!(
                    "RTT locator resolved: session={}, source=ram_scan, reason=no_firmware_artifact",
                    session_id
                );
                return Ok(ScanRegion::Ram);
            };
            let artifact = FirmwareArtifact::load(path).map_err(map_firmware_artifact_error)?;
            match find_rtt_control_block_in_raw_file(artifact.bytes()).map_err(|error| {
                RttError::new(
                    RttErrorCode::FirmwareArtifactInvalid,
                    format!("无法解析固件符号文件 {path}: {error}"),
                )
            })? {
                Some(address) => {
                    log::info!(
                        "RTT locator resolved: session={}, source=elf_symbol, symbol=_SEGGER_RTT, address=0x{:X}, firmware={}",
                        session_id,
                        address,
                        path
                    );
                    Ok(ScanRegion::Exact(address))
                }
                None => {
                    log::info!(
                        "RTT locator resolved: session={}, source=ram_scan, reason=elf_symbol_missing, firmware={}",
                        session_id,
                        path
                    );
                    Ok(ScanRegion::Ram)
                }
            }
        }
        RttLocator::Exact(address) => {
            log::info!(
                "RTT locator resolved: session={}, source=user_exact, address=0x{:X}",
                session_id,
                address
            );
            Ok(ScanRegion::Exact(*address))
        }
        RttLocator::Ranges(ranges) => {
            const MAX_LOGGED_RANGES: usize = 16;
            let mut entries = ranges
                .iter()
                .take(MAX_LOGGED_RANGES)
                .map(|range| format!("0x{:X}-0x{:X}", range.start, range.end))
                .collect::<Vec<_>>();
            if ranges.len() > MAX_LOGGED_RANGES {
                entries.push(format!(
                    "...(+{})",
                    ranges.len().saturating_sub(MAX_LOGGED_RANGES)
                ));
            }
            log::info!(
                "RTT locator resolved: session={}, source=user_ranges, ranges={}",
                session_id,
                entries.join(",")
            );
            Ok(ScanRegion::Ranges(ranges.clone()))
        }
    }
}

fn channel_summary(channels: &[RttChannelInfo]) -> String {
    const MAX_LOGGED_CHANNELS: usize = 16;
    let mut entries = channels
        .iter()
        .take(MAX_LOGGED_CHANNELS)
        .map(|channel| {
            let up = channel
                .up
                .as_ref()
                .and_then(|direction| direction.buffer_size)
                .map(|size| size.to_string())
                .unwrap_or_else(|| "-".to_string());
            let down = channel
                .down
                .as_ref()
                .and_then(|direction| direction.buffer_size)
                .map(|size| size.to_string())
                .unwrap_or_else(|| "-".to_string());
            format!("{}(up={},down={})", channel.index, up, down)
        })
        .collect::<Vec<_>>();
    if channels.len() > MAX_LOGGED_CHANNELS {
        entries.push(format!(
            "...(+{})",
            channels.len().saturating_sub(MAX_LOGGED_CHANNELS)
        ));
    }
    entries.join(",")
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
            target: Some(self.service.descriptor().target.clone()),
            probe: Some(self.service.descriptor().probe_label.clone()),
            control_block_address: Some(format!("0x{:X}", self.control_block_address)),
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
        let rtt = Arc::clone(&self.rtt);
        let core_index = self.core_index;
        let poll_cursor = self.poll_cursor;
        let (chunks, next_poll_cursor) = self
            .service
            .execute(TARGET_OPERATION_TIMEOUT, move |probe| {
                let mut core = probe.session_mut().core(core_index).map_err(|error| {
                    RttError::new(
                        RttErrorCode::ProbeDisconnected,
                        format!("RTT 轮询时无法访问 CPU Core: {error}"),
                    )
                })?;
                let mut rtt = rtt
                    .lock()
                    .map_err(|error| RttError::backend(error.to_string()))?;
                let channel_count = rtt.up_channels().iter().count();
                if channel_count == 0 {
                    return Ok((Vec::new(), 0));
                }

                let start = poll_cursor % channel_count;
                let channel_budget = channel_count.min(MAX_UP_CHANNELS_PER_POLL);
                let mut chunks = Vec::new();
                let mut buffer = [0u8; 4 * 1024];
                for (position, channel) in rtt.up_channels().iter_mut().enumerate() {
                    let distance = (position + channel_count - start) % channel_count;
                    if distance >= channel_budget {
                        continue;
                    }
                    for _ in 0..MAX_READS_PER_UP_CHANNEL {
                        let count = channel
                            .read(&mut core, &mut buffer)
                            .map_err(map_rtt_io_error)?;
                        if count == 0 {
                            break;
                        }
                        chunks.push(RttReadChunk {
                            channel_index: channel.number() as u32,
                            data: buffer[..count].to_vec(),
                        });
                        if count < buffer.len() {
                            break;
                        }
                    }
                }
                Ok((chunks, (start + channel_budget) % channel_count))
            })
            .map_err(map_target_runtime_error)??;
        self.poll_cursor = next_poll_cursor;
        output.extend(chunks);
        Ok(())
    }

    fn write(&mut self, channel_index: u32, data: &[u8]) -> Result<usize, RttError> {
        let rtt = Arc::clone(&self.rtt);
        let core_index = self.core_index;
        let data = data.to_vec();
        self.service
            .execute(TARGET_OPERATION_TIMEOUT, move |probe| {
                let mut core = probe.session_mut().core(core_index).map_err(|error| {
                    RttError::new(
                        RttErrorCode::ProbeDisconnected,
                        format!("RTT 写入时无法访问 CPU Core: {error}"),
                    )
                })?;
                let mut rtt = rtt
                    .lock()
                    .map_err(|error| RttError::backend(error.to_string()))?;
                let channel = rtt.down_channel(channel_index as usize).ok_or_else(|| {
                    RttError::new(
                        RttErrorCode::RttChannelNotFound,
                        format!("RTT Down Channel {channel_index} 不存在"),
                    )
                })?;
                channel.write(&mut core, &data).map_err(map_rtt_io_error)
            })
            .map_err(map_target_runtime_error)?
    }

    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
        let rtt = Arc::clone(&self.rtt);
        let core_index = self.core_index;
        let refresh_timeout = self.refresh_timeout;
        let region = self.region.clone();
        let (channels, control_block_address) = self
            .service
            .execute(
                refresh_timeout.saturating_add(Duration::from_secs(1)),
                move |probe| {
                    let mut core = probe.session_mut().core(core_index).map_err(|error| {
                        RttError::new(
                            RttErrorCode::ProbeDisconnected,
                            format!("刷新 RTT Channel 时无法访问 CPU Core: {error}"),
                        )
                    })?;
                    let mut refreshed = try_attach_to_rtt(&mut core, refresh_timeout, &region)
                        .map_err(map_rtt_attach_error)?;
                    drop(core);
                    let channels = collect_channels(&mut refreshed)?;
                    let control_block_address = refreshed.ptr();
                    *rtt.lock()
                        .map_err(|error| RttError::backend(error.to_string()))? = refreshed;
                    Ok((channels, control_block_address))
                },
            )
            .map_err(map_target_runtime_error)??;
        self.control_block_address = control_block_address;
        self.channels = channels;
        Ok(self.channels.clone())
    }
}

fn map_target_runtime_error(error: DebugTargetRuntimeError) -> RttError {
    match error {
        DebugTargetRuntimeError::Open(error) => map_probe_open_error(error),
        DebugTargetRuntimeError::ServiceBusy { .. }
        | DebugTargetRuntimeError::TargetOpening { .. }
        | DebugTargetRuntimeError::TargetClosing { .. }
        | DebugTargetRuntimeError::TargetConfigConflict { .. } => {
            RttError::new(RttErrorCode::ProbeBusy, error.to_string())
        }
        DebugTargetRuntimeError::StartupTimeout => {
            RttError::new(RttErrorCode::TargetAttachFailed, error.to_string())
        }
        DebugTargetRuntimeError::Timeout => {
            RttError::new(RttErrorCode::OperationTimeout, error.to_string())
        }
        DebugTargetRuntimeError::InFlightTimeout => {
            RttError::new(RttErrorCode::OperationOutcomeUnknown, error.to_string())
        }
        DebugTargetRuntimeError::QueueFull => {
            RttError::new(RttErrorCode::SchedulerBusy, error.to_string())
        }
        DebugTargetRuntimeError::WorkerStopped | DebugTargetRuntimeError::WorkerStart(_) => {
            RttError::new(RttErrorCode::BackendFault, error.to_string())
        }
    }
}

fn map_probe_open_error(error: DebugProbeOpenError) -> RttError {
    let code = match error {
        DebugProbeOpenError::NotFound => RttErrorCode::ProbeNotFound,
        DebugProbeOpenError::Open(_) => RttErrorCode::BackendFault,
        DebugProbeOpenError::Ambiguous => RttErrorCode::ProbeAmbiguous,
        DebugProbeOpenError::Busy(_) => RttErrorCode::ProbeBusy,
        DebugProbeOpenError::PermissionDenied(_) => RttErrorCode::ProbePermissionDenied,
        DebugProbeOpenError::UnsupportedWireProtocol { .. } => {
            RttErrorCode::UnsupportedWireProtocol
        }
        DebugProbeOpenError::InvalidSelector(_) => RttErrorCode::InvalidConfig,
        DebugProbeOpenError::ConfigureSpeed { .. } | DebugProbeOpenError::TargetAttach { .. } => {
            RttErrorCode::TargetAttachFailed
        }
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
            format!("RTT Control Block 无效或尚未完成初始化。技术详情: {message}"),
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_control_block_keeps_stable_error_code_and_detail() {
        let error = map_rtt_attach_error(ProbeRttError::ControlBlockCorrupted(
            "bad descriptor".to_string(),
        ));
        assert_eq!(error.code, RttErrorCode::RttInvalidControlBlock);
        assert!(error.message.contains("无效或尚未完成初始化"));
        assert!(error.message.contains("bad descriptor"));
    }

    #[test]
    fn channel_summary_is_bounded() {
        let channels = (0..20)
            .map(|index| RttChannelInfo {
                index,
                name: None,
                up: Some(RttChannelDirectionInfo {
                    buffer_size: Some(1024),
                }),
                down: None,
                metadata_complete: true,
            })
            .collect::<Vec<_>>();

        let summary = channel_summary(&channels);
        assert!(summary.contains("0(up=1024,down=-)"));
        assert!(summary.contains("15(up=1024,down=-)"));
        assert!(summary.contains("...(+4)"));
        assert!(!summary.contains("16(up=1024,down=-)"));
    }
}
