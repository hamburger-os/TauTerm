use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus};

const SERIAL_READ_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialParity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SerialStopBits {
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialFlowControl {
    None,
    RtsCts,
    XonXoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialTransportConfig {
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: SerialParity,
    pub stop_bits: SerialStopBits,
    pub flow_control: SerialFlowControl,
}

impl Default for SerialTransportConfig {
    fn default() -> Self {
        Self {
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            parity: default_parity(),
            stop_bits: default_stop_bits(),
            flow_control: default_flow_control(),
        }
    }
}

fn default_baud_rate() -> u32 {
    115_200
}
fn default_data_bits() -> u8 {
    8
}
fn default_parity() -> SerialParity {
    SerialParity::None
}
fn default_stop_bits() -> SerialStopBits {
    SerialStopBits::One
}
fn default_flow_control() -> SerialFlowControl {
    SerialFlowControl::None
}

pub fn open_serial(
    endpoint: &str,
    config: &SerialTransportConfig,
) -> Result<SerialDriver, TransportError> {
    validate_config(config)?;
    let data_bits = match config.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        8 => serialport::DataBits::Eight,
        _ => unreachable!("validated"),
    };
    let parity = match config.parity {
        SerialParity::None => serialport::Parity::None,
        SerialParity::Even => serialport::Parity::Even,
        SerialParity::Odd => serialport::Parity::Odd,
    };
    let stop_bits = match config.stop_bits {
        SerialStopBits::One => serialport::StopBits::One,
        SerialStopBits::Two => serialport::StopBits::Two,
    };
    let flow_control = match config.flow_control {
        SerialFlowControl::None => serialport::FlowControl::None,
        SerialFlowControl::RtsCts => serialport::FlowControl::Hardware,
        SerialFlowControl::XonXoff => serialport::FlowControl::Software,
    };

    // One connect request performs one physical open. Retry/reconnect belongs to the Session layer.
    let port = serialport::new(endpoint, config.baud_rate)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow_control)
        .timeout(SERIAL_READ_TIMEOUT)
        .open()
        .map_err(map_open_error)?;

    // Never purge immediately after open: startup/boot bytes are valid input.
    Ok(SerialDriver { port })
}

fn map_open_error(error: serialport::Error) -> TransportError {
    let kind = match error.kind() {
        serialport::ErrorKind::NoDevice => TransportErrorKind::DeviceNotFound,
        serialport::ErrorKind::InvalidInput => TransportErrorKind::InvalidConfiguration,
        serialport::ErrorKind::Unknown => TransportErrorKind::Connect,
        serialport::ErrorKind::Io(io_kind) => match io_kind {
            std::io::ErrorKind::PermissionDenied => TransportErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound => TransportErrorKind::DeviceNotFound,
            std::io::ErrorKind::TimedOut => TransportErrorKind::Timeout,
            std::io::ErrorKind::WouldBlock => TransportErrorKind::DeviceBusy,
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                TransportErrorKind::ConnectionReset
            }
            _ => TransportErrorKind::Connect,
        },
    };
    TransportError::new(kind, "serial_open", error.to_string())
}

fn validate_config(config: &SerialTransportConfig) -> Result<(), TransportError> {
    if matches!(config.data_bits, 5..=8) && config.baud_rate > 0 {
        Ok(())
    } else {
        Err(TransportError::new(
            TransportErrorKind::InvalidConfiguration,
            "serial_config",
            "invalid serial transport configuration",
        ))
    }
}

pub struct SerialDriver {
    port: Box<dyn serialport::SerialPort>,
}

impl BlockingByteStream for SerialDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        match self.port.read(buf) {
            Ok(0) => Ok(ReadStatus::Idle),
            Ok(n) => Ok(ReadStatus::Data(n)),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(ReadStatus::Idle)
            }
            Err(error) => Err(TransportError::io("serial_read", error)),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.port
            .write_all(data)
            .map_err(|error| TransportError::io("serial_write", error))
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        self.port
            .flush()
            .map_err(|error| TransportError::io("serial_flush", error))
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_defaults_are_stable() {
        let config = SerialTransportConfig::default();
        assert_eq!(config.baud_rate, 115_200);
        assert_eq!(config.data_bits, 8);
        assert_eq!(config.parity, SerialParity::None);
        assert_eq!(config.stop_bits, SerialStopBits::One);
        assert_eq!(config.flow_control, SerialFlowControl::None);
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn typed_link_fields_parse_from_wire_schema() {
        let config: SerialTransportConfig = serde_json::from_value(serde_json::json!({
            "baud_rate": 921600,
            "data_bits": 7,
            "parity": "even",
            "stop_bits": "2",
            "flow_control": "rts_cts"
        }))
        .unwrap();
        assert_eq!(config.parity, SerialParity::Even);
        assert_eq!(config.stop_bits, SerialStopBits::Two);
        assert_eq!(config.flow_control, SerialFlowControl::RtsCts);
    }

    #[test]
    fn invalid_serial_config_is_rejected_before_open() {
        let config = SerialTransportConfig {
            data_bits: 9,
            ..Default::default()
        };
        assert_eq!(
            validate_config(&config).unwrap_err().kind,
            TransportErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn serialport_error_kind_is_mapped_without_parsing_localized_text() {
        let permission = map_open_error(serialport::Error::new(
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "localized message",
        ));
        assert_eq!(permission.kind, TransportErrorKind::PermissionDenied);

        let invalid = map_open_error(serialport::Error::new(
            serialport::ErrorKind::InvalidInput,
            "localized message",
        ));
        assert_eq!(invalid.kind, TransportErrorKind::InvalidConfiguration);
    }
}
