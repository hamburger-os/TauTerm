//! 串口协议插件。
//!
//! 插件只描述 Raw Serial 会话语义；端口发现/打开和实际字节 I/O 由 transport 层负责。

use crate::kernel::plugin_adapter::{
    ContentType, EndpointInfo, ProtocolAdapter, ProtocolConnection, TransferProtocolType,
};
use crate::session::SessionError;
use crate::transport::serial::{open_serial, SerialTransportConfig};
use crate::transport::DataPlaneRuntime;
use crate::virtual_port::backend::is_internal_endpoint_path;
use serde::{Deserialize, Serialize};

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
    115_200
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
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            parity: default_parity(),
            stop_bits: default_stop_bits(),
            flow_control: default_flow_control(),
            data_mode: default_data_mode(),
            virtual_port_enabled: false,
            virtual_port_count: 0,
        }
    }
}

impl SerialConfig {
    fn transport(&self) -> SerialTransportConfig {
        SerialTransportConfig {
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            parity: self.parity.clone(),
            stop_bits: self.stop_bits.clone(),
            flow_control: self.flow_control.clone(),
            read_timeout_ms: 20,
        }
    }
}

pub struct SerialAdapter;

impl SerialAdapter {
    pub fn new() -> Self {
        Self
    }

    fn parse_params(params: &serde_json::Value) -> SerialConfig {
        serde_json::from_value(params.clone()).unwrap_or_default()
    }
}

/// 驱动友好名经常以 `(COMx)` 重复携带当前系统端口名。
/// 只移除与当前端口完全匹配的末尾标记，保留其它产品文本原样。
fn normalize_device_label(label: &str, port_name: &str) -> String {
    let trimmed = label.trim();
    let suffix = format!(" ({port_name})");
    let start = trimmed.len().saturating_sub(suffix.len());
    let matches = trimmed
        .get(start..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(&suffix));
    if matches {
        trimmed
            .get(..start)
            .unwrap_or(trimmed)
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
        let driver = open_serial(endpoint, &config.transport())?;
        Ok(ProtocolConnection {
            data_plane: Some(DataPlaneRuntime::spawn(Box::new(driver))),
            side_channel: None,
            channel_factory: None,
            on_attached: None,
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
                        let normalized = normalize_device_label(raw_label, &port_name);
                        let label = if normalized.is_empty() {
                            "USB Serial".to_string()
                        } else {
                            normalized
                        };
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
                        serde_json::json!({"kind": "bluetooth", "system_port": port_name.clone()}),
                    ),
                    serialport::SerialPortType::PciPort => (
                        "PCI Serial".to_string(),
                        serde_json::json!({"kind": "pci", "system_port": port_name.clone()}),
                    ),
                    serialport::SerialPortType::Unknown => (
                        port_name.clone(),
                        serde_json::json!({"kind": "unknown", "system_port": port_name.clone()}),
                    ),
                };
                EndpointInfo {
                    name: port_name,
                    description,
                    params: Some(serde_json::json!({"device_identity": identity})),
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
        assert_eq!(config.baud_rate, 115_200);
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
        assert_eq!(normalize_device_label("设备适配器", "COM5"), "设备适配器");
    }

    #[test]
    fn serial_adapter_contract_exposes_expected_shared_capabilities() {
        let adapter = SerialAdapter::new();
        let protocols = adapter
            .transfer_protocols()
            .into_iter()
            .map(|protocol| protocol.to_string())
            .collect::<Vec<_>>();
        assert_eq!(protocols, vec!["ymodem", "xmodem", "zmodem"]);
        assert_eq!(adapter.content_type(), ContentType::Terminal);
    }
}
