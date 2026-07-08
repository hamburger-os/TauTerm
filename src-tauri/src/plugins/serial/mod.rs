//! 串口协议插件
//!
//! 实现 `ProtocolAdapter` trait，提供 RS-232/RS-485 串口终端会话。

use serde::{Deserialize, Serialize};
use crate::channel::{Channel, ContentType, IoStrategy};
use crate::channel::error::SessionError;
use crate::channel::serial_channel::SerialChannel;
use crate::kernel::plugin_adapter::{EndpointInfo, PluginManifest, ProtocolAdapter, TransferProtocolType};

// ── 串口配置 ────────────────────────────────────────

/// 串口连接参数
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
    /// 虚拟串口是否启用（仅用于 serde 反序列化默认值，实际逻辑在 commands::connect_session_serial 中）
    #[serde(default = "default_virtual_port_enabled")]
    pub virtual_port_enabled: bool,
    /// 虚拟串口设备数量（仅用于 serde 反序列化默认值，实际逻辑在 commands::connect_session_serial 中）
    #[serde(default = "default_virtual_port_count")]
    pub virtual_port_count: u32,
}

fn default_baud_rate() -> u32 { 115200 }
fn default_data_bits() -> u8 { 8 }
fn default_parity() -> String { "none".into() }
fn default_stop_bits() -> String { "1".into() }
fn default_flow_control() -> String { "none".into() }
fn default_data_mode() -> String { "text".into() }
fn default_virtual_port_enabled() -> bool { false }
fn default_virtual_port_count() -> u32 { 0 }

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

/// 串口协议适配器
pub struct SerialAdapter;

impl SerialAdapter {
    pub fn new() -> Self { Self }

    /// 创建串口插件清单
    pub fn manifest() -> PluginManifest {
        PluginManifest {
            id: "serial".into(),
            name: "Serial Port".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            description: "RS-232/RS-485 串口终端会话".into(),
            icon: "serial".into(),
            content_type: "terminal".into(),
            capabilities: vec![
                "connection".into(),
                "transfer".into(),
                "endpoint_discovery".into(),
            ],
            transfer_protocols: vec![
                TransferProtocolType::YModem,
                TransferProtocolType::XModem,
                TransferProtocolType::ZModem,
            ],
        }
    }

    /// 从 JSON Value 解析串口参数
    fn parse_params(params: &serde_json::Value) -> SerialConfig {
        serde_json::from_value(params.clone()).unwrap_or_default()
    }

    /// 打开串口端口（带重试）
    fn open_port(endpoint: &str, config: &SerialConfig) -> Result<Box<dyn serialport::SerialPort>, SessionError> {
        let db = match config.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let pa = match config.parity.as_str() {
            "even" => serialport::Parity::Even,
            "odd" => serialport::Parity::Odd,
            _ => serialport::Parity::None,
        };
        let sb = match config.stop_bits.as_str() {
            "2" => serialport::StopBits::Two,
            _ => serialport::StopBits::One,
        };
        let fc = match config.flow_control.as_str() {
            "rts_cts" => serialport::FlowControl::Hardware,
            "xon_xoff" => serialport::FlowControl::Software,
            _ => serialport::FlowControl::None,
        };

        let mut last_err = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match serialport::new(endpoint, config.baud_rate)
                .data_bits(db)
                .parity(pa)
                .stop_bits(sb)
                .flow_control(fc)
                .timeout(std::time::Duration::from_millis(50))
                .open()
            {
                Ok(port) => {
                    let _ = port.clear(serialport::ClearBuffer::All);
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    return Ok(port);
                }
                Err(e) => {
                    last_err = format!("无法打开端口 {}: {}", endpoint, e);
                }
            }
        }
        Err(SessionError::ConnectionFailed { reason: last_err })
    }
}

impl ProtocolAdapter for SerialAdapter {
    fn connect(
        &self,
        endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<Box<dyn Channel>, SessionError> {
        let config = Self::parse_params(params);
        let port = Self::open_port(endpoint, &config)?;
        let channel = SerialChannel::new(port);
        Ok(Box::new(channel))
    }

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        let ports = serialport::available_ports()
            .map_err(|e| SessionError::ConnectionFailed { reason: e.to_string() })?;
        Ok(ports.into_iter().map(|p| EndpointInfo {
            name: p.port_name.clone(),
            description: p.port_name,
        }).collect())
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![
            TransferProtocolType::YModem,
            TransferProtocolType::XModem,
            TransferProtocolType::ZModem,
        ]
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
    }
}
