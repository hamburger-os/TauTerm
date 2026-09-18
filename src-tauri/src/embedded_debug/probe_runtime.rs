use probe_rs::probe::{list::Lister, DebugProbeSelector, WireProtocol};
use probe_rs::{Permissions, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugWireProtocol {
    Swd,
    Jtag,
}

#[derive(Debug, Clone)]
pub struct DebugProbeConfig {
    pub selector: Option<String>,
    pub target: String,
    pub wire_protocol: DebugWireProtocol,
    pub speed_khz: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DebugProbeInfo {
    pub selector: String,
    pub display_name: String,
    pub identifier: String,
    pub serial_number: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DebugProbeOpenError {
    #[error("无效调试探针 selector: {0}")]
    InvalidSelector(String),
    #[error("未发现可用调试探针")]
    NotFound,
    #[error("发现多个调试探针，请明确选择一个")]
    Ambiguous,
    #[error("调试探针权限不足: {0}")]
    PermissionDenied(String),
    #[error("调试探针正在被占用: {0}")]
    Busy(String),
    #[error("打开调试探针失败: {0}")]
    Open(String),
    #[error("调试探针不支持所选接口 {protocol}: {detail}")]
    UnsupportedWireProtocol { protocol: String, detail: String },
    #[error("设置调试接口速度 {speed_khz} kHz 失败: {detail}")]
    ConfigureSpeed { speed_khz: u32, detail: String },
    #[error("连接目标芯片 {target} 失败: {detail}")]
    TargetAttach { target: String, detail: String },
}

/// Probe + target session owned exclusively by the shared embedded-debug target worker.
///
/// The value is deliberately not exposed through shared mutable ownership. `DebugTargetRuntime`
/// keeps it on one thread and observation services submit short operations through the bounded
/// scheduler, so RTT and future memory/trace producers cannot concurrently borrow the probe.
pub struct DebugProbeRuntime {
    session: Session,
    target: String,
    probe_label: String,
}

impl DebugProbeRuntime {
    pub fn open(config: &DebugProbeConfig) -> Result<Self, DebugProbeOpenError> {
        let lister = Lister::new();
        let (mut probe, probe_label) = if let Some(raw_selector) = &config.selector {
            let selector = raw_selector
                .parse::<DebugProbeSelector>()
                .map_err(|error| DebugProbeOpenError::InvalidSelector(error.to_string()))?;
            let probe = lister.open(selector).map_err(map_open_error)?;
            (probe, raw_selector.clone())
        } else {
            let probes = lister.list_all();
            match probes.as_slice() {
                [] => return Err(DebugProbeOpenError::NotFound),
                [info] => {
                    let label = info.to_string();
                    let probe = info.open().map_err(map_open_error)?;
                    (probe, label)
                }
                _ => return Err(DebugProbeOpenError::Ambiguous),
            }
        };

        let wire_protocol = match config.wire_protocol {
            DebugWireProtocol::Swd => WireProtocol::Swd,
            DebugWireProtocol::Jtag => WireProtocol::Jtag,
        };
        probe.select_protocol(wire_protocol).map_err(|error| {
            DebugProbeOpenError::UnsupportedWireProtocol {
                protocol: format!("{wire_protocol:?}"),
                detail: error.to_string(),
            }
        })?;

        if let Some(speed_khz) = config.speed_khz {
            probe
                .set_speed(speed_khz)
                .map_err(|error| DebugProbeOpenError::ConfigureSpeed {
                    speed_khz,
                    detail: error.to_string(),
                })?;
        }

        let session = probe
            .attach(config.target.clone(), Permissions::default())
            .map_err(|error| DebugProbeOpenError::TargetAttach {
                target: config.target.clone(),
                detail: error.to_string(),
            })?;

        Ok(Self {
            session,
            target: config.target.clone(),
            probe_label,
        })
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn probe_label(&self) -> &str {
        &self.probe_label
    }
}

pub fn list_probes() -> Vec<DebugProbeInfo> {
    Lister::new()
        .list_all()
        .into_iter()
        .map(|info| DebugProbeInfo {
            selector: DebugProbeSelector::from(&info).to_string(),
            display_name: info.to_string(),
            identifier: info.identifier,
            serial_number: info.serial_number,
        })
        .collect()
}

fn map_open_error(error: probe_rs::probe::DebugProbeError) -> DebugProbeOpenError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("permission") || lower.contains("access") {
        DebugProbeOpenError::PermissionDenied(message)
    } else if lower.contains("busy") || lower.contains("in use") {
        DebugProbeOpenError::Busy(message)
    } else {
        DebugProbeOpenError::Open(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_probe_config_keeps_wire_protocol_vendor_neutral() {
        let config = DebugProbeConfig {
            selector: None,
            target: "example".to_string(),
            wire_protocol: DebugWireProtocol::Swd,
            speed_khz: None,
        };
        assert_eq!(config.wire_protocol, DebugWireProtocol::Swd);
    }
}
