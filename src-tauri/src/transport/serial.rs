use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::Duration;

use crate::transport::error::{TransportError, TransportErrorKind};
use crate::transport::stream::{BlockingByteStream, ReadStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialTransportConfig {
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
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: u64,
}

impl Default for SerialTransportConfig {
    fn default() -> Self {
        Self {
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            parity: default_parity(),
            stop_bits: default_stop_bits(),
            flow_control: default_flow_control(),
            read_timeout_ms: default_read_timeout_ms(),
        }
    }
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
fn default_read_timeout_ms() -> u64 {
    50
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
    let parity = match config.parity.as_str() {
        "none" => serialport::Parity::None,
        "even" => serialport::Parity::Even,
        "odd" => serialport::Parity::Odd,
        _ => unreachable!("validated"),
    };
    let stop_bits = match config.stop_bits.as_str() {
        "1" => serialport::StopBits::One,
        "2" => serialport::StopBits::Two,
        _ => unreachable!("validated"),
    };
    let flow_control = match config.flow_control.as_str() {
        "none" => serialport::FlowControl::None,
        "rts_cts" => serialport::FlowControl::Hardware,
        "xon_xoff" => serialport::FlowControl::Software,
        _ => unreachable!("validated"),
    };

    // Transport adapter performs exactly one physical open. Reconnect/retry policy belongs to the
    // Session layer, which already owns teardown timing. Hidden sleeps/retries here make one connect
    // request nondeterministic and can mask the original failure class.
    let port = serialport::new(endpoint, config.baud_rate)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow_control)
        .timeout(Duration::from_millis(config.read_timeout_ms.clamp(1, 1000)))
        .open()
        .map_err(map_open_error)?;

    // Do not purge RX/TX after opening. Bytes produced immediately after open (including boot/reset
    // output from embedded devices) are valid input and must enter the DataPlane startup buffer.
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
        _ => TransportErrorKind::Connect,
    };
    TransportError::new(kind, "serial_open", error.to_string())
}

fn validate_config(config: &SerialTransportConfig) -> Result<(), TransportError> {
    let valid = matches!(config.data_bits, 5..=8)
        && matches!(config.parity.as_str(), "none" | "even" | "odd")
        && matches!(config.stop_bits.as_str(), "1" | "2")
        && matches!(
            config.flow_control.as_str(),
            "none" | "rts_cts" | "xon_xoff"
        )
        && config.baud_rate > 0;
    if valid {
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
        assert_eq!(config.parity, "none");
        assert_eq!(config.stop_bits, "1");
        assert_eq!(config.flow_control, "none");
        assert_eq!(config.read_timeout_ms, 50);
        assert!(validate_config(&config).is_ok());
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
