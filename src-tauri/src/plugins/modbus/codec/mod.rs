pub mod ascii;
pub mod pdu;
pub mod rtu;
pub mod tcp;

pub use pdu::{
    decode_request, encode_request, validate_response, BitReadArea, FileRecordRead,
    FileRecordWrite, ModbusRequest, RegisterReadArea,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AduMode {
    Rtu,
    Ascii,
    Tcp,
}

pub fn encode_adu(
    mode: AduMode,
    unit_id: u8,
    transaction_id: u16,
    pdu: &[u8],
) -> Result<Vec<u8>, String> {
    match mode {
        AduMode::Rtu => rtu::encode(unit_id, pdu),
        AduMode::Ascii => ascii::encode(unit_id, pdu),
        AduMode::Tcp => tcp::encode(transaction_id, unit_id, pdu),
    }
}
