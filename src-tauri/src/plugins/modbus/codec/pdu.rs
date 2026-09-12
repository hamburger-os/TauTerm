use serde::{Deserialize, Serialize};

pub const MAX_PDU_LEN: usize = 253;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModbusRequest {
    ReadBits {
        function: u8,
        address: u16,
        quantity: u16,
    },
    ReadRegisters {
        function: u8,
        address: u16,
        quantity: u16,
    },
    WriteSingle {
        function: u8,
        address: u16,
        value: u16,
    },
    ReadExceptionStatus,
    Diagnostics {
        sub_function: u16,
        data: Vec<u8>,
    },
    GetCommEventCounter,
    GetCommEventLog,
    WriteMultipleCoils {
        address: u16,
        quantity: u16,
        values: Vec<u8>,
    },
    WriteMultipleRegisters {
        address: u16,
        values: Vec<u16>,
    },
    ReportServerId,
    ReadFileRecord {
        records: Vec<FileRecordRead>,
    },
    WriteFileRecord {
        records: Vec<FileRecordWrite>,
    },
    MaskWriteRegister {
        address: u16,
        and_mask: u16,
        or_mask: u16,
    },
    ReadWriteMultipleRegisters {
        read_address: u16,
        read_quantity: u16,
        write_address: u16,
        values: Vec<u16>,
    },
    ReadFifoQueue {
        address: u16,
    },
    Mei {
        mei_type: u8,
        data: Vec<u8>,
    },
    Raw {
        function: u8,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecordRead {
    pub file_number: u16,
    pub record_number: u16,
    pub record_length: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecordWrite {
    pub file_number: u16,
    pub record_number: u16,
    pub values: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedRequest {
    pub function: u8,
    pub request: ModbusRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModbusResponse {
    pub function: u8,
    pub data: Vec<u8>,
    pub exception: Option<u8>,
}

impl ModbusRequest {
    pub fn function(&self) -> u8 {
        match self {
            Self::ReadBits { function, .. }
            | Self::ReadRegisters { function, .. }
            | Self::WriteSingle { function, .. }
            | Self::Raw { function, .. } => *function,
            Self::ReadExceptionStatus => 0x07,
            Self::Diagnostics { .. } => 0x08,
            Self::GetCommEventCounter => 0x0B,
            Self::GetCommEventLog => 0x0C,
            Self::WriteMultipleCoils { .. } => 0x0F,
            Self::WriteMultipleRegisters { .. } => 0x10,
            Self::ReportServerId => 0x11,
            Self::ReadFileRecord { .. } => 0x14,
            Self::WriteFileRecord { .. } => 0x15,
            Self::MaskWriteRegister { .. } => 0x16,
            Self::ReadWriteMultipleRegisters { .. } => 0x17,
            Self::ReadFifoQueue { .. } => 0x18,
            Self::Mei { .. } => 0x2B,
        }
    }

    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Self::WriteSingle { .. }
                | Self::WriteMultipleCoils { .. }
                | Self::WriteMultipleRegisters { .. }
                | Self::WriteFileRecord { .. }
                | Self::MaskWriteRegister { .. }
                | Self::ReadWriteMultipleRegisters { .. }
        )
    }
}

pub fn encode_request(request: &ModbusRequest) -> Result<Vec<u8>, String> {
    let mut pdu = vec![request.function()];
    match request {
        ModbusRequest::ReadBits {
            function,
            address,
            quantity,
        } => {
            if !matches!(*function, 0x01 | 0x02) {
                return Err("ReadBits function must be 0x01 or 0x02".into());
            }
            ensure_range(*quantity, 1, 2000, "bit quantity")?;
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, *quantity);
        }
        ModbusRequest::ReadRegisters {
            function,
            address,
            quantity,
        } => {
            if !matches!(*function, 0x03 | 0x04) {
                return Err("ReadRegisters function must be 0x03 or 0x04".into());
            }
            ensure_range(*quantity, 1, 125, "register quantity")?;
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, *quantity);
        }
        ModbusRequest::WriteSingle {
            function,
            address,
            value,
        } => {
            if !matches!(*function, 0x05 | 0x06) {
                return Err("WriteSingle function must be 0x05 or 0x06".into());
            }
            if *function == 0x05 && !matches!(*value, 0x0000 | 0xFF00) {
                return Err("coil value must be 0x0000 or 0xFF00".into());
            }
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, *value);
        }
        ModbusRequest::ReadExceptionStatus
        | ModbusRequest::GetCommEventCounter
        | ModbusRequest::GetCommEventLog
        | ModbusRequest::ReportServerId => {}
        ModbusRequest::Diagnostics { sub_function, data } => {
            push_u16(&mut pdu, *sub_function);
            pdu.extend_from_slice(data);
        }
        ModbusRequest::WriteMultipleCoils {
            address,
            quantity,
            values,
        } => {
            ensure_range(*quantity, 1, 1968, "coil quantity")?;
            let expected = (*quantity as usize).div_ceil(8);
            if values.len() != expected {
                return Err(format!("coil byte count must be {expected}"));
            }
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, *quantity);
            pdu.push(values.len() as u8);
            pdu.extend_from_slice(values);
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            ensure_range(values.len() as u16, 1, 123, "register quantity")?;
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, values.len() as u16);
            pdu.push((values.len() * 2) as u8);
            for value in values {
                push_u16(&mut pdu, *value);
            }
        }
        ModbusRequest::ReadFileRecord { records } => {
            if records.is_empty() {
                return Err("at least one file record is required".into());
            }
            let byte_count = records.len() * 7;
            if byte_count > 0xF5 {
                return Err("file record request exceeds PDU limit".into());
            }
            pdu.push(byte_count as u8);
            for record in records {
                ensure_range(record.record_length, 1, 125, "file record length")?;
                pdu.push(0x06);
                push_u16(&mut pdu, record.file_number);
                push_u16(&mut pdu, record.record_number);
                push_u16(&mut pdu, record.record_length);
            }
        }
        ModbusRequest::WriteFileRecord { records } => {
            if records.is_empty() {
                return Err("at least one file record is required".into());
            }
            let byte_count = records.iter().try_fold(0usize, |acc, record| {
                if record.values.is_empty() {
                    return Err("file record values cannot be empty".to_string());
                }
                Ok(acc + 7 + record.values.len() * 2)
            })?;
            if byte_count > 0xF5 {
                return Err("file record request exceeds PDU limit".into());
            }
            pdu.push(byte_count as u8);
            for record in records {
                pdu.push(0x06);
                push_u16(&mut pdu, record.file_number);
                push_u16(&mut pdu, record.record_number);
                push_u16(&mut pdu, record.values.len() as u16);
                for value in &record.values {
                    push_u16(&mut pdu, *value);
                }
            }
        }
        ModbusRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => {
            push_u16(&mut pdu, *address);
            push_u16(&mut pdu, *and_mask);
            push_u16(&mut pdu, *or_mask);
        }
        ModbusRequest::ReadWriteMultipleRegisters {
            read_address,
            read_quantity,
            write_address,
            values,
        } => {
            ensure_range(*read_quantity, 1, 125, "read quantity")?;
            ensure_range(values.len() as u16, 1, 121, "write quantity")?;
            push_u16(&mut pdu, *read_address);
            push_u16(&mut pdu, *read_quantity);
            push_u16(&mut pdu, *write_address);
            push_u16(&mut pdu, values.len() as u16);
            pdu.push((values.len() * 2) as u8);
            for value in values {
                push_u16(&mut pdu, *value);
            }
        }
        ModbusRequest::ReadFifoQueue { address } => push_u16(&mut pdu, *address),
        ModbusRequest::Mei { mei_type, data } => {
            pdu.push(*mei_type);
            pdu.extend_from_slice(data);
        }
        ModbusRequest::Raw { data, .. } => pdu.extend_from_slice(data),
    }
    if pdu.len() > MAX_PDU_LEN {
        return Err(format!("PDU exceeds {MAX_PDU_LEN} bytes"));
    }
    Ok(pdu)
}

pub fn decode_request(pdu: &[u8]) -> Result<DecodedRequest, String> {
    if pdu.is_empty() {
        return Err("empty PDU".into());
    }
    let function = pdu[0];
    let data = &pdu[1..];
    let request = match function {
        0x01 | 0x02 => {
            require_len(data, 4)?;
            let quantity = u16_at(data, 2)?;
            ensure_range(quantity, 1, 2000, "bit quantity")?;
            ModbusRequest::ReadBits {
                function,
                address: u16_at(data, 0)?,
                quantity,
            }
        }
        0x03 | 0x04 => {
            require_len(data, 4)?;
            let quantity = u16_at(data, 2)?;
            ensure_range(quantity, 1, 125, "register quantity")?;
            ModbusRequest::ReadRegisters {
                function,
                address: u16_at(data, 0)?,
                quantity,
            }
        }
        0x05 | 0x06 => {
            require_len(data, 4)?;
            let value = u16_at(data, 2)?;
            if function == 0x05 && !matches!(value, 0x0000 | 0xFF00) {
                return Err("invalid coil value".into());
            }
            ModbusRequest::WriteSingle {
                function,
                address: u16_at(data, 0)?,
                value,
            }
        }
        0x07 => {
            require_len(data, 0)?;
            ModbusRequest::ReadExceptionStatus
        }
        0x08 => {
            if data.len() < 2 {
                return Err("diagnostics PDU too short".into());
            }
            ModbusRequest::Diagnostics {
                sub_function: u16_at(data, 0)?,
                data: data[2..].to_vec(),
            }
        }
        0x0B => {
            require_len(data, 0)?;
            ModbusRequest::GetCommEventCounter
        }
        0x0C => {
            require_len(data, 0)?;
            ModbusRequest::GetCommEventLog
        }
        0x0F => {
            if data.len() < 5 {
                return Err("write coils PDU too short".into());
            }
            let address = u16_at(data, 0)?;
            let quantity = u16_at(data, 2)?;
            ensure_range(quantity, 1, 1968, "coil quantity")?;
            let count = data[4] as usize;
            if count != (quantity as usize).div_ceil(8) || data.len() != 5 + count {
                return Err("invalid coil byte count".into());
            }
            ModbusRequest::WriteMultipleCoils {
                address,
                quantity,
                values: data[5..].to_vec(),
            }
        }
        0x10 => {
            if data.len() < 5 {
                return Err("write registers PDU too short".into());
            }
            let address = u16_at(data, 0)?;
            let quantity = u16_at(data, 2)?;
            ensure_range(quantity, 1, 123, "register quantity")?;
            let count = data[4] as usize;
            if count != quantity as usize * 2 || data.len() != 5 + count {
                return Err("invalid register byte count".into());
            }
            ModbusRequest::WriteMultipleRegisters {
                address,
                values: words(&data[5..])?,
            }
        }
        0x11 => {
            require_len(data, 0)?;
            ModbusRequest::ReportServerId
        }
        0x14 => ModbusRequest::ReadFileRecord {
            records: decode_file_reads(data)?,
        },
        0x15 => ModbusRequest::WriteFileRecord {
            records: decode_file_writes(data)?,
        },
        0x16 => {
            require_len(data, 6)?;
            ModbusRequest::MaskWriteRegister {
                address: u16_at(data, 0)?,
                and_mask: u16_at(data, 2)?,
                or_mask: u16_at(data, 4)?,
            }
        }
        0x17 => {
            if data.len() < 9 {
                return Err("read/write PDU too short".into());
            }
            let read_address = u16_at(data, 0)?;
            let read_quantity = u16_at(data, 2)?;
            let write_address = u16_at(data, 4)?;
            let write_quantity = u16_at(data, 6)?;
            ensure_range(read_quantity, 1, 125, "read quantity")?;
            ensure_range(write_quantity, 1, 121, "write quantity")?;
            let count = data[8] as usize;
            if count != write_quantity as usize * 2 || data.len() != 9 + count {
                return Err("invalid read/write byte count".into());
            }
            ModbusRequest::ReadWriteMultipleRegisters {
                read_address,
                read_quantity,
                write_address,
                values: words(&data[9..])?,
            }
        }
        0x18 => {
            require_len(data, 2)?;
            ModbusRequest::ReadFifoQueue {
                address: u16_at(data, 0)?,
            }
        }
        0x2B => {
            if data.is_empty() {
                return Err("MEI PDU too short".into());
            }
            ModbusRequest::Mei {
                mei_type: data[0],
                data: data[1..].to_vec(),
            }
        }
        _ => ModbusRequest::Raw {
            function,
            data: data.to_vec(),
        },
    };
    Ok(DecodedRequest { function, request })
}

pub fn validate_response(request: &ModbusRequest, pdu: &[u8]) -> Result<ModbusResponse, String> {
    if pdu.is_empty() {
        return Err("empty response PDU".into());
    }
    let function = request.function();
    if pdu[0] == (function | 0x80) {
        if pdu.len() != 2 {
            return Err("exception response must be exactly 2 bytes".into());
        }
        return Ok(ModbusResponse {
            function,
            data: Vec::new(),
            exception: Some(pdu[1]),
        });
    }
    if pdu[0] != function {
        return Err(format!(
            "function mismatch: expected 0x{function:02X}, got 0x{:02X}",
            pdu[0]
        ));
    }
    let data = &pdu[1..];
    match request {
        ModbusRequest::ReadBits { quantity, .. } => {
            validate_byte_count(data, (*quantity as usize).div_ceil(8))?;
        }
        ModbusRequest::ReadRegisters { quantity, .. } => {
            validate_byte_count(data, *quantity as usize * 2)?;
        }
        ModbusRequest::WriteSingle { address, value, .. } => {
            require_len(data, 4)?;
            if u16_at(data, 0)? != *address || u16_at(data, 2)? != *value {
                return Err("write-single echo mismatch".into());
            }
        }
        ModbusRequest::WriteMultipleCoils {
            address, quantity, ..
        } => {
            validate_write_multiple_echo(data, *address, *quantity)?;
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            validate_write_multiple_echo(data, *address, values.len() as u16)?;
        }
        ModbusRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => {
            require_len(data, 6)?;
            if u16_at(data, 0)? != *address
                || u16_at(data, 2)? != *and_mask
                || u16_at(data, 4)? != *or_mask
            {
                return Err("mask-write echo mismatch".into());
            }
        }
        ModbusRequest::ReadWriteMultipleRegisters { read_quantity, .. } => {
            validate_byte_count(data, *read_quantity as usize * 2)?;
        }
        ModbusRequest::ReadFileRecord { .. } => validate_count_prefixed(data)?,
        ModbusRequest::WriteFileRecord { .. } => {
            let expected = &encode_request(request)?[1..];
            if data != expected {
                return Err("write-file response does not echo request".into());
            }
        }
        ModbusRequest::ReadExceptionStatus => require_len(data, 1)?,
        ModbusRequest::Diagnostics {
            sub_function,
            data: request_data,
        } => {
            if data.len() < 2 || u16_at(data, 0)? != *sub_function {
                return Err("diagnostics sub-function mismatch".into());
            }
            if *sub_function == 0 && data[2..] != request_data[..] {
                return Err("diagnostics loopback data mismatch".into());
            }
        }
        ModbusRequest::GetCommEventCounter => require_len(data, 4)?,
        ModbusRequest::GetCommEventLog | ModbusRequest::ReportServerId => {
            validate_count_prefixed(data)?;
        }
        ModbusRequest::ReadFifoQueue { .. } => validate_fifo_response(data)?,
        ModbusRequest::Mei { mei_type, .. } => {
            if data.first().copied() != Some(*mei_type) {
                return Err("MEI type mismatch".into());
            }
        }
        ModbusRequest::Raw { .. } => {}
    }
    Ok(ModbusResponse {
        function,
        data: data.to_vec(),
        exception: None,
    })
}

fn validate_write_multiple_echo(data: &[u8], address: u16, quantity: u16) -> Result<(), String> {
    require_len(data, 4)?;
    if u16_at(data, 0)? != address {
        return Err("write-multiple address echo mismatch".into());
    }
    if u16_at(data, 2)? != quantity {
        return Err("write-multiple quantity echo mismatch".into());
    }
    Ok(())
}

fn validate_byte_count(data: &[u8], expected: usize) -> Result<(), String> {
    if data.is_empty() {
        return Err("missing byte count".into());
    }
    let count = data[0] as usize;
    if count != expected || data.len() != count + 1 {
        return Err(format!(
            "byte count mismatch: expected {expected}, got {count}"
        ));
    }
    Ok(())
}

fn validate_count_prefixed(data: &[u8]) -> Result<(), String> {
    if data.is_empty() {
        return Err("response missing byte count".into());
    }
    let count = data[0] as usize;
    if count != data.len() - 1 {
        return Err("response byte count does not match payload length".into());
    }
    Ok(())
}

fn validate_fifo_response(data: &[u8]) -> Result<(), String> {
    if data.len() < 4 {
        return Err("FIFO response too short".into());
    }
    let byte_count = u16_at(data, 0)? as usize;
    if byte_count < 2 || data.len() != byte_count + 2 {
        return Err("FIFO byte count mismatch".into());
    }
    let fifo_count = u16_at(data, 2)? as usize;
    if fifo_count > 31 || byte_count != 2 + fifo_count * 2 {
        return Err("FIFO count mismatch".into());
    }
    Ok(())
}

fn decode_file_reads(data: &[u8]) -> Result<Vec<FileRecordRead>, String> {
    if data.is_empty() || data[0] as usize != data.len() - 1 || !(data.len() - 1).is_multiple_of(7)
    {
        return Err("invalid read-file byte count".into());
    }
    let mut out = Vec::new();
    for chunk in data[1..].as_chunks::<7>().0 {
        if chunk[0] != 0x06 {
            return Err("file reference type must be 0x06".into());
        }
        let record_length = u16_at(chunk, 5)?;
        ensure_range(record_length, 1, 125, "file record length")?;
        out.push(FileRecordRead {
            file_number: u16_at(chunk, 1)?,
            record_number: u16_at(chunk, 3)?,
            record_length,
        });
    }
    Ok(out)
}

fn decode_file_writes(data: &[u8]) -> Result<Vec<FileRecordWrite>, String> {
    if data.is_empty() || data[0] as usize != data.len() - 1 {
        return Err("invalid write-file byte count".into());
    }
    let mut cursor = 1usize;
    let mut out = Vec::new();
    while cursor < data.len() {
        if cursor + 7 > data.len() || data[cursor] != 0x06 {
            return Err("invalid file record header".into());
        }
        let file_number = u16_at(data, cursor + 1)?;
        let record_number = u16_at(data, cursor + 3)?;
        let count = u16_at(data, cursor + 5)? as usize;
        if count == 0 {
            return Err("file record values cannot be empty".into());
        }
        cursor += 7;
        let bytes = count.checked_mul(2).ok_or("record length overflow")?;
        if cursor + bytes > data.len() {
            return Err("truncated file record values".into());
        }
        out.push(FileRecordWrite {
            file_number,
            record_number,
            values: words(&data[cursor..cursor + bytes])?,
        });
        cursor += bytes;
    }
    Ok(out)
}

fn words(bytes: &[u8]) -> Result<Vec<u16>, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("register bytes must be even".into());
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect())
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| "PDU truncated".to_string())?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn require_len(data: &[u8], expected: usize) -> Result<(), String> {
    if data.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "invalid PDU length: expected {expected}, got {}",
            data.len()
        ))
    }
}

fn ensure_range(value: u16, min: u16, max: u16, label: &str) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{label} must be {min}..={max}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_quantity_limits_are_enforced() {
        assert!(encode_request(&ModbusRequest::ReadRegisters {
            function: 0x03,
            address: 0,
            quantity: 125,
        })
        .is_ok());
        assert!(encode_request(&ModbusRequest::ReadRegisters {
            function: 0x03,
            address: 0,
            quantity: 126,
        })
        .is_err());
        assert!(encode_request(&ModbusRequest::ReadBits {
            function: 0x01,
            address: 0,
            quantity: 2000,
        })
        .is_ok());
        assert!(encode_request(&ModbusRequest::ReadBits {
            function: 0x01,
            address: 0,
            quantity: 2001,
        })
        .is_err());
    }

    #[test]
    fn write_response_echo_is_strict() {
        let request = ModbusRequest::WriteSingle {
            function: 0x06,
            address: 0x0010,
            value: 0x1234,
        };
        assert!(validate_response(&request, &[0x06, 0x00, 0x10, 0x12, 0x34]).is_ok());
        assert!(validate_response(&request, &[0x06, 0x00, 0x11, 0x12, 0x34]).is_err());
    }

    #[test]
    fn write_multiple_response_uses_request_specific_quantity() {
        let registers = ModbusRequest::WriteMultipleRegisters {
            address: 0x0010,
            values: vec![1, 2, 3],
        };
        assert!(validate_response(&registers, &[0x10, 0x00, 0x10, 0x00, 0x03]).is_ok());
        assert!(validate_response(&registers, &[0x10, 0x00, 0x10, 0x00, 0x02]).is_err());

        let coils = ModbusRequest::WriteMultipleCoils {
            address: 0x0020,
            quantity: 9,
            values: vec![0xAA, 0x01],
        };
        assert!(validate_response(&coils, &[0x0F, 0x00, 0x20, 0x00, 0x09]).is_ok());
    }

    #[test]
    fn fifo_response_is_structurally_validated() {
        let request = ModbusRequest::ReadFifoQueue { address: 1 };
        assert!(validate_response(
            &request,
            &[0x18, 0x00, 0x06, 0x00, 0x02, 0x12, 0x34, 0x56, 0x78]
        )
        .is_ok());
        assert!(validate_response(&request, &[0x18, 0x00, 0x06, 0x00, 0x03, 0x12, 0x34]).is_err());
    }
}
