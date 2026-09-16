use super::error::RttError;
use serde_json::Value;
use std::ops::Range;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RttBackendKind {
    ProbeRs,
    JlinkExisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RttWireProtocol {
    Swd,
    Jtag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RttLocator {
    AutoRam,
    Exact(u64),
    Ranges(Vec<Range<u64>>),
}

#[derive(Debug, Clone)]
pub struct RttConfig {
    pub backend: RttBackendKind,
    pub probe_selector: Option<String>,
    pub target: Option<String>,
    pub wire_protocol: RttWireProtocol,
    pub speed_khz: Option<u32>,
    pub core_index: usize,
    pub locator: RttLocator,
    pub attach_timeout: Duration,
    pub poll_interval: Duration,
    pub write_timeout: Duration,
    pub jlink_port: u16,
    pub jlink_channels: Vec<u32>,
}

impl RttConfig {
    pub fn from_params(params: &Value) -> Result<Self, RttError> {
        let object = params
            .as_object()
            .ok_or_else(|| RttError::invalid_config("RTT 会话参数必须是 JSON object"))?;
        let backend = match object
            .get("backend")
            .and_then(Value::as_str)
            .unwrap_or("probe_rs")
        {
            "probe_rs" => RttBackendKind::ProbeRs,
            "jlink_existing" => RttBackendKind::JlinkExisting,
            other => return Err(RttError::invalid_config(format!("未知 RTT backend: {other}"))),
        };
        let probe_selector = nonempty_string(object.get("probe_selector"));
        let target = nonempty_string(object.get("target"));
        if backend == RttBackendKind::ProbeRs && target.is_none() {
            return Err(RttError::invalid_config("ProbeRs 模式必须指定目标芯片"));
        }
        let wire_protocol = match object
            .get("wire_protocol")
            .and_then(Value::as_str)
            .unwrap_or("swd")
        {
            "swd" => RttWireProtocol::Swd,
            "jtag" => RttWireProtocol::Jtag,
            other => {
                return Err(RttError::invalid_config(format!(
                    "不支持的调试接口: {other}"
                )))
            }
        };
        let speed_khz = match object.get("speed_khz").and_then(Value::as_u64) {
            None | Some(0) => None,
            Some(value) if value <= 50_000 => Some(value as u32),
            Some(_) => return Err(RttError::invalid_config("调试接口速度必须在 1..50000 kHz")),
        };
        let core_index = bounded_u64(object.get("core_index"), 0, 0, 31, "CPU Core")? as usize;
        let attach_timeout = Duration::from_millis(bounded_u64(
            object.get("attach_timeout_ms"),
            5_000,
            100,
            30_000,
            "RTT Attach 超时",
        )?);
        let poll_interval = Duration::from_millis(bounded_u64(
            object.get("poll_interval_ms"),
            5,
            2,
            100,
            "RTT 轮询间隔",
        )?);
        let write_timeout = Duration::from_millis(bounded_u64(
            object.get("write_timeout_ms"),
            500,
            50,
            5_000,
            "RTT 写入超时",
        )?);
        let locator = parse_locator(object)?;
        let jlink_port = bounded_u64(
            object.get("jlink_port"),
            19_021,
            1,
            u16::MAX as u64,
            "J-Link RTT 端口",
        )? as u16;
        let jlink_channels = parse_channels(object.get("jlink_channels"))?;

        Ok(Self {
            backend,
            probe_selector,
            target,
            wire_protocol,
            speed_khz,
            core_index,
            locator,
            attach_timeout,
            poll_interval,
            write_timeout,
            jlink_port,
            jlink_channels,
        })
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn bounded_u64(
    value: Option<&Value>,
    default: u64,
    min: u64,
    max: u64,
    label: &str,
) -> Result<u64, RttError> {
    let value = value.and_then(Value::as_u64).unwrap_or(default);
    if !(min..=max).contains(&value) {
        return Err(RttError::invalid_config(format!(
            "{label} 必须在 {min}..{max} 范围"
        )));
    }
    Ok(value)
}

fn parse_address(raw: &str) -> Result<u64, RttError> {
    let value = raw.trim().replace('_', "");
    if value.is_empty() {
        return Err(RttError::invalid_config("RTT 地址不能为空"));
    }
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
            .map_err(|_| RttError::invalid_config(format!("无效十六进制地址: {raw}")))
    } else {
        value
            .parse::<u64>()
            .map_err(|_| RttError::invalid_config(format!("无效地址: {raw}")))
    }
}

fn parse_locator(
    object: &serde_json::Map<String, Value>,
) -> Result<RttLocator, RttError> {
    match object
        .get("locator_mode")
        .and_then(Value::as_str)
        .unwrap_or("auto")
    {
        "auto" => Ok(RttLocator::AutoRam),
        "exact" => {
            let raw = object
                .get("control_block_address")
                .and_then(Value::as_str)
                .ok_or_else(|| RttError::invalid_config("指定地址模式需要 RTT Control Block 地址"))?;
            Ok(RttLocator::Exact(parse_address(raw)?))
        }
        "ranges" => {
            let raw = object
                .get("scan_ranges")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut ranges = Vec::new();
            for item in raw.split([',', '\n', ';']).map(str::trim).filter(|s| !s.is_empty()) {
                let (start, end) = item.split_once('-').ok_or_else(|| {
                    RttError::invalid_config(format!("RTT 搜索范围必须使用 start-end 格式: {item}"))
                })?;
                let start = parse_address(start)?;
                let end = parse_address(end)?;
                if start >= end {
                    return Err(RttError::invalid_config(format!(
                        "RTT 搜索范围起点必须小于终点: {item}"
                    )));
                }
                ranges.push(start..end);
            }
            if ranges.is_empty() {
                return Err(RttError::invalid_config("指定范围模式至少需要一个 RTT 搜索范围"));
            }
            if ranges.len() > 16 {
                return Err(RttError::invalid_config("RTT 搜索范围最多 16 个"));
            }
            Ok(RttLocator::Ranges(ranges))
        }
        other => Err(RttError::invalid_config(format!(
            "未知 RTT 定位模式: {other}"
        ))),
    }
}

fn parse_channels(value: Option<&Value>) -> Result<Vec<u32>, RttError> {
    let mut channels = match value {
        None => vec![0],
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| RttError::invalid_config("J-Link RTT Channel 必须是非负整数"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(Value::String(raw)) => raw
            .split([',', ';', ' '])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse::<u32>()
                    .map_err(|_| RttError::invalid_config(format!("无效 J-Link RTT Channel: {value}")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(RttError::invalid_config("J-Link RTT Channel 列表格式无效")),
    };
    channels.sort_unstable();
    channels.dedup();
    if channels.is_empty() {
        channels.push(0);
    }
    if channels.len() > 16 {
        return Err(RttError::invalid_config("J-Link RTT Channel 最多配置 16 个"));
    }
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_direct_probe_config_and_hex_address() {
        let config = RttConfig::from_params(&json!({
            "backend": "probe_rs",
            "target": "STM32F407VG",
            "locator_mode": "exact",
            "control_block_address": "0x2000_0100",
            "wire_protocol": "swd"
        }))
        .unwrap();
        assert_eq!(config.backend, RttBackendKind::ProbeRs);
        assert_eq!(config.locator, RttLocator::Exact(0x2000_0100));
    }

    #[test]
    fn parses_search_ranges() {
        let config = RttConfig::from_params(&json!({
            "target": "nRF52840_xxAA",
            "locator_mode": "ranges",
            "scan_ranges": "0x20000000-0x20010000, 0x20020000-0x20030000"
        }))
        .unwrap();
        assert!(matches!(config.locator, RttLocator::Ranges(ref values) if values.len() == 2));
    }

    #[test]
    fn jlink_existing_does_not_require_target() {
        let config = RttConfig::from_params(&json!({
            "backend": "jlink_existing",
            "jlink_channels": "2,0,2"
        }))
        .unwrap();
        assert_eq!(config.jlink_port, 19_021);
        assert_eq!(config.jlink_channels, vec![0, 2]);
    }
}
