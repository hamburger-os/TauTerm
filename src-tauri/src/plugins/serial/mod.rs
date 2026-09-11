//! 串口协议插件
//!
//! 实现 `ProtocolAdapter` trait，提供串口终端会话。

use crate::channel::error::SessionError;
use crate::channel::serial_channel::SerialChannel;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    EndpointInfo, ProtocolAdapter, ProtocolConnection, TransferProtocolType,
};
use crate::virtual_port::backend::is_internal_endpoint_path;
use serde::{Deserialize, Serialize};

// ── 串口配置 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialConfig {
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: String,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: String,
    #[serde(default = "default_flow_control")]
    pub flow_control: String,
    #[serde(default = "default_data_mode")]
    pub data_mode: String,
    #[serde(default = "default_virtual_port_enabled")]
    pub virtual_port_enabled: bool,
    #[serde(default = "default_virtual_port_count")]
    pub virtual_port_count: u32,
}

fn default_baud_rate() -> u32 {
    115200
}
fn default_data_bits() -> u8 {
    8
}
fn default_parity() -> String {
    "none".into()
}
fn default_stop_bits() -> String {
    "1".into()
}
fn default_flow_control() -> String {
    "none".into()
}
fn default_data_mode() -> String {
    "text".into()
}
fn default_virtual_port_enabled() -> bool {
    false
}
fn default_virtual_port_count() -> u32 {
    0
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: 115200,
            data_bits: 8,
            parity: "none".into(),
            stop_bits: "1".into(),
            flow_control: "none".into(),
            data_mode: "text".into(),
            virtual_port_enabled: false,
            virtual_port_count: 0,
        }
    }
}

// ── 串口适配器 ──────────────────────────────────────

pub struct SerialAdapter;

impl SerialAdapter {
    pub fn new() -> Self {
        Self
    }

    fn parse_params(params: &serde_json::Value) -> SerialConfig {
        serde_json::from_value(params.clone()).unwrap_or_default()
    }

    fn open_port(
        endpoint: &str,
        config: &SerialConfig,
    ) -> Result<Box<dyn serialport::SerialPort>, SessionError> {
        let data_bits = match config.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let parity = match config.parity.as_str() {
            "even" => serialport::Parity::Even,
            "odd" => serialport::Parity::Odd,
            _ => serialport::Parity::None,
        };
        let stop_bits = match config.stop_bits.as_str() {
            "2" => serialport::StopBits::Two,
            _ => serialport::StopBits::One,
        };
        let flow_control = match config.flow_control.as_str() {
            "rts_cts" => serialport::FlowControl::Hardware,
            "xon_xoff" => serialport::FlowControl::Software,
            _ => serialport::FlowControl::None,
        };

        let mut last_error = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match serialport::new(endpoint, config.baud_rate)
                .data_bits(data_bits)
                .parity(parity)
                .stop_bits(stop_bits)
                .flow_control(flow_control)
                .timeout(std::time::Duration::from_millis(50))
                .open()
            {
                Ok(port) => {
                    let _ = port.clear(serialport::ClearBuffer::All);
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    return Ok(port);
                }
                Err(error) => {
                    last_error = format!("无法打开端口 {endpoint}: {error}");
                }
            }
        }
        Err(SessionError::ConnectionFailed { reason: last_error })
    }
}

/// 驱动友好名经常以 `(COMx)` 重复携带当前系统端口名。
/// 只移除与当前端口完全匹配的末尾标记，保留其它产品文本原样。
fn normalize_device_label(label: &str, port_name: &str) -> String {
    let trimmed = label.trim();
    let suffix = format!(" ({port_name})");
    if trimmed.len() >= suffix.len()
        && trimmed[trimmed.len() - suffix.len()..].eq_ignore_ascii_case(&suffix)
    {
        trimmed[..trimmed.len() - suffix.len()]
            .trim_end()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for SerialAdapter {
    async fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let config = Self::parse_params(params);
        let port = Self::open_port(endpoint, &config)?;
        let channel = SerialChannel::new(port);
        Ok(ProtocolConnection {
            channel: Some(crate::kernel::plugin_adapter::ChannelKind::Sync(Box::new(
                channel,
            ))),
            comm_handle: None,
            side_channel: None,
            channel_factory: None,
            teardown_delay: self.teardown_delay(),
        })
    }

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        let ports =
            serialport::available_ports().map_err(|error| SessionError::ConnectionFailed {
                reason: error.to_string(),
            })?;

        Ok(ports
            .into_iter()
            // Windows 虚拟串口的 bridge 端只供 TauTerm 内部桥接线程打开，不能作为
            // 用户可选串口再次暴露；external 端则仍正常出现在列表中。
            .filter(|port| !is_internal_endpoint_path(&port.port_name))
            .map(|port| {
                let port_name = port.port_name.clone();
                let (description, identity) = match &port.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        let raw_label = info
                            .product
                            .as_deref()
                            .or(info.manufacturer.as_deref())
                            .unwrap_or("USB Serial");
                        let label = normalize_device_label(raw_label, &port_name);
                        let description = format!("{label} [{:04X}:{:04X}]", info.vid, info.pid);
                        let stable_id = info.serial_number.as_ref().map(|serial| {
                            format!("usb:{:04x}:{:04x}:{serial}", info.vid, info.pid)
                        });
                        (
                            description,
                            serde_json::json!({
                                "kind": "usb",
                                "system_port": port_name.clone(),
                                "vid": info.vid,
                                "pid": info.pid,
                                "serial_number": info.serial_number.clone(),
                                "manufacturer": info.manufacturer.clone(),
                                "product": info.product.clone(),
                                "stable_id": stable_id,
                            }),
                        )
                    }
                    serialport::SerialPortType::BluetoothPort => (
                        "Bluetooth Serial".to_string(),
                        serde_json::json!({
                            "kind": "bluetooth",
                            "system_port": port_name.clone(),
                        }),
                    ),
                    serialport::SerialPortType::PciPort => (
                        "PCI Serial".to_string(),
                        serde_json::json!({
                            "kind": "pci",
                            "system_port": port_name.clone(),
                        }),
                    ),
                    serialport::SerialPortType::Unknown => (
                        port_name.clone(),
                        serde_json::json!({
                            "kind": "unknown",
                            "system_port": port_name.clone(),
                        }),
                    ),
                };

                EndpointInfo {
                    name: port_name,
                    description,
                    params: Some(serde_json::json!({
                        "device_identity": identity,
                    })),
                }
            })
            .collect())
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![
            TransferProtocolType::ymodem(),
            TransferProtocolType::xmodem(),
            TransferProtocolType::zmodem(),
        ]
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
    }

    fn teardown_delay(&self) -> std::time::Duration {
        #[cfg(target_os = "windows")]
        {
            std::time::Duration::from_millis(100)
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::time::Duration::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_config_defaults_are_stable() {
        let config = SerialAdapter::parse_params(&serde_json::json!({}));
        assert_eq!(config.baud_rate, 115200);
        assert_eq!(config.data_bits, 8);
        assert_eq!(config.parity, "none");
        assert_eq!(config.stop_bits, "1");
        assert_eq!(config.flow_control, "none");
        assert_eq!(config.data_mode, "text");
        assert!(!config.virtual_port_enabled);
        assert_eq!(config.virtual_port_count, 0);
    }

    #[test]
    fn serial_config_parses_explicit_transport_settings() {
        let config = SerialAdapter::parse_params(&serde_json::json!({
            "baud_rate": 921600,
            "data_bits": 7,
            "parity": "even",
            "stop_bits": "2",
            "flow_control": "rts_cts",
            "data_mode": "hex",
            "virtual_port_enabled": true,
            "virtual_port_count": 2
        }));
        assert_eq!(config.baud_rate, 921600);
        assert_eq!(config.data_bits, 7);
        assert_eq!(config.parity, "even");
        assert_eq!(config.stop_bits, "2");
        assert_eq!(config.flow_control, "rts_cts");
        assert_eq!(config.data_mode, "hex");
        assert!(config.virtual_port_enabled);
        assert_eq!(config.virtual_port_count, 2);
    }

    #[test]
    fn device_label_drops_only_matching_port_suffix() {
        assert_eq!(
            normalize_device_label("STLink Virtual COM Port (COM5)", "COM5"),
            "STLink Virtual COM Port"
        );
        assert_eq!(
            normalize_device_label("Adapter (COM6)", "COM5"),
            "Adapter (COM6)"
        );
    }

    #[test]
    fn serial_adapter_contract_exposes_expected_shared_capabilities() {
        let adapter = SerialAdapter::new();
        assert_eq!(adapter.io_strategy(), IoStrategy::Sync);
        let protocols = adapter
            .transfer_protocols()
            .into_iter()
            .map(|protocol| protocol.to_string())
            .collect::<Vec<_>>();
        assert_eq!(protocols, vec!["ymodem", "xmodem", "zmodem"]);
        assert_eq!(adapter.content_type(), ContentType::Terminal);
    }
}
