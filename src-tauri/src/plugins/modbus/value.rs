use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    Big,
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordOrder {
    Normal,
    Reverse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueFormat {
    pub value_type: ValueType,
    #[serde(default = "default_byte_order")]
    pub byte_order: ByteOrder,
    #[serde(default = "default_word_order")]
    pub word_order: WordOrder,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub bit: Option<u8>,
}

fn default_byte_order() -> ByteOrder {
    ByteOrder::Big
}

fn default_word_order() -> WordOrder {
    WordOrder::Normal
}

fn default_scale() -> f64 {
    1.0
}

pub fn required_registers(format: &ValueFormat) -> Option<u16> {
    if format.bit.is_some() {
        return Some(1);
    }
    use ValueType::*;
    match format.value_type {
        Bool | UInt16 | Int16 => Some(1),
        UInt32 | Int32 | Float32 => Some(2),
        UInt64 | Int64 | Float64 => Some(4),
        Hex | Binary | Ascii | Utf8 => None,
    }
}

pub fn decode_register_bytes(
    bytes: &[u8],
    format: &ValueFormat,
) -> Result<serde_json::Value, String> {
    if let Some(bit) = format.bit {
        if bit > 15 || bytes.len() < 2 {
            return Err("bit extraction requires one register and bit 0..15".into());
        }
        let ordered = ordered_register_bytes(&bytes[..2], format.byte_order, WordOrder::Normal)?;
        let value = u16::from_be_bytes([ordered[0], ordered[1]]);
        return Ok(serde_json::json!(((value >> bit) & 1) != 0));
    }

    use ValueType::*;
    match format.value_type {
        Bool => {
            let ordered =
                ordered_register_bytes(take(bytes, 2)?, format.byte_order, WordOrder::Normal)?;
            Ok(serde_json::json!(
                u16::from_be_bytes([ordered[0], ordered[1]]) != 0
            ))
        }
        UInt16 => {
            let ordered =
                ordered_register_bytes(take(bytes, 2)?, format.byte_order, WordOrder::Normal)?;
            scaled(u16::from_be_bytes([ordered[0], ordered[1]]) as f64, format)
        }
        Int16 => {
            let ordered =
                ordered_register_bytes(take(bytes, 2)?, format.byte_order, WordOrder::Normal)?;
            scaled(i16::from_be_bytes([ordered[0], ordered[1]]) as f64, format)
        }
        UInt32 => {
            let ordered =
                ordered_register_bytes(take(bytes, 4)?, format.byte_order, format.word_order)?;
            scaled(
                u32::from_be_bytes(ordered.try_into().expect("length checked")) as f64,
                format,
            )
        }
        Int32 => {
            let ordered =
                ordered_register_bytes(take(bytes, 4)?, format.byte_order, format.word_order)?;
            scaled(
                i32::from_be_bytes(ordered.try_into().expect("length checked")) as f64,
                format,
            )
        }
        Float32 => {
            let ordered =
                ordered_register_bytes(take(bytes, 4)?, format.byte_order, format.word_order)?;
            scaled(
                f32::from_be_bytes(ordered.try_into().expect("length checked")) as f64,
                format,
            )
        }
        UInt64 => {
            require_identity_scaling(format, "uint64")?;
            let ordered =
                ordered_register_bytes(take(bytes, 8)?, format.byte_order, format.word_order)?;
            Ok(serde_json::Value::String(
                u64::from_be_bytes(ordered.try_into().expect("length checked")).to_string(),
            ))
        }
        Int64 => {
            require_identity_scaling(format, "int64")?;
            let ordered =
                ordered_register_bytes(take(bytes, 8)?, format.byte_order, format.word_order)?;
            Ok(serde_json::Value::String(
                i64::from_be_bytes(ordered.try_into().expect("length checked")).to_string(),
            ))
        }
        Float64 => {
            let ordered =
                ordered_register_bytes(take(bytes, 8)?, format.byte_order, format.word_order)?;
            scaled(
                f64::from_be_bytes(ordered.try_into().expect("length checked")),
                format,
            )
        }
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

fn require_identity_scaling(format: &ValueFormat, label: &str) -> Result<(), String> {
    if format.scale == 1.0 && format.offset == 0.0 {
        Ok(())
    } else {
        Err(format!(
            "{label} uses exact decimal representation and requires scale=1, offset=0"
        ))
    }
}

fn take(bytes: &[u8], len: usize) -> Result<&[u8], String> {
    bytes
        .get(..len)
        .ok_or_else(|| format!("requires {len} bytes"))
}

fn ordered_register_bytes(
    bytes: &[u8],
    byte_order: ByteOrder,
    word_order: WordOrder,
) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err("register value requires an even, non-zero byte count".into());
    }
    let mut words: Vec<[u8; 2]> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| match byte_order {
            ByteOrder::Big => [chunk[0], chunk[1]],
            ByteOrder::Little => [chunk[1], chunk[0]],
        })
        .collect();
    if word_order == WordOrder::Reverse {
        words.reverse();
    }
    Ok(words.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(value_type: ValueType) -> ValueFormat {
        ValueFormat {
            value_type,
            byte_order: ByteOrder::Big,
            word_order: WordOrder::Normal,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
            bit: None,
        }
    }

    #[test]
    fn byte_and_word_order_are_independent() {
        let mut f = format(ValueType::UInt32);
        f.byte_order = ByteOrder::Little;
        f.word_order = WordOrder::Reverse;
        assert_eq!(
            decode_register_bytes(&[0x44, 0x33, 0x22, 0x11], &f).unwrap(),
            serde_json::json!(0x11223344u32 as f64)
        );
    }

    #[test]
    fn exact_u64_does_not_cross_f64_boundary() {
        let f = format(ValueType::UInt64);
        assert_eq!(
            decode_register_bytes(&u64::MAX.to_be_bytes(), &f).unwrap(),
            serde_json::Value::String(u64::MAX.to_string())
        );
    }

    #[test]
    fn exact_integer_scaling_is_rejected() {
        let mut f = format(ValueType::Int64);
        f.scale = 0.1;
        assert!(decode_register_bytes(&1i64.to_be_bytes(), &f).is_err());
    }

    #[test]
    fn scalar_widths_are_explicit() {
        assert_eq!(required_registers(&format(ValueType::UInt16)), Some(1));
        assert_eq!(required_registers(&format(ValueType::Float32)), Some(2));
        assert_eq!(required_registers(&format(ValueType::Float64)), Some(4));
        assert_eq!(required_registers(&format(ValueType::Hex)), None);
    }
}
