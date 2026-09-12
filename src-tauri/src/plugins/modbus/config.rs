use serde::{Deserialize, Serialize};

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
    pub fn validate(&self) -> Result<(), String> {
        if self.response_timeout_ms == 0 || self.response_timeout_ms > 120_000 {
            return Err("response_timeout_ms must be 1..=120000".into());
        }
        if self.read_retries > 10 {
            return Err("read_retries must be <= 10".into());
        }
        if self.retry_writes && self.role != ModbusRole::Client {
            return Err("retry_writes is a client-only option".into());
        }
        match self.mode {
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
            }
            ModbusMode::Tcp => {
                if self.host.trim().is_empty() {
                    return Err("host is required".into());
                }
                if self.port == 0 {
                    return Err("port must be non-zero".into());
                }
            }
        }
        if self.server_max_clients > 256 {
            return Err("server_max_clients must be <= 256".into());
        }
        if let Some(code) = self.server_fault.exception_code {
            if !(1..=11).contains(&code) {
                return Err(
                    "fault exception_code must be a standard non-zero exception code".into(),
                );
            }
        }
        Ok(())
    }

    pub fn rtu_frame_gap(&self) -> std::time::Duration {
        if self.serial.baud_rate > 19_200 {
            std::time::Duration::from_micros(1_750)
        } else {
            let data_bits = self.serial.data_bits as f64;
            let parity_bits = if self.serial.parity == "none" {
                0.0
            } else {
                1.0
            };
            let stop_bits = if self.serial.stop_bits == "2" {
                2.0
            } else {
                1.0
            };
            let bits_per_char = 1.0 + data_bits + parity_bits + stop_bits;
            let micros = (3.5 * bits_per_char * 1_000_000.0 / self.serial.baud_rate.max(1) as f64)
                .ceil() as u64;
            std::time::Duration::from_micros(micros.max(1))
        }
    }
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
