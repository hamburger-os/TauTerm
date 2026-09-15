//! 串口协议插件。
//!
//! 插件只描述 Raw Serial 会话语义；端口发现/打开和实际字节 I/O 由 transport 层负责。

pub const PLUGIN_ID: &str = "serial";

use crate::kernel::plugin_adapter::{
    ContentType, EndpointInfo, ProtocolAdapter, ProtocolConnection,
};
use crate::session::SessionError;
use crate::transport::serial::{open_serial, SerialTransportConfig};
use crate::transport::DataPlaneRuntime;
use crate::virtual_port::backend::is_internal_endpoint_path;

pub struct SerialAdapter;

impl SerialAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Serial 插件只解析 transport 真正消费的链路字段。
    ///
    /// params 还会携带终端显示、文件传输和虚拟串口等 Session/UI 配置；Serde 默认忽略
    /// 未知字段，因此这里不复制第二套 Serial DTO。已声明链路字段一旦类型错误则明确失败，
    /// 禁止静默退回 115200/8N1 后继续打开设备。
    fn parse_transport_params(
        params: &serde_json::Value,
    ) -> Result<SerialTransportConfig, SessionError> {
        serde_json::from_value(params.clone()).map_err(|error| {
            SessionError::Config(format!("invalid serial transport configuration: {error}"))
        })
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
    fn plugin_id(&self) -> Option<&'static str> {
        Some(PLUGIN_ID)
    }

    async fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let config = Self::parse_transport_params(params)?;
        let driver = open_serial(endpoint, &config)?;
        Ok(ProtocolConnection {
            data_plane: Some(DataPlaneRuntime::spawn(Box::new(driver))),
            service: None,
            file_transfer: None,
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
    use crate::transport::serial::{SerialFlowControl, SerialParity, SerialStopBits};

    #[test]
    fn serial_adapter_parses_transport_fields_without_owning_session_ui_fields() {
        let config = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": 921600,
            "data_bits": 7,
            "parity": "even",
            "stop_bits": "2",
            "flow_control": "rts_cts",
            "data_mode": "hex",
            "virtual_port_enabled": true,
            "virtual_port_count": 2
        }))
        .unwrap();
        assert_eq!(config.baud_rate, 921600);
        assert_eq!(config.data_bits, 7);
        assert_eq!(config.parity, SerialParity::Even);
        assert_eq!(config.stop_bits, SerialStopBits::Two);
        assert_eq!(config.flow_control, SerialFlowControl::RtsCts);
    }

    #[test]
    fn missing_transport_field_is_rejected_instead_of_being_repaired() {
        let error = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": 115200,
            "data_bits": 8,
            "parity": "none",
            "stop_bits": "1"
        }))
        .unwrap_err();
        assert!(matches!(error, SessionError::Config(_)));
    }

    #[test]
    fn malformed_transport_field_is_rejected_instead_of_falling_back_to_defaults() {
        let error = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": "921600",
            "data_bits": 8,
            "parity": "none",
            "stop_bits": "1",
            "flow_control": "none"
        }))
        .unwrap_err();
        assert!(matches!(error, SessionError::Config(_)));
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
    fn serial_adapter_contract_exposes_terminal_content_type() {
        let adapter = SerialAdapter::new();
        assert_eq!(adapter.content_type(), ContentType::Terminal);
    }
}
