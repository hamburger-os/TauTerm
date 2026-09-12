use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::plugins::modbus::codec::{
    BitReadArea, FileRecordRead, FileRecordWrite, ModbusRequest, RegisterReadArea,
};

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
struct AddressBlock<T> {
    values: BTreeMap<u16, T>,
}

impl<T: Copy> AddressBlock<T> {
    fn set(&mut self, address: u16, value: T) {
        self.values.insert(address, value);
    }

    fn get(&self, address: u16) -> Result<T, u8> {
        self.values
            .get(&address)
            .copied()
            .ok_or(EX_ILLEGAL_DATA_ADDRESS)
    }

    fn read(&self, address: u16, quantity: u16) -> Result<Vec<T>, u8> {
        ensure_span(address, quantity as usize)?;
        (0..quantity)
            .map(|offset| self.get(address + offset))
            .collect()
    }

    fn write(&mut self, address: u16, values: &[T]) -> Result<(), u8> {
        ensure_span(address, values.len())?;
        for (index, value) in values.iter().enumerate() {
            self.set(address + index as u16, *value);
        }
        Ok(())
    }

    fn snapshot(&self) -> Vec<(u16, T)> {
        self.values
            .iter()
            .map(|(address, value)| (*address, *value))
            .collect()
    }

    fn can_read_after_write(
        &self,
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
            if !self.values.contains_key(&target) && !(write_address..=write_end).contains(&target)
            {
                return Err(EX_ILLEGAL_DATA_ADDRESS);
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct AddressSpace {
    coils: AddressBlock<bool>,
    discrete_inputs: AddressBlock<bool>,
    holding_registers: AddressBlock<u16>,
    input_registers: AddressBlock<u16>,
}

#[derive(Default)]
struct ModelInner {
    address_space: AddressSpace,
    file_records: HashMap<(u16, u16), u16>,
    fifo: HashMap<u16, Vec<u16>>,
    device_objects: BTreeMap<u8, String>,
    exception_status: u8,
    comm_event_count: u16,
    message_count: u16,
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
            .address_space
            .coils
            .set(address, value);
    }

    pub fn set_discrete_input(&self, address: u16, value: bool) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .address_space
            .discrete_inputs
            .set(address, value);
    }

    pub fn set_holding_register(&self, address: u16, value: u16) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .address_space
            .holding_registers
            .set(address, value);
    }

    pub fn set_input_register(&self, address: u16, value: u16) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .address_space
            .input_registers
            .set(address, value);
    }

    pub fn snapshot(&self) -> DataModelSnapshot {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        DataModelSnapshot {
            coils: inner.address_space.coils.snapshot(),
            discrete_inputs: inner.address_space.discrete_inputs.snapshot(),
            holding_registers: inner.address_space.holding_registers.snapshot(),
            input_registers: inner.address_space.input_registers.snapshot(),
        }
    }

    /// Execute one decoded request atomically and return a complete response PDU.
    ///
    /// Every multi-address mutation validates its complete target before the first write.
    /// Failed requests therefore never leave a partially updated simulator state.
    pub fn execute(&self, request: &ModbusRequest) -> Result<Vec<u8>, u8> {
        let mut inner = self.inner.write().map_err(|_| EX_SERVER_DEVICE_FAILURE)?;
        let clear_counters = matches!(
            request,
            ModbusRequest::Diagnostics {
                sub_function: 0x000A,
                data,
            } if data.as_slice() == [0x00, 0x00]
        );
        let result = execute_request(&mut inner, request);
        if !clear_counters {
            inner.message_count = inner.message_count.wrapping_add(1);
        }
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
            area,
            address,
            quantity,
        } => {
            let values = match area {
                BitReadArea::Coils => inner.address_space.coils.read(*address, *quantity)?,
                BitReadArea::DiscreteInputs => inner
                    .address_space
                    .discrete_inputs
                    .read(*address, *quantity)?,
            };
            let packed = pack_bits(&values);
            out.push(packed.len() as u8);
            out.extend_from_slice(&packed);
        }
        ModbusRequest::ReadRegisters {
            area,
            address,
            quantity,
        } => {
            let values = match area {
                RegisterReadArea::HoldingRegisters => inner
                    .address_space
                    .holding_registers
                    .read(*address, *quantity)?,
                RegisterReadArea::InputRegisters => inner
                    .address_space
                    .input_registers
                    .read(*address, *quantity)?,
            };
            out.push((values.len() * 2) as u8);
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        ModbusRequest::WriteSingleCoil { address, value } => {
            inner.address_space.coils.set(*address, *value);
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&(if *value { 0xFF00u16 } else { 0x0000u16 }).to_be_bytes());
        }
        ModbusRequest::WriteSingleRegister { address, value } => {
            inner.address_space.holding_registers.set(*address, *value);
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
        }
        ModbusRequest::ReadExceptionStatus => out.push(inner.exception_status),
        ModbusRequest::Diagnostics { sub_function, data } => {
            out.extend_from_slice(&sub_function.to_be_bytes());
            match *sub_function {
                0x0000 => out.extend_from_slice(data),
                0x000A if data.as_slice() == [0x00, 0x00] => {
                    inner.comm_event_count = 0;
                    inner.message_count = 0;
                    inner.exception_status = 0;
                    out.extend_from_slice(data);
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
            out.push(6);
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&inner.comm_event_count.to_be_bytes());
            out.extend_from_slice(&inner.message_count.to_be_bytes());
        }
        ModbusRequest::WriteMultipleCoils { address, values } => {
            inner.address_space.coils.write(*address, values)?;
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&(values.len() as u16).to_be_bytes());
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            inner
                .address_space
                .holding_registers
                .write(*address, values)?;
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
            let current = inner.address_space.holding_registers.get(*address)?;
            let value = (current & *and_mask) | (*or_mask & !*and_mask);
            inner.address_space.holding_registers.set(*address, value);
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
            inner.address_space.holding_registers.can_read_after_write(
                *read_address,
                *read_quantity,
                *write_address,
                values.len(),
            )?;
            inner
                .address_space
                .holding_registers
                .write(*write_address, values)?;
            let read = inner
                .address_space
                .holding_registers
                .read(*read_address, *read_quantity)?;
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
            encode_mei_response(&mut out, &inner.device_objects, *mei_type, data)?;
        }
        ModbusRequest::Raw { .. } => return Err(EX_ILLEGAL_FUNCTION),
    }
    Ok(out)
}

fn encode_mei_response(
    out: &mut Vec<u8>,
    objects: &BTreeMap<u8, String>,
    mei_type: u8,
    data: &[u8],
) -> Result<(), u8> {
    if mei_type != 0x0E {
        return Err(EX_ILLEGAL_FUNCTION);
    }
    if data.len() != 2 || !(1..=4).contains(&data[0]) {
        return Err(EX_ILLEGAL_DATA_VALUE);
    }
    let read_code = data[0];
    let start = data[1];
    let selected: Vec<_> = if read_code == 0x04 {
        vec![objects
            .get_key_value(&start)
            .ok_or(EX_ILLEGAL_DATA_ADDRESS)?]
    } else {
        let first = if objects.contains_key(&start) {
            start
        } else {
            0
        };
        objects.range(first..).take(17).collect()
    };
    let more_follows = selected.len() > 16;
    let visible = selected.iter().take(16).copied().collect::<Vec<_>>();
    let next_object = if more_follows { *selected[16].0 } else { 0 };
    out.extend_from_slice(&[
        0x0E,
        read_code,
        0x81,
        if more_follows { 0xFF } else { 0x00 },
        next_object,
        visible.len() as u8,
    ]);
    for (id, value) in visible {
        let bytes = value.as_bytes();
        let len = bytes.len().min(255);
        out.push(*id);
        out.push(len as u8);
        out.extend_from_slice(&bytes[..len]);
    }
    Ok(())
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

fn pack_bits(values: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; values.len().div_ceil(8)];
    for (index, value) in values.iter().enumerate() {
        if *value {
            out[index / 8] |= 1 << (index % 8);
        }
    }
    out
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
            records.insert(
                (request.file_number, request.record_number + offset as u16),
                *value,
            );
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
    fn address_block_validates_before_multi_write() {
        let mut block = AddressBlock::default();
        block.set(u16::MAX, 7u16);
        assert_eq!(block.write(u16::MAX, &[1, 2]), Err(EX_ILLEGAL_DATA_ADDRESS));
        assert_eq!(block.snapshot(), vec![(u16::MAX, 7)]);
    }

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
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            })
            .unwrap();
        let before = model.execute(&ModbusRequest::GetCommEventCounter).unwrap();
        assert_eq!(before, vec![0x0B, 0, 0, 0, 1]);

        model
            .execute(&ModbusRequest::Diagnostics {
                sub_function: 0x000A,
                data: vec![0x00, 0x00],
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

    #[test]
    fn clear_counters_requires_and_echoes_standard_zero_data() {
        let model = ModbusDataModel::default();
        assert_eq!(
            model.execute(&ModbusRequest::Diagnostics {
                sub_function: 0x000A,
                data: Vec::new(),
            }),
            Err(EX_ILLEGAL_DATA_VALUE)
        );
        assert_eq!(
            model
                .execute(&ModbusRequest::Diagnostics {
                    sub_function: 0x000A,
                    data: vec![0x00, 0x00],
                })
                .unwrap(),
            vec![0x08, 0x00, 0x0A, 0x00, 0x00]
        );
    }

    #[test]
    fn event_log_uses_distinct_message_and_event_counters() {
        let model = ModbusDataModel::default();
        model.set_holding_register(0, 1);
        model
            .execute(&ModbusRequest::ReadRegisters {
                area: RegisterReadArea::HoldingRegisters,
                address: 0,
                quantity: 1,
            })
            .unwrap();
        let _ = model.execute(&ModbusRequest::GetCommEventCounter).unwrap();
        let log = model.execute(&ModbusRequest::GetCommEventLog).unwrap();
        assert_eq!(&log[4..6], &[0x00, 0x01]);
        assert_eq!(&log[6..8], &[0x00, 0x02]);
    }

    #[test]
    fn device_identification_unknown_stream_object_restarts_at_zero() {
        let model = ModbusDataModel::default();
        let pdu = model
            .execute(&ModbusRequest::Mei {
                mei_type: 0x0E,
                data: vec![0x01, 0x7F],
            })
            .unwrap();
        assert_eq!(pdu[7], 0x00);
    }

    #[test]
    fn device_identification_individual_access_returns_only_requested_object() {
        let model = ModbusDataModel::default();
        let pdu = model
            .execute(&ModbusRequest::Mei {
                mei_type: 0x0E,
                data: vec![0x04, 0x01],
            })
            .unwrap();
        assert_eq!(pdu[1..7], [0x0E, 0x04, 0x81, 0x00, 0x00, 0x01]);
        assert_eq!(pdu[7], 0x01);
    }
}
