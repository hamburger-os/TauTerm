use super::RttBackend;
use crate::plugins::rtt::config::{RttConfig, RttLocator, RttWireProtocol};
use crate::plugins::rtt::error::{RttError, RttErrorCode};
use crate::plugins::rtt::model::{
    RttBackendCapabilities, RttBackendDescriptor, RttChannelDirectionInfo, RttChannelInfo,
    RttProbeInfo, RttReadChunk,
};
use probe_rs::probe::{list::Lister, DebugProbeSelector, WireProtocol};
use probe_rs::rtt::{try_attach_to_rtt, Error as ProbeRttError, Rtt, ScanRegion};
use probe_rs::{Permissions, Session};
use std::collections::BTreeMap;

pub struct ProbeRsRttBackend {
    session: Session,
    rtt: Rtt,
    core_index: usize,
    target: String,
    probe_label: String,
    channels: Vec<RttChannelInfo>,
}

pub fn list_probes() -> Vec<RttProbeInfo> {
    Lister::new()
        .list_all()
        .into_iter()
        .map(|info| {
            let selector = DebugProbeSelector::from(&info).to_string();
            let display_name = info.to_string();
            RttProbeInfo {
                selector,
                display_name,
                identifier: info.identifier,
                serial_number: info.serial_number,
            }
        })
        .collect()
}

impl ProbeRsRttBackend {
    pub fn open(config: &RttConfig) -> Result<Self, RttError> {
        let target = config
            .target
            .clone()
            .ok_or_else(|| RttError::invalid_config("ProbeRs 模式缺少目标芯片"))?;
        let lister = Lister::new();
        let (mut probe, probe_label) = if let Some(raw_selector) = &config.probe_selector {
            let selector = raw_selector
                .parse::<DebugProbeSelector>()
                .map_err(|error| {
                    RttError::invalid_config(format!("无效调试探针 selector: {error}"))
                })?;
            let probe = lister.open(selector).map_err(map_open_error)?;
            (probe, raw_selector.clone())
        } else {
            let probes = lister.list_all();
            match probes.as_slice() {
                [] => {
                    return Err(RttError::new(
                        RttErrorCode::ProbeNotFound,
                        "未发现可用调试探针",
                    ));
                }
                [info] => {
                    let label = info.to_string();
                    let probe = info.open().map_err(map_open_error)?;
                    (probe, label)
                }
                _ => {
                    return Err(RttError::new(
                        RttErrorCode::ProbeAmbiguous,
                        "发现多个调试探针，请在会话配置中明确选择一个",
                    ));
                }
            }
        };

        let wire_protocol = match config.wire_protocol {
            RttWireProtocol::Swd => WireProtocol::Swd,
            RttWireProtocol::Jtag => WireProtocol::Jtag,
        };
        probe.select_protocol(wire_protocol).map_err(|error| {
            RttError::new(
                RttErrorCode::UnsupportedWireProtocol,
                format!("调试探针不支持所选接口 {wire_protocol:?}: {error}"),
            )
        })?;
        if let Some(speed) = config.speed_khz {
            probe.set_speed(speed).map_err(|error| {
                RttError::new(
                    RttErrorCode::TargetAttachFailed,
                    format!("设置调试接口速度 {speed} kHz 失败: {error}"),
                )
            })?;
        }

        let mut session = probe
            .attach(target.clone(), Permissions::default())
            .map_err(|error| {
                RttError::new(
                    RttErrorCode::TargetAttachFailed,
                    format!("连接目标芯片 {target} 失败: {error}"),
                )
            })?;
        let region = match &config.locator {
            RttLocator::AutoRam => ScanRegion::Ram,
            RttLocator::Exact(address) => ScanRegion::Exact(*address),
            RttLocator::Ranges(ranges) => ScanRegion::Ranges(ranges.clone()),
        };
        let mut core = session.core(config.core_index).map_err(|error| {
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
            session,
            rtt,
            core_index: config.core_index,
            target,
            probe_label,
            channels,
        })
    }
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
            target: Some(self.target.clone()),
            probe: Some(self.probe_label.clone()),
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
        let mut core = self.session.core(self.core_index).map_err(|error| {
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
        let mut core = self.session.core(self.core_index).map_err(|error| {
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
        self.channels = collect_channels(&mut self.rtt)?;
        Ok(self.channels.clone())
    }
}

fn map_open_error(error: probe_rs::probe::DebugProbeError) -> RttError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let code = if lower.contains("permission") || lower.contains("access") {
        RttErrorCode::ProbePermissionDenied
    } else if lower.contains("busy") || lower.contains("in use") {
        RttErrorCode::ProbeBusy
    } else {
        RttErrorCode::ProbeNotFound
    };
    RttError::new(code, format!("打开调试探针失败: {message}"))
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
