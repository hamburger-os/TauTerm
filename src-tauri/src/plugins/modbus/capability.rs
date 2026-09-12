use crate::plugins::modbus::codec::ModbusRequest;
use crate::plugins::modbus::config::ModbusMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportScope {
    Any,
    SerialLine,
}

pub fn transport_scope(request: &ModbusRequest) -> TransportScope {
    match request {
        ModbusRequest::ReadExceptionStatus
        | ModbusRequest::Diagnostics { .. }
        | ModbusRequest::GetCommEventCounter
        | ModbusRequest::GetCommEventLog
        | ModbusRequest::ReportServerId => TransportScope::SerialLine,
        _ => TransportScope::Any,
    }
}

pub fn request_supported_on_mode(mode: ModbusMode, request: &ModbusRequest) -> bool {
    mode != ModbusMode::Tcp || transport_scope(request) != TransportScope::SerialLine
}

pub fn validate_client_unit_id(mode: ModbusMode, unit_id: u8) -> Result<(), String> {
    if mode != ModbusMode::Tcp && unit_id > 247 {
        return Err("serial unit_id must be 0..=247".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::modbus::codec::{ModbusRequest, RegisterReadArea};

    #[test]
    fn serial_line_capability_is_single_source_of_truth() {
        assert_eq!(
            transport_scope(&ModbusRequest::GetCommEventCounter),
            TransportScope::SerialLine
        );
        assert!(!request_supported_on_mode(
            ModbusMode::Tcp,
            &ModbusRequest::GetCommEventCounter
        ));
        assert!(request_supported_on_mode(
            ModbusMode::Tcp,
            &ModbusRequest::ReadRegisters {
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            }
        ));
    }

    #[test]
    fn serial_client_units_reject_reserved_addresses() {
        assert!(validate_client_unit_id(ModbusMode::Rtu, 247).is_ok());
        assert!(validate_client_unit_id(ModbusMode::Rtu, 248).is_err());
        assert!(validate_client_unit_id(ModbusMode::Tcp, 255).is_ok());
    }
}
