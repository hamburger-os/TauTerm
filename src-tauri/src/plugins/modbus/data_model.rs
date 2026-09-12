use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::plugins::modbus::codec::{FileRecordRead, FileRecordWrite, ModbusRequest};

pub const EX_ILLEGAL_FUNCTION: u8 = 0x01;
pub const EX_ILLEGAL_DATA_ADDRESS: u8 = 0x02;
pub const EX_ILLEGAL_DATA_VALUE: u8 = 0x03;
pub const EX_SERVER_DEVICE_FAILURE: u8 = 0x04;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataModelSnapshot {
    pub coils: Vec<(u16, bool)>,
    pub discrete_inputs: Vec<(u16, bool)>,
    pub holding_registers: Vec<(u16, u16)>,
    pub input_registers: Vec<(u16, u16)>,
}

#[derive(Default)]
struct ModelInner {
    coils: HashMap<u16, bool>,
    discrete_inputs: HashMap<u16, bool>,
    holding_registers: HashMap<u16, u16>,
    input_registers: HashMap<u16, u16>,
    file_records: HashMap<(u16, u16), u16>,
    fifo: HashMap<u16, Vec<u16>>,
    device_objects: BTreeMap<u8, String>,
    exception_status: u8,
    comm_event_count: u16,
}

pub struct ModbusDataModel {
    inner: RwLock<ModelInner>,
}

impl Default for ModbusDataModel {
    fn default() -> Self {
        let mut inner = ModelInner::default();
        inner.device_objects.insert(0x00, "TauTerm".into());
        inner
            .device_objects
            .insert(0x01, env!("CARGO_PKG_VERSION").into());
        inner.device_objects.insert(0x02, "Modbus Simulator".into());
        Self {
            inner: RwLock::new(inner),
        }
    }
}

impl ModbusDataModel {
    pub fn set_coil(&self, address: u16, value: bool) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .coils
            .insert(address, value);
    }

    pub fn set_discrete_input(&self, address: u16, value: bool) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .discrete_inputs
            .insert(address, value);
    }

    pub fn set_holding_register(&self, address: u16, value: u16) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .holding_registers
            .insert(address, value);
    }

    pub fn set_input_register(&self, address: u16, value: u16) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .input_registers
            .insert(address, value);
    }

    pub fn snapshot(&self) -> DataModelSnapshot {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        DataModelSnapshot {
            coils: sorted_bool(&inner.coils),
            discrete_inputs: sorted_bool(&inner.discrete_inputs),
            holding_registers: sorted_u16(&inner.holding_registers),
            input_registers: sorted_u16(&inner.input_registers),
        }
    }

    /// Execute one decoded request atomically and return a full response PDU.
    ///
    /// All range/address validation that can fail after a mutation is completed before
    /// the mutation is committed. Failed requests therefore cannot leave a partially
    /// updated simulator state behind.
    pub fn execute(&self, request: &ModbusRequest) -> Result<Vec<u8>, u8> {
        let mut inner = self.inner.write().map_err(|_| EX_SERVER_DEVICE_FAILURE)?;
        let result = execute_request(&mut inner, request);
        if result.is_ok() && counts_comm_event(request) {
            inner.comm_event_count = inner.comm_event_count.wrapping_add(1);
        }
        result
    }
}

fn execute_request(inner: &mut ModelInner, request: &ModbusRequest) -> Result<Vec<u8>, u8> {
    let function = request.function();
    let mut out = vec![function];
    match request {
        ModbusRequest::ReadBits {
            function,
            address,
            quantity,
        } => {
            let source = if *function == 0x01 {
                &inner.coils
            } else {
                &inner.discrete_inputs
            };
            let values = read_bool_range(source, *address, *quantity)?;
            let packed = pack_bits(&values);
            out.push(packed.len() as u8);
            out.extend_from_slice(&packed);
        }
        ModbusRequest::ReadRegisters {
            function,
            address,
            quantity,
        } => {
            let source = if *function == 0x03 {
                &inner.holding_registers
            } else {
                &inner.input_registers
            };
            let values = read_u16_range(source, *address, *quantity)?;
            out.push((values.len() * 2) as u8);
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        ModbusRequest::WriteSingle {
            function,
            address,
            value,
        } => {
            match function {
                0x05 => {
                    inner.coils.insert(*address, *value == 0xFF00);
                }
                0x06 => {
                    inner.holding_registers.insert(*address, *value);
                }
                _ => return Err(EX_ILLEGAL_FUNCTION),
            }
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
        }
        ModbusRequest::ReadExceptionStatus => out.push(inner.exception_status),
        ModbusRequest::Diagnostics { sub_function, data } => {
            out.extend_from_slice(&sub_function.to_be_bytes());
            match *sub_function {
                0x0000 => out.extend_from_slice(data),
                0x000A if data.is_empty() => {
                    inner.comm_event_count = 0;
                    inner.exception_status = 0;
                }
                0x000A => return Err(EX_ILLEGAL_DATA_VALUE),
                _ => return Err(EX_ILLEGAL_DATA_VALUE),
            }
        }
        ModbusRequest::GetCommEventCounter => {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&inner.comm_event_count.to_be_bytes());
        }
        ModbusRequest::GetCommEventLog => {
            // status + event count + message count; no event bytes by default.
            out.push(6);
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&inner.comm_event_count.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes());
        }
        ModbusRequest::WriteMultipleCoils {
            address,
            quantity,
            values,
        } => {
            ensure_span(*address, *quantity as usize)?;
            for offset in 0..*quantity {
                let bit = (values[offset as usize / 8] >> (offset % 8)) & 1 != 0;
                inner.coils.insert(address + offset, bit);
            }
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&quantity.to_be_bytes());
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            write_u16_range(&mut inner.holding_registers, *address, values)?;
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&(values.len() as u16).to_be_bytes());
        }
        ModbusRequest::ReportServerId => {
            let id = b"TauTerm";
            out.push((id.len() + 2) as u8);
            out.push(0x01);
            out.push(0xFF);
            out.extend_from_slice(id);
        }
        ModbusRequest::ReadFileRecord { records } => {
            encode_file_read_response(&mut out, &inner.file_records, records)?
        }
        ModbusRequest::WriteFileRecord { records } => {
            validate_file_writes(records)?;
            let mut response = vec![function];
            encode_file_write_echo(&mut response, records)?;
            apply_file_writes(&mut inner.file_records, records);
            return Ok(response);
        }
        ModbusRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => {
            let current = *inner
                .holding_registers
                .get(address)
                .ok_or(EX_ILLEGAL_DATA_ADDRESS)?;
            let value = (current & *and_mask) | (*or_mask & !*and_mask);
            inner.holding_registers.insert(*address, value);
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&and_mask.to_be_bytes());
            out.extend_from_slice(&or_mask.to_be_bytes());
        }
        ModbusRequest::ReadWriteMultipleRegisters {
            read_address,
            read_quantity,
            write_address,
            values,
        } => {
            validate_read_after_write(
                &inner.holding_registers,
                *read_address,
                *read_quantity,
                *write_address,
                values.len(),
            )?;
            write_u16_range(&mut inner.holding_registers, *write_address, values)?;
            let read = read_u16_range(&inner.holding_registers, *read_address, *read_quantity)?;
            out.push((read.len() * 2) as u8);
            for value in read {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        ModbusRequest::ReadFifoQueue { address } => {
            let values = inner.fifo.get(address).ok_or(EX_ILLEGAL_DATA_ADDRESS)?;
            if values.len() > 31 {
                return Err(EX_ILLEGAL_DATA_VALUE);
            }
            let byte_count = 2 + values.len() * 2;
            out.extend_from_slice(&(byte_count as u16).to_be_bytes());
            out.extend_from_slice(&(values.len() as u16).to_be_bytes());
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        ModbusRequest::Mei { mei_type, data } => {
            if *mei_type != 0x0E {
                return Err(EX_ILLEGAL_FUNCTION);
            }
            if data.len() < 2 {
                return Err(EX_ILLEGAL_DATA_VALUE);
            }
            let read_code = data[0];
            let start = data[1];
            if !(1..=4).contains(&read_code) {
                return Err(EX_ILLEGAL_DATA_VALUE);
            }
            let objects: Vec<_> = inner.device_objects.range(start..).take(16).collect();
            out.extend_from_slice(&[0x0E, read_code, 0x01, 0x00, 0x00, objects.len() as u8]);
            for (id, value) in objects {
                out.push(*id);
                out.push(value.len().min(255) as u8);
                out.extend_from_slice(&value.as_bytes()[..value.len().min(255)]);
            }
        }
        ModbusRequest::Raw { .. } => return Err(EX_ILLEGAL_FUNCTION),
    }
    Ok(out)
}

fn counts_comm_event(request: &ModbusRequest) -> bool {
    !matches!(
        request,
        ModbusRequest::GetCommEventCounter
            | ModbusRequest::Diagnostics {
                sub_function: 0x000A,
                ..
            }
    )
}

fn ensure_span(address: u16, len: usize) -> Result<(), u8> {
    if len == 0 {
        return Err(EX_ILLEGAL_DATA_VALUE);
    }
    let last = len - 1;
    if last > u16::MAX as usize || address.checked_add(last as u16).is_none() {
        return Err(EX_ILLEGAL_DATA_ADDRESS);
    }
    Ok(())
}

fn read_bool_range(map: &HashMap<u16, bool>, address: u16, quantity: u16) -> Result<Vec<bool>, u8> {
    ensure_span(address, quantity as usize)?;
    (0..quantity)
        .map(|offset| map.get(&(address + offset)).copied().ok_or(EX_ILLEGAL_DATA_ADDRESS))
        .collect()
}

fn read_u16_range(map: &HashMap<u16, u16>, address: u16, quantity: u16) -> Result<Vec<u16>, u8> {
    ensure_span(address, quantity as usize)?;
    (0..quantity)
        .map(|offset| map.get(&(address + offset)).copied().ok_or(EX_ILLEGAL_DATA_ADDRESS))
        .collect()
}

fn write_u16_range(map: &mut HashMap<u16, u16>, address: u16, values: &[u16]) -> Result<(), u8> {
    ensure_span(address, values.len())?;
    for (index, value) in values.iter().enumerate() {
        map.insert(address + index as u16, *value);
    }
    Ok(())
}

fn validate_read_after_write(
    map: &HashMap<u16, u16>,
    read_address: u16,
    read_quantity: u16,
    write_address: u16,
    write_len: usize,
) -> Result<(), u8> {
    ensure_span(read_address, read_quantity as usize)?;
    ensure_span(write_address, write_len)?;
    let write_end = write_address + (write_len - 1) as u16;
    for offset in 0..read_quantity {
        let target = read_address + offset;
        if !map.contains_key(&target) && !(write_address..=write_end).contains(&target) {
            return Err(EX_ILLEGAL_DATA_ADDRESS);
        }
    }
    Ok(())
}

fn pack_bits(values: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; values.len().div_ceil(8)];
    for (i, value) in values.iter().enumerate() {
        if *value {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

fn sorted_bool(map: &HashMap<u16, bool>) -> Vec<(u16, bool)> {
    let mut v: Vec<_> = map.iter().map(|(a, v)| (*a, *v)).collect();
    v.sort_unstable_by_key(|x| x.0);
    v
}

fn sorted_u16(map: &HashMap<u16, u16>) -> Vec<(u16, u16)> {
    let mut v: Vec<_> = map.iter().map(|(a, v)| (*a, *v)).collect();
    v.sort_unstable_by_key(|x| x.0);
    v
}

fn encode_file_read_response(
    out: &mut Vec<u8>,
    records: &HashMap<(u16, u16), u16>,
    requests: &[FileRecordRead],
) -> Result<(), u8> {
    let mut payload = Vec::new();
    for request in requests {
        let mut values = Vec::new();
        for offset in 0..request.record_length {
            let record = request
                .record_number
                .checked_add(offset)
                .ok_or(EX_ILLEGAL_DATA_ADDRESS)?;
            values.push(
                *records
                    .get(&(request.file_number, record))
                    .ok_or(EX_ILLEGAL_DATA_ADDRESS)?,
            );
        }
        payload.push((1 + values.len() * 2) as u8);
        payload.push(0x06);
        for value in values {
            payload.extend_from_slice(&value.to_be_bytes());
        }
    }
    if payload.len() > 255 {
        return Err(EX_ILLEGAL_DATA_VALUE);
    }
    out.push(payload.len() as u8);
    out.extend_from_slice(&payload);
    Ok(())
}

fn validate_file_writes(requests: &[FileRecordWrite]) -> Result<(), u8> {
    for request in requests {
        ensure_span(request.record_number, request.values.len())?;
    }
    let mut response = vec![0x15];
    encode_file_write_echo(&mut response, requests)?;
    Ok(())
}

fn apply_file_writes(records: &mut HashMap<(u16, u16), u16>, requests: &[FileRecordWrite]) {
    for request in requests {
        for (offset, value) in request.values.iter().enumerate() {
            records.insert((request.file_number, request.record_number + offset as u16), *value);
        }
    }
}

fn encode_file_write_echo(out: &mut Vec<u8>, requests: &[FileRecordWrite]) -> Result<(), u8> {
    let mut payload = Vec::new();
    for request in requests {
        payload.push(0x06);
        payload.extend_from_slice(&request.file_number.to_be_bytes());
        payload.extend_from_slice(&request.record_number.to_be_bytes());
        payload.extend_from_slice(&(request.values.len() as u16).to_be_bytes());
        for value in &request.values {
            payload.extend_from_slice(&value.to_be_bytes());
        }
    }
    if payload.len() > 255 {
        return Err(EX_ILLEGAL_DATA_VALUE);
    }
    out.push(payload.len() as u8);
    out.extend_from_slice(&payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_read_write_applies_write_first() {
        let model = ModbusDataModel::default();
        model.set_holding_register(0, 1);
        let pdu = model
            .execute(&ModbusRequest::ReadWriteMultipleRegisters {
                read_address: 0,
                read_quantity: 1,
                write_address: 0,
                values: vec![9],
            })
            .unwrap();
        assert_eq!(pdu, vec![0x17, 2, 0, 9]);
    }

    #[test]
    fn failed_multi_register_write_is_atomic() {
        let model = ModbusDataModel::default();
        model.set_holding_register(u16::MAX, 7);
        let result = model.execute(&ModbusRequest::WriteMultipleRegisters {
            address: u16::MAX,
            values: vec![1, 2],
        });
        assert_eq!(result, Err(EX_ILLEGAL_DATA_ADDRESS));
        assert_eq!(model.snapshot().holding_registers, vec![(u16::MAX, 7)]);
    }

    #[test]
    fn failed_read_write_does_not_commit_write_side() {
        let model = ModbusDataModel::default();
        model.set_holding_register(0, 1);
        let result = model.execute(&ModbusRequest::ReadWriteMultipleRegisters {
            read_address: 100,
            read_quantity: 1,
            write_address: 0,
            values: vec![9],
        });
        assert_eq!(result, Err(EX_ILLEGAL_DATA_ADDRESS));
        assert_eq!(model.snapshot().holding_registers, vec![(0, 1)]);
    }

    #[test]
    fn event_counter_counts_success_and_clear_resets_without_self_increment() {
        let model = ModbusDataModel::default();
        model.set_holding_register(0, 1);
        model
            .execute(&ModbusRequest::ReadRegisters {
                function: 0x03,
                address: 0,
                quantity: 1,
            })
            .unwrap();
        let before = model.execute(&ModbusRequest::GetCommEventCounter).unwrap();
        assert_eq!(before, vec![0x0B, 0, 0, 0, 1]);

        model
            .execute(&ModbusRequest::Diagnostics {
                sub_function: 0x000A,
                data: Vec::new(),
            })
            .unwrap();
        let after = model.execute(&ModbusRequest::GetCommEventCounter).unwrap();
        assert_eq!(after, vec![0x0B, 0, 0, 0, 0]);
    }

    #[test]
    fn unsupported_diagnostic_subfunction_is_not_silently_echoed() {
        let model = ModbusDataModel::default();
        assert_eq!(
            model.execute(&ModbusRequest::Diagnostics {
                sub_function: 0x1234,
                data: vec![0, 1],
            }),
            Err(EX_ILLEGAL_DATA_VALUE)
        );
    }
}
