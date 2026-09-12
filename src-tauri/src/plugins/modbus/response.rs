use serde::Serialize;

use crate::plugins::modbus::codec::{ModbusRequest, ModbusResponse};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticResponse {
    Bits { values: Vec<bool> },
    Registers { values: Vec<u16> },
    Acknowledged,
    Diagnostics { sub_function: u16, data: Vec<u8> },
    Raw { data: Vec<u8> },
}

pub fn decode_semantic_response(
    request: &ModbusRequest,
    response: &ModbusResponse,
) -> Option<SemanticResponse> {
    if response.exception.is_some() {
        return None;
    }

    match request {
        ModbusRequest::ReadBits { quantity, .. } => {
            let bytes = response.data.get(1..)?;
            let values = (0..*quantity as usize)
                .map(|index| {
                    let byte = bytes.get(index / 8).copied().unwrap_or(0);
                    ((byte >> (index % 8)) & 1) != 0
                })
                .collect();
            Some(SemanticResponse::Bits { values })
        }
        ModbusRequest::ReadRegisters { .. }
        | ModbusRequest::ReadWriteMultipleRegisters { .. } => {
            let bytes = response.data.get(1..)?;
            if !bytes.len().is_multiple_of(2) {
                return None;
            }
            let values = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            Some(SemanticResponse::Registers { values })
        }
        ModbusRequest::WriteSingleCoil { .. }
        | ModbusRequest::WriteSingleRegister { .. }
        | ModbusRequest::WriteMultipleCoils { .. }
        | ModbusRequest::WriteMultipleRegisters { .. }
        | ModbusRequest::WriteFileRecord { .. }
        | ModbusRequest::MaskWriteRegister { .. } => Some(SemanticResponse::Acknowledged),
        ModbusRequest::Diagnostics { sub_function, .. } => Some(SemanticResponse::Diagnostics {
            sub_function: *sub_function,
            data: response.data.get(2..).unwrap_or_default().to_vec(),
        }),
        _ => Some(SemanticResponse::Raw {
            data: response.data.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::modbus::codec::{BitReadArea, RegisterReadArea};

    #[test]
    fn read_register_response_is_semantic() {
        let request = ModbusRequest::ReadRegisters {
            area: RegisterReadArea::HoldingRegisters,
            address: 0,
            quantity: 2,
        };
        let response = ModbusResponse {
            function: 3,
            data: vec![4, 0x12, 0x34, 0x56, 0x78],
            exception: None,
        };
        assert_eq!(
            decode_semantic_response(&request, &response),
            Some(SemanticResponse::Registers {
                values: vec![0x1234, 0x5678],
            })
        );
    }

    #[test]
    fn bit_response_uses_requested_quantity() {
        let request = ModbusRequest::ReadBits {
            area: BitReadArea::Coils,
            address: 0,
            quantity: 3,
        };
        let response = ModbusResponse {
            function: 1,
            data: vec![1, 0b0000_0101],
            exception: None,
        };
        assert_eq!(
            decode_semantic_response(&request, &response),
            Some(SemanticResponse::Bits {
                values: vec![true, false, true],
            })
        );
    }
}
