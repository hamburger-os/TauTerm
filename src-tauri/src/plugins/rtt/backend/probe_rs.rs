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
use probe_rs::MemoryInterface;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CHANNEL_REFRESH_TIMEOUT_CAP: Duration = Duration::from_secs(2);
const TARGET_OPERATION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_UP_CHANNELS_PER_POLL: usize = 4;
const MAX_READS_PER_UP_CHANNEL: usize = 4;
const MAX_DIAGNOSTIC_CHANNELS: usize = 16;

#[derive(Debug)]
enum ProbeRttHandle {
    Strict(Rtt),
    Degraded(DegradedRtt),
}

impl ProbeRttHandle {
    fn ptr(&self) -> u64 {
        match self {
            Self::Strict(rtt) => rtt.ptr(),
            Self::Degraded(rtt) => rtt.ptr,
        }
    }

    fn channel_metadata(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
        match self {
            Self::Strict(rtt) => collect_channels(rtt),
            Self::Degraded(rtt) => Ok(rtt.channels.clone()),
        }
    }
}

#[derive(Debug)]
struct DirectRttChannel {
    index: u32,
    buffer_address: u64,
    buffer_size: u32,
    write_offset_address: u64,
    read_offset_address: u64,
    last_read_offset: Option<u32>,
}

#[derive(Debug)]
struct DegradedRtt {
    ptr: u64,
    up_channels: Vec<DirectRttChannel>,
    down_channels: Vec<DirectRttChannel>,
    channels: Vec<RttChannelInfo>,
}


pub struct ProbeRsRttBackend {
    service: DebugServiceLease,
    session_id: String,
    rtt: Arc<Mutex<ProbeRttHandle>>,
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
            "RTT native target ready: session={}, probe={}, target={}, core={}, wire={}, speed_khz={}",
            session_id,
            descriptor.probe_label,
            descriptor.target,
            config.core_index,
            config.wire_protocol.as_str(),
            config
                .speed_khz
                .map(|value| value.to_string())
                .unwrap_or_else(|| "auto".to_string())
        );

        let region = resolve_scan_region(config, session_id)?;
        let core_index = config.core_index;
        let attach_timeout = config.attach_timeout;
        let attach_region = region.clone();
        let diagnostic_session_id = session_id.to_string();
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
                    let mut rtt = attach_rtt_with_diagnostics(
                        &mut core,
                        attach_timeout,
                        &attach_region,
                        &diagnostic_session_id,
                    )?;
                    drop(core);
                    let channels = rtt.channel_metadata()?;
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
            session_id: session_id.to_string(),
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

fn attach_rtt_with_diagnostics(
    core: &mut probe_rs::Core<'_>,
    timeout: Duration,
    region: &ScanRegion,
    session_id: &str,
) -> Result<ProbeRttHandle, RttError> {
    match try_attach_to_rtt(core, timeout, region) {
        Ok(rtt) => Ok(ProbeRttHandle::Strict(rtt)),
        Err(error) => {
            match &error {
                ProbeRttError::ControlBlockCorrupted(_) => {
                    log_control_block_snapshot(core, region, session_id);
                    match attach_degraded_rtt(core, region, session_id) {
                        Ok(rtt) => return Ok(ProbeRttHandle::Degraded(rtt)),
                        Err(degraded_error) => {
                            log::warn!(
                                "RTT degraded attach rejected: session={}, code={}, message={}",
                                session_id,
                                degraded_error.code.as_str(),
                                degraded_error.message
                            );
                        }
                    }
                }
                ProbeRttError::ControlBlockNotFound | ProbeRttError::NoControlBlockLocation => {
                    if let ScanRegion::Exact(address) = region {
                        log_exact_address_snapshot(core, *address, session_id);
                    }
                }
                _ => {}
            }
            Err(map_rtt_attach_error(error))
        }
    }
}

fn bytes_as_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|value| format!("{value:02X}"))
        .collect::<Vec<_>>()
        .join("")
}

fn words_as_hex(words: &[u32]) -> String {
    words
        .iter()
        .map(|value| format!("0x{value:08X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn words_as_le_bytes(words: &[u32]) -> Vec<u8> {
    words
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>()
}

fn option_bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unavailable",
    }
}

fn log_exact_address_snapshot(core: &mut probe_rs::Core<'_>, address: u64, session_id: &str) {
    let mut bytes = [0u8; 16];
    let byte_error = core
        .read_8(address, &mut bytes)
        .err()
        .map(|error| error.to_string());

    let mut block_words = [0u32; 4];
    let block_error = core
        .read_32(address, &mut block_words)
        .err()
        .map(|error| error.to_string());

    let mut single_words = Vec::with_capacity(4);
    let mut single_error = None;
    for word_index in 0..4 {
        match core.read_word_32(address + (word_index * 4) as u64) {
            Ok(value) => single_words.push(value),
            Err(error) => {
                single_error = Some(error.to_string());
                break;
            }
        }
    }

    let byte_ok = byte_error.is_none();
    let block_ok = block_error.is_none();
    let single_ok = single_error.is_none() && single_words.len() == 4;

    let block_bytes = block_ok.then(|| words_as_le_bytes(&block_words));
    let single_bytes = single_ok.then(|| words_as_le_bytes(&single_words));

    let magic_match_read8 = byte_ok.then_some(bytes == Rtt::RTT_ID);
    let magic_match_read32 = block_bytes
        .as_deref()
        .map(|value| value == &Rtt::RTT_ID[..]);
    let magic_match_word32 = single_bytes
        .as_deref()
        .map(|value| value == &Rtt::RTT_ID[..]);
    let read8_read32_consistent = match (byte_ok, block_bytes.as_deref()) {
        (true, Some(block)) => Some(block == &bytes[..]),
        _ => None,
    };
    let read8_word32_consistent = match (byte_ok, single_bytes.as_deref()) {
        (true, Some(single)) => Some(single == &bytes[..]),
        _ => None,
    };
    let read32_word32_consistent = match (block_bytes.as_deref(), single_bytes.as_deref()) {
        (Some(block), Some(single)) => Some(block == single),
        _ => None,
    };

    log::warn!(
        "RTT exact address diagnostic: session={}, address=0x{:X}, expected_magic_hex={}, read8_hex={}, read32_words={}, word32_words={}, magic_match_read8={}, magic_match_read32={}, magic_match_word32={}, read8_read32_consistent={}, read8_word32_consistent={}, read32_word32_consistent={}, read8_error={}, read32_error={}, word32_error={}",
        session_id,
        address,
        bytes_as_hex(&Rtt::RTT_ID),
        if byte_ok {
            bytes_as_hex(&bytes)
        } else {
            "-".to_string()
        },
        if block_ok {
            words_as_hex(&block_words)
        } else {
            "-".to_string()
        },
        if single_ok {
            words_as_hex(&single_words)
        } else {
            "-".to_string()
        },
        option_bool_label(magic_match_read8),
        option_bool_label(magic_match_read32),
        option_bool_label(magic_match_word32),
        option_bool_label(read8_read32_consistent),
        option_bool_label(read8_word32_consistent),
        option_bool_label(read32_word32_consistent),
        byte_error.as_deref().unwrap_or("-"),
        block_error.as_deref().unwrap_or("-"),
        single_error.as_deref().unwrap_or("-")
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RttDescriptorSnapshot {
    name_pointer: u64,
    buffer_pointer: u64,
    size: u32,
    write_offset: u32,
    read_offset: u32,
    flags: u32,
}

fn parse_descriptor_words(words: &[u32], is_64_bit: bool) -> Option<RttDescriptorSnapshot> {
    if is_64_bit {
        if words.len() != 8 {
            return None;
        }
        Some(RttDescriptorSnapshot {
            name_pointer: u64::from(words[0]) | (u64::from(words[1]) << 32),
            buffer_pointer: u64::from(words[2]) | (u64::from(words[3]) << 32),
            size: words[4],
            write_offset: words[5],
            read_offset: words[6],
            flags: words[7],
        })
    } else {
        if words.len() != 6 {
            return None;
        }
        Some(RttDescriptorSnapshot {
            name_pointer: u64::from(words[0]),
            buffer_pointer: u64::from(words[1]),
            size: words[2],
            write_offset: words[3],
            read_offset: words[4],
            flags: words[5],
        })
    }
}


fn attach_degraded_rtt(
    core: &mut probe_rs::Core<'_>,
    region: &ScanRegion,
    session_id: &str,
) -> Result<DegradedRtt, RttError> {
    let address = match region {
        ScanRegion::Exact(address) => *address,
        _ => Rtt::find_control_block(core, region).map_err(map_rtt_attach_error)?,
    };
    let rtt = DegradedRtt::attach_at(core, address)?;
    let usable_up = rtt.up_channels.len();
    let usable_down = rtt.down_channels.len();
    let invalid_directions = rtt
        .channels
        .iter()
        .flat_map(|channel| [channel.up.as_ref(), channel.down.as_ref()])
        .flatten()
        .filter(|direction| !direction.usable)
        .count();
    if usable_up + usable_down == 0 {
        return Err(RttError::new(
            RttErrorCode::RttInvalidControlBlock,
            "RTT Control Block 没有任何可安全使用的 Channel",
        ));
    }
    log::warn!(
        "RTT degraded attach accepted: session={}, control_block=0x{:X}, usable_up={}, usable_down={}, invalid_directions={}",
        session_id,
        address,
        usable_up,
        usable_down,
        invalid_directions
    );
    Ok(rtt)
}

impl DegradedRtt {
    fn attach_at(core: &mut probe_rs::Core<'_>, ptr: u64) -> Result<Self, RttError> {
        let mut id = [0u8; 16];
        core.read_8(ptr, &mut id)
            .map_err(|error| rtt_memory_error("读取 RTT Control Block 标识失败", error))?;
        if id != Rtt::RTT_ID {
            return Err(RttError::new(
                RttErrorCode::RttControlBlockNotFound,
                format!("地址 0x{ptr:X} 未找到有效 RTT Control Block"),
            ));
        }

        let mut counts = [0u32; 2];
        core.read_32(ptr + 16, &mut counts)
            .map_err(|error| rtt_memory_error("读取 RTT Channel 数量失败", error))?;
        let max_up = counts[0] as usize;
        let max_down = counts[1] as usize;
        if max_up > 255 || max_down > 255 {
            return Err(RttError::new(
                RttErrorCode::RttInvalidControlBlock,
                format!("RTT Channel 数量异常: up={max_up}, down={max_down}"),
            ));
        }

        let is_64_bit = core.is_64_bit();
        let descriptor_words = if is_64_bit { 8 } else { 6 };
        let descriptor_size = (descriptor_words * std::mem::size_of::<u32>()) as u64;
        let total = max_up.saturating_add(max_down);
        let mut entries: BTreeMap<u32, RttChannelInfo> = BTreeMap::new();
        let mut up_channels = Vec::new();
        let mut down_channels = Vec::new();

        for flat_index in 0..total {
            let metadata_address = ptr + 24 + descriptor_size * flat_index as u64;
            let mut words = vec![0u32; descriptor_words];
            core.read_32(metadata_address, &mut words).map_err(|error| {
                rtt_memory_error(
                    &format!("读取 RTT Channel metadata 0x{metadata_address:X} 失败"),
                    error,
                )
            })?;
            let snapshot = parse_descriptor_words(&words, is_64_bit).ok_or_else(|| {
                RttError::new(
                    RttErrorCode::RttInvalidControlBlock,
                    format!("RTT Channel metadata 0x{metadata_address:X} 长度无效"),
                )
            })?;

            let (is_up, channel_index) = if flat_index < max_up {
                (true, flat_index)
            } else {
                (false, flat_index - max_up)
            };
            if snapshot.buffer_pointer == 0 {
                continue;
            }

            let index = u32::try_from(channel_index)
                .map_err(|_| RttError::backend("RTT Channel index 超出 u32"))?;
            let (name, name_issue) = read_degraded_channel_name(core, snapshot.name_pointer);
            let validation_issue = validate_degraded_descriptor(core, &snapshot);
            let issue = validation_issue.or(name_issue);
            let usable = validation_issue.is_none();
            let direction = RttChannelDirectionInfo {
                buffer_size: usize::try_from(snapshot.size).ok(),
                usable,
                issue: issue.clone(),
            };
            let entry = entries.entry(index).or_insert_with(|| RttChannelInfo {
                index,
                name: name.clone(),
                up: None,
                down: None,
                metadata_complete: issue.is_none(),
            });
            if entry.name.is_none() {
                entry.name = name;
            }
            if issue.is_some() {
                entry.metadata_complete = false;
            }

            if is_up {
                entry.up = Some(direction);
            } else {
                entry.down = Some(direction);
            }

            if !usable {
                log::warn!(
                    "RTT channel direction quarantined: direction={}, channel={}, metadata=0x{:X}, buffer=0x{:X}, size={}, issue={}",
                    if is_up { "up" } else { "down" },
                    channel_index,
                    metadata_address,
                    snapshot.buffer_pointer,
                    snapshot.size,
                    issue.as_deref().unwrap_or("invalid descriptor")
                );
                continue;
            }

            let (write_offset_address, read_offset_address) = if is_64_bit {
                (metadata_address + 20, metadata_address + 24)
            } else {
                (metadata_address + 12, metadata_address + 16)
            };
            let channel = DirectRttChannel {
                index,
                buffer_address: snapshot.buffer_pointer,
                buffer_size: snapshot.size,
                write_offset_address,
                read_offset_address,
                last_read_offset: None,
            };
            if is_up {
                up_channels.push(channel);
            } else {
                down_channels.push(channel);
            }
        }

        Ok(Self {
            ptr,
            up_channels,
            down_channels,
            channels: entries.into_values().collect(),
        })
    }
}

fn rtt_memory_error(context: &str, error: probe_rs::Error) -> RttError {
    RttError::new(
        RttErrorCode::ProbeDisconnected,
        format!("{context}: {error}"),
    )
}

fn merged_ram_ranges(core: &probe_rs::Core<'_>) -> Vec<(u64, u64)> {
    let mut ranges = core
        .memory_regions()
        .filter(|region| region.is_ram())
        .map(|region| {
            let range = region.address_range();
            (range.start, range.end)
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| range.0);
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn validate_degraded_descriptor(
    core: &probe_rs::Core<'_>,
    descriptor: &RttDescriptorSnapshot,
) -> Option<String> {
    if descriptor.size < 2 {
        return Some(format!("buffer size {} 小于 RTT ring buffer 最小值 2", descriptor.size));
    }
    if descriptor.write_offset >= descriptor.size {
        return Some(format!(
            "write offset 0x{:X} 超出 buffer size 0x{:X}",
            descriptor.write_offset, descriptor.size
        ));
    }
    if descriptor.read_offset >= descriptor.size {
        return Some(format!(
            "read offset 0x{:X} 超出 buffer size 0x{:X}",
            descriptor.read_offset, descriptor.size
        ));
    }
    if descriptor.flags & 0x3 == 0x3 {
        return Some(format!("RTT Channel mode flags 无效: 0x{:X}", descriptor.flags));
    }

    let Some(end) = descriptor
        .buffer_pointer
        .checked_add(u64::from(descriptor.size))
    else {
        return Some("RTT buffer 地址溢出".to_string());
    };
    let ranges = merged_ram_ranges(core);
    if !ranges.is_empty()
        && !ranges
            .iter()
            .any(|(start, range_end)| descriptor.buffer_pointer >= *start && end <= *range_end)
    {
        return Some(format!(
            "buffer 0x{:X}..0x{:X} 不在目标 RAM 范围内",
            descriptor.buffer_pointer, end
        ));
    }
    None
}

fn read_degraded_channel_name(
    core: &mut probe_rs::Core<'_>,
    pointer: u64,
) -> (Option<String>, Option<String>) {
    if pointer == 0 {
        return (None, None);
    }

    let memory_range = core
        .memory_regions()
        .filter(|region| region.is_ram() || region.is_nvm())
        .find_map(|region| {
            let range = region.address_range();
            (pointer >= range.start && pointer < range.end).then_some(range)
        });

    let mut bytes = if let Some(range) = memory_range {
        let length = std::cmp::min(128, (range.end - pointer) as usize);
        let mut bytes = vec![0u8; length];
        if let Err(error) = core.read_8(pointer, &mut bytes) {
            return (
                None,
                Some(format!("读取 Channel 名称 0x{pointer:X} 失败: {error}")),
            );
        }
        bytes
    } else if core.target().memory_map.is_empty() {
        let mut bytes = Vec::with_capacity(128);
        for offset in 0..128u64 {
            match core.read_word_8(pointer + offset) {
                Ok(value) => {
                    bytes.push(value);
                    if value == 0 {
                        break;
                    }
                }
                Err(error) => {
                    return (
                        None,
                        Some(format!("读取 Channel 名称 0x{pointer:X} 失败: {error}")),
                    );
                }
            }
        }
        bytes
    } else {
        return (
            None,
            Some(format!("Channel 名称指针 0x{pointer:X} 不在 RAM/NVM 范围内")),
        );
    };

    let Some(end) = bytes.iter().position(|byte| *byte == 0) else {
        return (
            None,
            Some(format!("Channel 名称 0x{pointer:X} 在 128 bytes 内未终止")),
        );
    };
    bytes.truncate(end);
    (Some(String::from_utf8_lossy(&bytes).into_owned()), None)
}

impl DirectRttChannel {
    fn read_offsets(&self, core: &mut probe_rs::Core<'_>) -> Result<(u32, u32), RttError> {
        let mut offsets = [0u32; 2];
        core.read_32(self.write_offset_address, &mut offsets)
            .map_err(|error| rtt_memory_error("读取 RTT ring offsets 失败", error))?;
        if offsets[0] >= self.buffer_size || offsets[1] >= self.buffer_size {
            return Err(RttError::new(
                RttErrorCode::RttInvalidControlBlock,
                format!(
                    "RTT Channel {} 运行期 offset 无效: write={}, read={}, size={}",
                    self.index, offsets[0], offsets[1], self.buffer_size
                ),
            ));
        }
        Ok((offsets[0], offsets[1]))
    }

    fn read_up(
        &mut self,
        core: &mut probe_rs::Core<'_>,
        buffer: &mut [u8],
    ) -> Result<usize, RttError> {
        let (write, mut read) = self.read_offsets(core)?;
        if let Some(last_read) = self.last_read_offset {
            if read != last_read {
                return Err(RttError::new(
                    RttErrorCode::RttInvalidControlBlock,
                    format!(
                        "RTT Up Channel {} read offset 被外部修改: expected={}, actual={}",
                        self.index, last_read, read
                    ),
                ));
            }
        }

        let mut total = 0usize;
        while total < buffer.len() && read != write {
            let available = if read < write {
                write - read
            } else {
                self.buffer_size - read
            } as usize;
            let count = available.min(buffer.len() - total);
            if count == 0 {
                break;
            }
            core.read_8(
                self.buffer_address + u64::from(read),
                &mut buffer[total..total + count],
            )
            .map_err(|error| rtt_memory_error("读取 RTT Up buffer 失败", error))?;
            total += count;
            read += count as u32;
            if read >= self.buffer_size {
                read = 0;
            }
        }

        if total > 0 {
            core.write_word_32(self.read_offset_address, read)
                .map_err(|error| rtt_memory_error("更新 RTT Up read offset 失败", error))?;
            self.last_read_offset = Some(read);
        }
        Ok(total)
    }

    fn write_down(
        &mut self,
        core: &mut probe_rs::Core<'_>,
        data: &[u8],
    ) -> Result<usize, RttError> {
        let (mut write, read) = self.read_offsets(core)?;
        let mut total = 0usize;
        while total < data.len() {
            let available = if read > write {
                read - write - 1
            } else if read == 0 {
                self.buffer_size - write - 1
            } else {
                self.buffer_size - write
            } as usize;
            let count = available.min(data.len() - total);
            if count == 0 {
                break;
            }
            core.write(
                self.buffer_address + u64::from(write),
                &data[total..total + count],
            )
            .map_err(|error| rtt_memory_error("写入 RTT Down buffer 失败", error))?;
            total += count;
            write += count as u32;
            if write >= self.buffer_size {
                write = 0;
            }
        }
        if total > 0 {
            core.write_word_32(self.write_offset_address, write)
                .map_err(|error| rtt_memory_error("更新 RTT Down write offset 失败", error))?;
        }
        Ok(total)
    }
}

fn log_control_block_snapshot(
    core: &mut probe_rs::Core<'_>,
    region: &ScanRegion,
    session_id: &str,
) {
    let address = match region {
        ScanRegion::Exact(address) => *address,
        _ => match Rtt::find_control_block(core, region) {
            Ok(address) => address,
            Err(error) => {
                log::warn!(
                    "RTT diagnostic snapshot unavailable: session={}, reason=control_block_location, error={}",
                    session_id,
                    error
                );
                return;
            }
        },
    };

    let mut id = [0u8; 16];
    if let Err(error) = core.read(address, &mut id) {
        log::warn!(
            "RTT diagnostic snapshot unavailable: session={}, control_block=0x{:X}, reason=header_id_read, error={}",
            session_id,
            address,
            error
        );
        return;
    }
    let mut counts = [0u32; 2];
    if let Err(error) = core.read_32(address + 16, &mut counts) {
        log::warn!(
            "RTT diagnostic snapshot unavailable: session={}, control_block=0x{:X}, reason=channel_count_read, error={}",
            session_id,
            address,
            error
        );
        return;
    }

    let is_64_bit = core.is_64_bit();
    let max_up = counts[0] as usize;
    let max_down = counts[1] as usize;
    log::warn!(
        "RTT diagnostic header: session={}, control_block=0x{:X}, magic_match={}, pointer_width={}, max_up={}, max_down={}",
        session_id,
        address,
        id == Rtt::RTT_ID,
        if is_64_bit { 64 } else { 32 },
        max_up,
        max_down
    );

    let descriptor_words = if is_64_bit { 8 } else { 6 };
    let descriptor_size = (descriptor_words * std::mem::size_of::<u32>()) as u64;
    let total = max_up.saturating_add(max_down).min(MAX_DIAGNOSTIC_CHANNELS);

    for flat_index in 0..total {
        let metadata_address = address + 24 + descriptor_size * flat_index as u64;
        let mut block_words = vec![0u32; descriptor_words];
        if let Err(error) = core.read_32(metadata_address, &mut block_words) {
            log::warn!(
                "RTT diagnostic descriptor unavailable: session={}, metadata=0x{:X}, error={}",
                session_id,
                metadata_address,
                error
            );
            continue;
        }

        let mut single_words = Vec::with_capacity(descriptor_words);
        let mut single_read_error = None;
        for word_index in 0..descriptor_words {
            match core.read_word_32(metadata_address + (word_index * 4) as u64) {
                Ok(value) => single_words.push(value),
                Err(error) => {
                    single_read_error = Some(error.to_string());
                    break;
                }
            }
        }

        let (direction, channel_index) = if flat_index < max_up {
            ("up", flat_index)
        } else {
            ("down", flat_index - max_up)
        };
        let static_word_count = if is_64_bit { 5 } else { 3 };
        let static_fields_consistent = single_read_error.is_none()
            && single_words.get(..static_word_count) == block_words.get(..static_word_count);
        let canonical_words = if single_read_error.is_none() {
            &single_words
        } else {
            &block_words
        };
        let Some(snapshot) = parse_descriptor_words(canonical_words, is_64_bit) else {
            continue;
        };

        log::warn!(
            "RTT diagnostic descriptor: session={}, direction={}, channel={}, metadata=0x{:X}, name=0x{:X}, buffer=0x{:X}, size={}, write={}, read={}, flags=0x{:X}, static_fields_block_single_consistent={}, single_read_error={}",
            session_id,
            direction,
            channel_index,
            metadata_address,
            snapshot.name_pointer,
            snapshot.buffer_pointer,
            snapshot.size,
            snapshot.write_offset,
            snapshot.read_offset,
            snapshot.flags,
            static_fields_consistent,
            single_read_error.as_deref().unwrap_or("-")
        );
    }

    if max_up.saturating_add(max_down) > MAX_DIAGNOSTIC_CHANNELS {
        log::warn!(
            "RTT diagnostic descriptors truncated: session={}, total={}, logged={}",
            session_id,
            max_up.saturating_add(max_down),
            MAX_DIAGNOSTIC_CHANNELS
        );
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
            usable: true,
            issue: None,
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
            usable: true,
            issue: None,
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

                let channel_count = match &*rtt {
                    ProbeRttHandle::Strict(rtt) => rtt.up_channels().iter().count(),
                    ProbeRttHandle::Degraded(rtt) => rtt.up_channels.len(),
                };
                if channel_count == 0 {
                    return Ok((Vec::new(), 0));
                }

                let start = poll_cursor % channel_count;
                let channel_budget = channel_count.min(MAX_UP_CHANNELS_PER_POLL);
                let mut chunks = Vec::new();
                let mut buffer = [0u8; 4 * 1024];

                match &mut *rtt {
                    ProbeRttHandle::Strict(rtt) => {
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
                    }
                    ProbeRttHandle::Degraded(rtt) => {
                        for (position, channel) in rtt.up_channels.iter_mut().enumerate() {
                            let distance = (position + channel_count - start) % channel_count;
                            if distance >= channel_budget {
                                continue;
                            }
                            for _ in 0..MAX_READS_PER_UP_CHANNEL {
                                let count = channel.read_up(&mut core, &mut buffer)?;
                                if count == 0 {
                                    break;
                                }
                                chunks.push(RttReadChunk {
                                    channel_index: channel.index,
                                    data: buffer[..count].to_vec(),
                                });
                                if count < buffer.len() {
                                    break;
                                }
                            }
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
                match &mut *rtt {
                    ProbeRttHandle::Strict(rtt) => {
                        let channel = rtt.down_channel(channel_index as usize).ok_or_else(|| {
                            RttError::new(
                                RttErrorCode::RttChannelNotFound,
                                format!("RTT Down Channel {channel_index} 不存在"),
                            )
                        })?;
                        channel.write(&mut core, &data).map_err(map_rtt_io_error)
                    }
                    ProbeRttHandle::Degraded(rtt) => {
                        let channel = rtt
                            .down_channels
                            .iter_mut()
                            .find(|channel| channel.index == channel_index)
                            .ok_or_else(|| {
                                RttError::new(
                                    RttErrorCode::RttChannelNotFound,
                                    format!("RTT Down Channel {channel_index} 不可用"),
                                )
                            })?;
                        channel.write_down(&mut core, &data)
                    }
                }
            })
            .map_err(map_target_runtime_error)?
    }

    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
        let rtt = Arc::clone(&self.rtt);
        let core_index = self.core_index;
        let refresh_timeout = self.refresh_timeout;
        let region = self.region.clone();
        let diagnostic_session_id = self.session_id.clone();
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
                    let mut refreshed = attach_rtt_with_diagnostics(
                        &mut core,
                        refresh_timeout,
                        &region,
                        &diagnostic_session_id,
                    )?;
                    drop(core);
                    let channels = refreshed.channel_metadata()?;
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
    fn rtt_magic_serialization_is_stable() {
        assert_eq!(
            bytes_as_hex(&Rtt::RTT_ID),
            "53454747455220525454000000000000"
        );
        let words = [
            u32::from_le_bytes(Rtt::RTT_ID[0..4].try_into().unwrap()),
            u32::from_le_bytes(Rtt::RTT_ID[4..8].try_into().unwrap()),
            u32::from_le_bytes(Rtt::RTT_ID[8..12].try_into().unwrap()),
            u32::from_le_bytes(Rtt::RTT_ID[12..16].try_into().unwrap()),
        ];
        assert_eq!(words_as_le_bytes(&words), Rtt::RTT_ID);
        assert_eq!(
            words_as_hex(&words),
            "0x47474553:0x52205245:0x00005454:0x00000000"
        );
    }

    #[test]
    fn optional_boolean_diagnostic_labels_are_explicit() {
        assert_eq!(option_bool_label(Some(true)), "true");
        assert_eq!(option_bool_label(Some(false)), "false");
        assert_eq!(option_bool_label(None), "unavailable");
    }

    #[test]
    fn parses_32_bit_rtt_descriptor_words() {
        let snapshot =
            parse_descriptor_words(&[0x2000_0100, 0x2000_0200, 1024, 17, 9, 2], false).unwrap();
        assert_eq!(
            snapshot,
            RttDescriptorSnapshot {
                name_pointer: 0x2000_0100,
                buffer_pointer: 0x2000_0200,
                size: 1024,
                write_offset: 17,
                read_offset: 9,
                flags: 2,
            }
        );
    }

    #[test]
    fn parses_64_bit_rtt_descriptor_words() {
        let snapshot = parse_descriptor_words(
            &[
                0x5566_7788,
                0x1122_3344,
                0xDDEE_FF00,
                0x99AA_BBCC,
                4096,
                5,
                3,
                1,
            ],
            true,
        )
        .unwrap();
        assert_eq!(snapshot.name_pointer, 0x1122_3344_5566_7788);
        assert_eq!(snapshot.buffer_pointer, 0x99AA_BBCC_DDEE_FF00);
        assert_eq!(snapshot.size, 4096);
        assert_eq!(snapshot.write_offset, 5);
        assert_eq!(snapshot.read_offset, 3);
        assert_eq!(snapshot.flags, 1);
    }

    #[test]
    fn invalid_descriptor_offsets_are_rejected_before_memory_range_checks() {
        let descriptor = RttDescriptorSnapshot {
            name_pointer: 0,
            buffer_pointer: 0x2000_0000,
            size: 64,
            write_offset: 64,
            read_offset: 0,
            flags: 0,
        };
        assert!(validate_descriptor_shape(&descriptor)
            .is_some_and(|issue| issue.contains("write offset")));
    }

    #[test]
    fn unused_descriptor_is_not_a_runtime_direction() {
        let descriptor = RttDescriptorSnapshot {
            name_pointer: 0,
            buffer_pointer: 0,
            size: 0,
            write_offset: 0,
            read_offset: 0,
            flags: 0,
        };
        assert_eq!(descriptor.buffer_pointer, 0);
    }

    #[test]
    fn channel_summary_is_bounded() {
        let channels = (0..20)
            .map(|index| RttChannelInfo {
                index,
                name: None,
                up: Some(RttChannelDirectionInfo {
                    buffer_size: Some(1024),
                    usable: true,
                    issue: None,
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
