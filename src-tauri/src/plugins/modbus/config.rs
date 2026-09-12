use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::transport::serial::SerialTransportConfig;
use crate::transport::tcp::TcpConnectConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModbusMode {
    Rtu,
    Ascii,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModbusRole {
    Client,
    Server,
}

/// Session/persistence DTO. It is intentionally permissive at the JSON boundary; `validated()`
/// immediately converts it into the tagged runtime domain where invalid combinations cannot exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusConfig {
    pub mode: ModbusMode,
    #[serde(default = "default_role")]
    pub role: ModbusRole,
    #[serde(default)]
    pub serial_port: String,
    #[serde(default)]
    pub serial: SerialTransportConfig,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_tcp_port")]
    pub port: u16,
    #[serde(default)]
    pub tcp: TcpConnectConfig,
    #[serde(default = "default_unit_id")]
    pub unit_id: u8,
    #[serde(default = "default_response_timeout_ms")]
    pub response_timeout_ms: u64,
    #[serde(default = "default_read_retries")]
    pub read_retries: u8,
    #[serde(default)]
    pub retry_writes: bool,
    #[serde(default = "default_server_max_clients")]
    pub server_max_clients: usize,
    #[serde(default)]
    pub server_fault: ServerFaultConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerFaultConfig {
    #[serde(default)]
    pub no_response: bool,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub exception_code: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialMode {
    Rtu,
    Ascii,
}

#[derive(Debug, Clone)]
pub enum ModbusEndpointConfig {
    Serial {
        mode: SerialMode,
        port: String,
        transport: SerialTransportConfig,
    },
    Tcp {
        host: String,
        port: u16,
        transport: TcpConnectConfig,
    },
}

#[derive(Debug, Clone)]
pub struct ClientRuntimeConfig {
    pub response_timeout_ms: u64,
    pub read_retries: u8,
    pub retry_writes: bool,
}

#[derive(Debug, Clone)]
pub struct ServerRuntimeConfig {
    pub max_clients: usize,
    pub fault: ServerFaultConfig,
}

#[derive(Debug, Clone)]
pub enum ModbusRuntimeRole {
    Client(ClientRuntimeConfig),
    Server(ServerRuntimeConfig),
}

#[derive(Debug, Clone)]
pub struct ValidatedModbusConfig {
    pub endpoint: ModbusEndpointConfig,
    pub role: ModbusRuntimeRole,
    pub unit_id: u8,
}

impl ValidatedModbusConfig {
    pub fn mode(&self) -> ModbusMode {
        match &self.endpoint {
            ModbusEndpointConfig::Serial {
                mode: SerialMode::Rtu,
                ..
            } => ModbusMode::Rtu,
            ModbusEndpointConfig::Serial {
                mode: SerialMode::Ascii,
                ..
            } => ModbusMode::Ascii,
            ModbusEndpointConfig::Tcp { .. } => ModbusMode::Tcp,
        }
    }

    pub fn role(&self) -> ModbusRole {
        match &self.role {
            ModbusRuntimeRole::Client(_) => ModbusRole::Client,
            ModbusRuntimeRole::Server(_) => ModbusRole::Server,
        }
    }

    pub fn client(&self) -> Option<&ClientRuntimeConfig> {
        match &self.role {
            ModbusRuntimeRole::Client(config) => Some(config),
            ModbusRuntimeRole::Server(_) => None,
        }
    }

    pub fn server(&self) -> Option<&ServerRuntimeConfig> {
        match &self.role {
            ModbusRuntimeRole::Server(config) => Some(config),
            ModbusRuntimeRole::Client(_) => None,
        }
    }

    pub fn serial(&self) -> Option<(SerialMode, &str, &SerialTransportConfig)> {
        match &self.endpoint {
            ModbusEndpointConfig::Serial {
                mode,
                port,
                transport,
            } => Some((*mode, port.as_str(), transport)),
            ModbusEndpointConfig::Tcp { .. } => None,
        }
    }

    pub fn tcp(&self) -> Option<(&str, u16, &TcpConnectConfig)> {
        match &self.endpoint {
            ModbusEndpointConfig::Tcp {
                host,
                port,
                transport,
            } => Some((host.as_str(), *port, transport)),
            ModbusEndpointConfig::Serial { .. } => None,
        }
    }

    pub fn rtu_inter_char_gap(&self) -> Duration {
        match &self.endpoint {
            ModbusEndpointConfig::Serial { transport, .. } => serial_gap(transport, 1.5, 750),
            ModbusEndpointConfig::Tcp { .. } => Duration::ZERO,
        }
    }

    pub fn rtu_frame_gap(&self) -> Duration {
        match &self.endpoint {
            ModbusEndpointConfig::Serial { transport, .. } => serial_gap(transport, 3.5, 1_750),
            ModbusEndpointConfig::Tcp { .. } => Duration::ZERO,
        }
    }
}

fn default_role() -> ModbusRole {
    ModbusRole::Client
}
fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_tcp_port() -> u16 {
    502
}
fn default_unit_id() -> u8 {
    1
}
fn default_response_timeout_ms() -> u64 {
    1000
}
fn default_read_retries() -> u8 {
    1
}
fn default_server_max_clients() -> usize {
    16
}

impl ModbusConfig {
    pub fn validated(&self) -> Result<ValidatedModbusConfig, String> {
        let endpoint = match self.mode {
            ModbusMode::Rtu | ModbusMode::Ascii => {
                if self.serial_port.trim().is_empty() {
                    return Err("serial_port is required".into());
                }
                if self.unit_id > 247 {
                    return Err("serial unit_id must be 0..=247".into());
                }
                if self.role == ModbusRole::Server && self.unit_id == 0 {
                    return Err("server unit_id cannot be broadcast address 0".into());
                }
                ModbusEndpointConfig::Serial {
                    mode: if self.mode == ModbusMode::Rtu {
                        SerialMode::Rtu
                    } else {
                        SerialMode::Ascii
                    },
                    port: self.serial_port.trim().to_string(),
                    transport: self.serial.clone(),
                }
            }
            ModbusMode::Tcp => {
                if self.host.trim().is_empty() {
                    return Err("host is required".into());
                }
                if self.port == 0 {
                    return Err("port must be non-zero".into());
                }
                ModbusEndpointConfig::Tcp {
                    host: self.host.trim().to_string(),
                    port: self.port,
                    transport: self.tcp.clone(),
                }
            }
        };

        let role = match self.role {
            ModbusRole::Client => {
                if self.response_timeout_ms == 0 || self.response_timeout_ms > 120_000 {
                    return Err("response_timeout_ms must be 1..=120000".into());
                }
                if self.read_retries > 10 {
                    return Err("read_retries must be <= 10".into());
                }
                ModbusRuntimeRole::Client(ClientRuntimeConfig {
                    response_timeout_ms: self.response_timeout_ms,
                    read_retries: self.read_retries,
                    retry_writes: self.retry_writes,
                })
            }
            ModbusRole::Server => {
                if self.server_max_clients > 256 {
                    return Err("server_max_clients must be <= 256".into());
                }
                validate_fault(&self.server_fault)?;
                ModbusRuntimeRole::Server(ServerRuntimeConfig {
                    max_clients: self.server_max_clients,
                    fault: self.server_fault.clone(),
                })
            }
        };

        Ok(ValidatedModbusConfig {
            endpoint,
            role,
            unit_id: self.unit_id,
        })
    }
}

pub fn validate_fault(fault: &ServerFaultConfig) -> Result<(), String> {
    if fault.delay_ms > 60_000 {
        return Err("fault delay_ms must be <= 60000".into());
    }
    if let Some(code) = fault.exception_code {
        if !matches!(code, 1..=6 | 8 | 10 | 11) {
            return Err("fault exception_code must be a standard Modbus exception code".into());
        }
    }
    Ok(())
}

fn serial_gap(config: &SerialTransportConfig, chars: f64, high_speed_micros: u64) -> Duration {
    if config.baud_rate > 19_200 {
        return Duration::from_micros(high_speed_micros);
    }
    let data_bits = config.data_bits as f64;
    let parity_bits = if config.parity == "none" { 0.0 } else { 1.0 };
    let stop_bits = if config.stop_bits == "2" { 2.0 } else { 1.0 };
    let bits_per_char = 1.0 + data_bits + parity_bits + stop_bits;
    let micros =
        (chars * bits_per_char * 1_000_000.0 / config.baud_rate.max(1) as f64).ceil() as u64;
    Duration::from_micros(micros.max(1))
}

impl Default for ModbusConfig {
    fn default() -> Self {
        Self {
            mode: ModbusMode::Rtu,
            role: ModbusRole::Client,
            serial_port: String::new(),
            serial: SerialTransportConfig::default(),
            host: default_host(),
            port: default_tcp_port(),
            tcp: TcpConnectConfig::default(),
            unit_id: default_unit_id(),
            response_timeout_ms: default_response_timeout_ms(),
            read_retries: default_read_retries(),
            retry_writes: false,
            server_max_clients: default_server_max_clients(),
            server_fault: ServerFaultConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serial_client() -> ModbusConfig {
        ModbusConfig {
            serial_port: "COM1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn runtime_config_is_tagged_by_transport_and_role() {
        let config = serial_client().validated().unwrap();
        assert!(matches!(
            config.endpoint,
            ModbusEndpointConfig::Serial {
                mode: SerialMode::Rtu,
                ..
            }
        ));
        assert!(matches!(config.role, ModbusRuntimeRole::Client(_)));
    }

    #[test]
    fn inactive_role_fields_do_not_invalidate_runtime_config() {
        let config = ModbusConfig {
            mode: ModbusMode::Tcp,
            role: ModbusRole::Server,
            host: "127.0.0.1".into(),
            response_timeout_ms: 0,
            read_retries: 255,
            retry_writes: true,
            ..Default::default()
        };
        assert!(config.validated().is_ok());
    }

    #[test]
    fn server_rejects_non_standard_fault_exception() {
        let config = ModbusConfig {
            mode: ModbusMode::Tcp,
            role: ModbusRole::Server,
            host: "127.0.0.1".into(),
            server_fault: ServerFaultConfig {
                exception_code: Some(7),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validated().is_err());
    }

    #[test]
    fn high_speed_rtu_uses_standard_fixed_timing() {
        let mut config = serial_client();
        config.serial.baud_rate = 115_200;
        let config = config.validated().unwrap();
        assert_eq!(config.rtu_inter_char_gap().as_micros(), 750);
        assert_eq!(config.rtu_frame_gap().as_micros(), 1_750);
    }

    #[test]
    fn low_speed_rtu_timing_tracks_character_width() {
        let mut config = serial_client();
        config.serial.baud_rate = 9_600;
        config.serial.data_bits = 8;
        config.serial.parity = "none".into();
        config.serial.stop_bits = "1".into();
        let config = config.validated().unwrap();
        assert_eq!(config.rtu_inter_char_gap().as_micros(), 1_563);
        assert_eq!(config.rtu_frame_gap().as_micros(), 3_646);
    }
}
