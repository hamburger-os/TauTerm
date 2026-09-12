use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Bool,
    UInt16,
    Int16,
    UInt32,
    Int32,
    Float32,
    UInt64,
    Int64,
    Float64,
    Hex,
    Binary,
    Ascii,
    Utf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ByteOrder {
    ABCD,
    BADC,
    CDAB,
    DCBA,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueFormat {
    pub value_type: ValueType,
    #[serde(default = "default_order")]
    pub byte_order: ByteOrder,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub bit: Option<u8>,
}
fn default_order() -> ByteOrder {
    ByteOrder::ABCD
}
fn default_scale() -> f64 {
    1.0
}

pub fn decode_register_bytes(
    bytes: &[u8],
    format: &ValueFormat,
) -> Result<serde_json::Value, String> {
    if let Some(bit) = format.bit {
        if bit > 15 || bytes.len() < 2 {
            return Err("bit extraction requires one register and bit 0..15".into());
        }
        let value = u16::from_be_bytes([bytes[0], bytes[1]]);
        return Ok(serde_json::json!(((value >> bit) & 1) != 0));
    }
    use ValueType::*;
    match format.value_type {
        Bool => {
            if bytes.len() < 2 {
                return Err("bool register requires 2 bytes".into());
            }
            Ok(serde_json::json!(
                u16::from_be_bytes([bytes[0], bytes[1]]) != 0
            ))
        }
        UInt16 => scaled(u16::from_be_bytes(take2(bytes)?) as f64, format),
        Int16 => scaled(i16::from_be_bytes(take2(bytes)?) as f64, format),
        UInt32 => scaled(
            u32::from_be_bytes(order4(bytes, format.byte_order)?) as f64,
            format,
        ),
        Int32 => scaled(
            i32::from_be_bytes(order4(bytes, format.byte_order)?) as f64,
            format,
        ),
        Float32 => scaled(
            f32::from_be_bytes(order4(bytes, format.byte_order)?) as f64,
            format,
        ),
        UInt64 => scaled(
            u64::from_be_bytes(order8(bytes, format.byte_order)?) as f64,
            format,
        ),
        Int64 => scaled(
            i64::from_be_bytes(order8(bytes, format.byte_order)?) as f64,
            format,
        ),
        Float64 => scaled(
            f64::from_be_bytes(order8(bytes, format.byte_order)?),
            format,
        ),
        Hex => Ok(serde_json::Value::String(
            bytes
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
        )),
        Binary => Ok(serde_json::Value::String(
            bytes
                .iter()
                .map(|b| format!("{b:08b}"))
                .collect::<Vec<_>>()
                .join(" "),
        )),
        Ascii => Ok(serde_json::Value::String(
            bytes
                .iter()
                .map(|b| if b.is_ascii() { *b as char } else { '.' })
                .collect(),
        )),
        Utf8 => Ok(serde_json::Value::String(
            String::from_utf8_lossy(bytes).into_owned(),
        )),
    }
}
fn scaled(value: f64, format: &ValueFormat) -> Result<serde_json::Value, String> {
    let value = value * format.scale + format.offset;
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or("non-finite scaled value".into())
}
fn take2(bytes: &[u8]) -> Result<[u8; 2], String> {
    bytes
        .get(..2)
        .and_then(|b| b.try_into().ok())
        .ok_or("requires 2 bytes".into())
}
fn order4(bytes: &[u8], order: ByteOrder) -> Result<[u8; 4], String> {
    let b: [u8; 4] = bytes
        .get(..4)
        .and_then(|v| v.try_into().ok())
        .ok_or("requires 4 bytes")?;
    Ok(match order {
        ByteOrder::ABCD => b,
        ByteOrder::BADC => [b[1], b[0], b[3], b[2]],
        ByteOrder::CDAB => [b[2], b[3], b[0], b[1]],
        ByteOrder::DCBA => [b[3], b[2], b[1], b[0]],
    })
}
fn order8(bytes: &[u8], order: ByteOrder) -> Result<[u8; 8], String> {
    let b: [u8; 8] = bytes
        .get(..8)
        .and_then(|v| v.try_into().ok())
        .ok_or("requires 8 bytes")?;
    Ok(match order {
        ByteOrder::ABCD => b,
        ByteOrder::BADC => [b[1], b[0], b[3], b[2], b[5], b[4], b[7], b[6]],
        ByteOrder::CDAB => [b[6], b[7], b[4], b[5], b[2], b[3], b[0], b[1]],
        ByteOrder::DCBA => [b[7], b[6], b[5], b[4], b[3], b[2], b[1], b[0]],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_orders_are_explicit_application_conventions() {
        let f = ValueFormat {
            value_type: ValueType::UInt32,
            byte_order: ByteOrder::CDAB,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
            bit: None,
        };
        assert_eq!(
            decode_register_bytes(&[0x33, 0x44, 0x11, 0x22], &f).unwrap(),
            serde_json::json!(0x11223344u32 as f64)
        );
    }
}
