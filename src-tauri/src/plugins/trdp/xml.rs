use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpXmlElement {
    pub name: String,
    pub data_type: String,
    pub type_id: u32,
    pub array_size: u32,
    pub dynamic: bool,
    pub unit: Option<String>,
    pub scale: Option<f64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpXmlDataset {
    pub id: u32,
    pub name: String,
    pub elements: Vec<TrdpXmlElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpXmlTelegram {
    pub name: String,
    pub traffic_kind: String,
    pub com_id: u32,
    pub dataset_id: u32,
    pub cycle_us: Option<u32>,
    pub timeout_us: Option<u32>,
    pub timeout_behavior: Option<String>,
    pub sources: Vec<String>,
    pub destinations: Vec<String>,
    pub sdt_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpXmlImport {
    pub path: String,
    pub datasets: Vec<TrdpXmlDataset>,
    pub telegrams: Vec<TrdpXmlTelegram>,
    pub pd_port: u16,
    pub md_udp_port: u16,
    pub md_tcp_port: u16,
    pub sdt_detected: bool,
    pub warnings: Vec<String>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?i)\b{}\s*=\s*[\"']([^\"']*)[\"']"#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(tag)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn attr_u32(tag: &str, name: &str) -> Option<u32> {
    attr(tag, name)?.trim().parse().ok()
}

fn type_id(value: &str) -> Option<u32> {
    if let Ok(id) = value.trim().parse::<u32>() {
        return Some(id);
    }
    match value.trim().to_ascii_uppercase().as_str() {
        "BOOL8" | "BITSET8" | "ANTIVALENT8" => Some(1),
        "CHAR8" => Some(2),
        "UTF16" => Some(3),
        "INT8" => Some(4),
        "INT16" => Some(5),
        "INT32" => Some(6),
        "INT64" => Some(7),
        "UINT8" => Some(8),
        "UINT16" => Some(9),
        "UINT32" => Some(10),
        "UINT64" => Some(11),
        "REAL32" => Some(12),
        "REAL64" => Some(13),
        "TIMEDATE32" => Some(14),
        "TIMEDATE48" => Some(15),
        "TIMEDATE64" => Some(16),
        _ => None,
    }
}

fn type_name(id: u32, original: &str) -> String {
    match id {
        1 => "BITSET8",
        2 => "CHAR8",
        3 => "UTF16",
        4 => "INT8",
        5 => "INT16",
        6 => "INT32",
        7 => "INT64",
        8 => "UINT8",
        9 => "UINT16",
        10 => "UINT32",
        11 => "UINT64",
        12 => "REAL32",
        13 => "REAL64",
        14 => "TIMEDATE32",
        15 => "TIMEDATE48",
        16 => "TIMEDATE64",
        nested if nested >= 1000 => return format!("Dataset {nested}"),
        _ => original,
    }
    .to_string()
}

fn parse_xml(path: &str) -> Result<TrdpXmlImport, String> {
    let xml = fs::read_to_string(path).map_err(|error| format!("读取 TRDP XML 失败: {error}"))?;
    // Require whitespace after element names. `\b` is not sufficient because
    // the hyphen in <data-set-list>/<telegram-list> is itself a word boundary.
    let dataset_re = Regex::new(r#"(?is)<data-set\s+([^>]*)>(.*?)</data-set>"#)
        .map_err(|error| error.to_string())?;
    let element_re =
        Regex::new(r#"(?is)<element\s+([^>]*)/?>"#).map_err(|error| error.to_string())?;
    let telegram_re = Regex::new(r#"(?is)<telegram\s+([^>]*)>(.*?)</telegram>"#)
        .map_err(|error| error.to_string())?;
    let pd_re =
        Regex::new(r#"(?is)<pd-parameter(?:\s+([^>]*))?/?>"#).map_err(|error| error.to_string())?;
    let md_re =
        Regex::new(r#"(?is)<md-parameter(?:\s+([^>]*))?/?>"#).map_err(|error| error.to_string())?;
    let source_re = Regex::new(r#"(?is)<source\s+([^>]*)"#).map_err(|error| error.to_string())?;
    let destination_re =
        Regex::new(r#"(?is)<destination\s+([^>]*)"#).map_err(|error| error.to_string())?;
    let pd_config_re =
        Regex::new(r#"(?is)<pd-com-parameter\s+([^>]*)/?>"#).map_err(|error| error.to_string())?;
    let md_config_re =
        Regex::new(r#"(?is)<md-com-parameter\s+([^>]*)/?>"#).map_err(|error| error.to_string())?;

    let mut warnings = Vec::new();
    let mut datasets = Vec::new();
    let mut dataset_ids = HashSet::new();
    for capture in dataset_re.captures_iter(&xml) {
        let tag = capture
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let body = capture
            .get(2)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let Some(id) = attr_u32(tag, "id") else {
            warnings.push("忽略缺少数字 id 的 <data-set>".to_string());
            continue;
        };
        if !dataset_ids.insert(id) {
            warnings.push(format!(
                "Dataset {id} 重复定义；保留全部定义供预览，请在使用前修正配置"
            ));
        }
        let name = attr(tag, "name").unwrap_or_else(|| format!("Dataset {id}"));
        let mut elements = Vec::new();
        for element in element_re.captures_iter(body) {
            let attributes = element
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let raw_type = attr(attributes, "type").unwrap_or_else(|| "0".to_string());
            let element_type_id = type_id(&raw_type).unwrap_or(0);
            if element_type_id == 0 {
                warnings.push(format!(
                    "Dataset {id} 字段 {} 使用未知类型 {}",
                    attr(attributes, "name").unwrap_or_else(|| "unnamed".to_string()),
                    raw_type
                ));
            }
            let array_size = attr_u32(attributes, "array-size").unwrap_or(1);
            elements.push(TrdpXmlElement {
                name: attr(attributes, "name").unwrap_or_else(|| "unnamed".to_string()),
                data_type: type_name(element_type_id, &raw_type),
                type_id: element_type_id,
                array_size,
                dynamic: array_size == 0,
                unit: attr(attributes, "unit"),
                scale: attr(attributes, "scale").and_then(|value| value.parse().ok()),
                offset: attr(attributes, "offset").and_then(|value| value.parse().ok()),
            });
        }
        if elements
            .iter()
            .take(elements.len().saturating_sub(1))
            .any(|element| element.dynamic)
        {
            warnings.push(format!(
                "Dataset {id} 在非末尾位置包含动态数组；解码仅在后续字段固定长度时可确定边界"
            ));
        }
        datasets.push(TrdpXmlDataset { id, name, elements });
    }

    let mut telegrams = Vec::new();
    for capture in telegram_re.captures_iter(&xml) {
        let tag = capture
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let body = capture
            .get(2)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let Some(com_id) = attr_u32(tag, "com-id") else {
            warnings.push("忽略缺少 com-id 的 <telegram>".to_string());
            continue;
        };
        let dataset_id = attr_u32(tag, "data-set-id").unwrap_or(0);
        let pd_parameter = pd_re.captures(body);
        let md_parameter = md_re.captures(body);
        let traffic_kind = match (pd_parameter.is_some(), md_parameter.is_some()) {
            (true, false) => "pd",
            (false, true) => "md",
            (true, true) => {
                warnings.push(format!(
                    "ComID {com_id} 同时包含 pd-parameter 与 md-parameter；不会自动生成模板"
                ));
                "ambiguous"
            }
            (false, false) => {
                warnings.push(format!(
                    "ComID {com_id} 未声明 pd-parameter/md-parameter；协议类型标记为 unknown"
                ));
                "unknown"
            }
        };
        let pd_attributes = pd_parameter
            .as_ref()
            .and_then(|value| value.get(1))
            .map(|value| value.as_str())
            .unwrap_or_default();
        let sources = source_re
            .captures_iter(body)
            .filter_map(|value| value.get(1))
            .filter_map(|value| {
                attr(value.as_str(), "uri1").or_else(|| attr(value.as_str(), "uri"))
            })
            .collect();
        let destinations = destination_re
            .captures_iter(body)
            .filter_map(|value| value.get(1))
            .filter_map(|value| attr(value.as_str(), "uri"))
            .collect();
        telegrams.push(TrdpXmlTelegram {
            name: attr(tag, "name").unwrap_or_else(|| format!("ComID {com_id}")),
            traffic_kind: traffic_kind.to_string(),
            com_id,
            dataset_id,
            cycle_us: attr_u32(pd_attributes, "cycle"),
            timeout_us: attr_u32(pd_attributes, "timeout"),
            timeout_behavior: pd_parameter.as_ref().map(|_| {
                attr(pd_attributes, "validity-behavior").unwrap_or_else(|| "zero".to_string())
            }),
            sources,
            destinations,
            sdt_detected: body.to_ascii_lowercase().contains("<sdt-parameter"),
        });
    }

    let known_dataset_ids: HashSet<u32> = datasets.iter().map(|dataset| dataset.id).collect();
    for telegram in &telegrams {
        if telegram.dataset_id != 0 && !known_dataset_ids.contains(&telegram.dataset_id) {
            warnings.push(format!(
                "ComID {} 引用了不存在的 Dataset {}",
                telegram.com_id, telegram.dataset_id
            ));
        }
    }

    let pd_attributes = pd_config_re
        .captures(&xml)
        .and_then(|value| value.get(1))
        .map(|value| value.as_str())
        .unwrap_or_default();
    let md_attributes = md_config_re
        .captures(&xml)
        .and_then(|value| value.get(1))
        .map(|value| value.as_str())
        .unwrap_or_default();
    let pd_port = attr_u32(pd_attributes, "port")
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(17224);
    let md_udp_port = attr_u32(md_attributes, "udp-port")
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(17225);
    let md_tcp_port = attr_u32(md_attributes, "tcp-port")
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(17225);

    let lowercase = xml.to_ascii_lowercase();
    let sdt_detected = lowercase.contains("<sdt-parameter")
        || lowercase.contains("sdtv2")
        || lowercase.contains("sdtv4");
    if sdt_detected {
        warnings.push(
            "检测到 SDT 配置：TauTerm 首版仅展示元数据，不执行 SDTv2/SDTv4 安全验证。".to_string(),
        );
    }

    Ok(TrdpXmlImport {
        path: path.to_string(),
        datasets,
        telegrams,
        pd_port,
        md_udp_port,
        md_tcp_port,
        sdt_detected,
        warnings,
    })
}

pub fn trdp_import_xml(path: String) -> Result<TrdpXmlImport, String> {
    parse_xml(&path)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let compact: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if !compact.len().is_multiple_of(2) {
        return Err("HEX 长度必须为偶数".to_string());
    }
    (0..compact.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&compact[index..index + 2], 16)
                .map_err(|_| format!("无效 HEX: {}", &compact[index..index + 2]))
        })
        .collect()
}

fn primitive_width(type_id: u32) -> Option<usize> {
    match type_id {
        1 | 2 | 4 | 8 => Some(1),
        3 | 5 | 9 => Some(2),
        6 | 10 | 12 | 14 => Some(4),
        15 => Some(6),
        7 | 11 | 13 | 16 => Some(8),
        _ => None,
    }
}

fn decode_primitive(type_id: u32, bytes: &[u8]) -> Value {
    match type_id {
        1 => json!(bytes.first().copied().unwrap_or(0)),
        2 => json!(bytes
            .first()
            .map(|value| char::from(*value).to_string())
            .unwrap_or_default()),
        3 | 9 => json!(u16::from_be_bytes([bytes[0], bytes[1]])),
        4 => json!(bytes.first().copied().unwrap_or(0) as i8),
        5 => json!(i16::from_be_bytes([bytes[0], bytes[1]])),
        6 => json!(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        7 => json!(i64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))),
        8 => json!(bytes.first().copied().unwrap_or(0)),
        10 | 14 => json!(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        11 => json!(u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))),
        16 => json!({
            "seconds": u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            "microseconds": u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
        }),
        12 => json!(f32::from_bits(u32::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3]
        ]))),
        13 => json!(f64::from_bits(u64::from_be_bytes(
            bytes.try_into().unwrap_or([0; 8])
        ))),
        15 => json!({
            "seconds": u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            "ticks": u16::from_be_bytes([bytes[4], bytes[5]])
        }),
        _ => Value::Null,
    }
}

fn fixed_dataset_width(
    dataset_id: u32,
    imported: &TrdpXmlImport,
    visiting: &mut HashSet<u32>,
) -> Option<usize> {
    if !visiting.insert(dataset_id) {
        return None;
    }
    let dataset = imported
        .datasets
        .iter()
        .find(|dataset| dataset.id == dataset_id)?;
    let mut total = 0usize;
    for element in &dataset.elements {
        if element.dynamic {
            visiting.remove(&dataset_id);
            return None;
        }
        let width = if let Some(width) = primitive_width(element.type_id) {
            width
        } else if element.type_id >= 1000 {
            fixed_dataset_width(element.type_id, imported, visiting)?
        } else {
            visiting.remove(&dataset_id);
            return None;
        };
        total = total.checked_add(width.checked_mul(element.array_size as usize)?)?;
    }
    visiting.remove(&dataset_id);
    Some(total)
}

fn decode_dataset_inner(
    dataset_id: u32,
    payload: &[u8],
    imported: &TrdpXmlImport,
    visiting: &mut HashSet<u32>,
) -> Result<(Map<String, Value>, usize), String> {
    if !visiting.insert(dataset_id) {
        return Err(format!("Dataset 嵌套循环: {dataset_id}"));
    }
    let dataset = imported
        .datasets
        .iter()
        .find(|dataset| dataset.id == dataset_id)
        .ok_or_else(|| format!("Dataset {dataset_id} 不存在"))?;
    let mut offset = 0usize;
    let mut fields = Map::new();

    for (index, element) in dataset.elements.iter().enumerate() {
        let remaining_fixed_width =
            dataset.elements[index + 1..]
                .iter()
                .try_fold(0usize, |sum, trailing| {
                    if trailing.dynamic {
                        return None;
                    }
                    let width = if let Some(width) = primitive_width(trailing.type_id) {
                        width
                    } else if trailing.type_id >= 1000 {
                        fixed_dataset_width(trailing.type_id, imported, &mut HashSet::new())?
                    } else {
                        return None;
                    };
                    sum.checked_add(width.checked_mul(trailing.array_size as usize)?)
                });

        let item_width = if let Some(width) = primitive_width(element.type_id) {
            width
        } else if element.type_id >= 1000 {
            fixed_dataset_width(element.type_id, imported, &mut HashSet::new()).ok_or_else(
                || {
                    format!(
                        "嵌套 Dataset {} 包含动态长度，无法从父 Dataset 自动切片",
                        element.type_id
                    )
                },
            )?
        } else {
            return Err(format!(
                "Dataset {} 字段 {} 使用未知类型 {}",
                dataset_id, element.name, element.type_id
            ));
        };

        let count = if element.dynamic {
            let reserved = remaining_fixed_width.ok_or_else(|| {
                format!(
                    "Dataset {} 字段 {} 为动态数组，但其后仍有动态/未知长度字段",
                    dataset_id, element.name
                )
            })?;
            if payload.len() < offset + reserved {
                return Err(format!("Dataset {dataset_id} payload 长度不足"));
            }
            let available = payload.len() - offset - reserved;
            if !available.is_multiple_of(item_width) {
                return Err(format!(
                    "Dataset {} 字段 {} 的动态数组长度 {} 不是元素宽度 {} 的整数倍",
                    dataset_id, element.name, available, item_width
                ));
            }
            available / item_width
        } else {
            element.array_size as usize
        };

        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            if offset + item_width > payload.len() {
                return Err(format!(
                    "Dataset {} payload 长度不足：字段 {} 需要 {} bytes，当前 offset {} / {}",
                    dataset_id,
                    element.name,
                    item_width,
                    offset,
                    payload.len()
                ));
            }
            let slice = &payload[offset..offset + item_width];
            let value = if element.type_id >= 1000 {
                let (nested, consumed) =
                    decode_dataset_inner(element.type_id, slice, imported, visiting)?;
                if consumed != item_width {
                    return Err(format!("嵌套 Dataset {} 长度不一致", element.type_id));
                }
                Value::Object(nested)
            } else {
                decode_primitive(element.type_id, slice)
            };
            values.push(value);
            offset += item_width;
        }

        let raw = if values.len() == 1 && !element.dynamic {
            values.remove(0)
        } else {
            Value::Array(values)
        };
        let display = match raw.as_f64() {
            Some(number) if element.scale.is_some() || element.offset.is_some() => {
                json!(number * element.scale.unwrap_or(1.0) + element.offset.unwrap_or(0) as f64)
            }
            _ => raw.clone(),
        };
        fields.insert(
            element.name.clone(),
            json!({
                "type": element.data_type,
                "type_id": element.type_id,
                "unit": element.unit,
                "raw": raw,
                "value": display
            }),
        );
    }
    visiting.remove(&dataset_id);
    Ok((fields, offset))
}

fn encode_hex(data: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(data.len() * 2);
    for &byte in data {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn numeric_value(value: &Value, field: &str) -> Result<f64, String> {
    value
        .as_f64()
        .ok_or_else(|| format!("字段 {field} 需要数字值"))
}

fn integer_value(value: &Value, field: &str) -> Result<i128, String> {
    if let Some(value) = value.as_i64() {
        return Ok(value as i128);
    }
    if let Some(value) = value.as_u64() {
        return Ok(value as i128);
    }
    if let Some(value) = value.as_f64() {
        let rounded = value.round();
        if (value - rounded).abs() <= 1e-9 {
            return Ok(rounded as i128);
        }
    }
    Err(format!("字段 {field} 需要整数值"))
}

fn wire_value(element: &TrdpXmlElement, value: &Value) -> Result<Value, String> {
    if element.scale.is_none() && element.offset.is_none() {
        return Ok(value.clone());
    }
    let scale = element.scale.unwrap_or(1.0);
    if scale == 0.0 {
        return Err(format!("字段 {} 的 scale 不能为 0", element.name));
    }
    let display = numeric_value(value, &element.name)?;
    let raw = (display - element.offset.unwrap_or(0) as f64) / scale;
    serde_json::Number::from_f64(raw)
        .map(Value::Number)
        .ok_or_else(|| format!("字段 {} 缩放后的值无效", element.name))
}

fn encode_primitive(
    element: &TrdpXmlElement,
    value: &Value,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    let value = wire_value(element, value)?;
    let field = element.name.as_str();
    match element.type_id {
        1 => {
            let byte = if let Some(value) = value.as_bool() {
                u8::from(value)
            } else {
                u8::try_from(integer_value(&value, field)?)
                    .map_err(|_| format!("字段 {field} 超出 UINT8 范围"))?
            };
            output.push(byte);
        }
        2 => {
            if let Some(text) = value.as_str() {
                let bytes = text.as_bytes();
                if bytes.len() != 1 {
                    return Err(format!("字段 {field} 的 CHAR8 必须恰好一个字节"));
                }
                output.push(bytes[0]);
            } else {
                output.push(
                    u8::try_from(integer_value(&value, field)?)
                        .map_err(|_| format!("字段 {field} 超出 CHAR8 范围"))?,
                );
            }
        }
        3 => {
            let encoded = if let Some(text) = value.as_str() {
                let mut units = text.encode_utf16();
                let first = units
                    .next()
                    .ok_or_else(|| format!("字段 {field} 的 UTF16 不能为空"))?;
                if units.next().is_some() {
                    return Err(format!("字段 {field} 的 UTF16 必须恰好一个 code unit"));
                }
                first
            } else {
                u16::try_from(integer_value(&value, field)?)
                    .map_err(|_| format!("字段 {field} 超出 UTF16 范围"))?
            };
            output.extend_from_slice(&encoded.to_be_bytes());
        }
        4 => output.push(
            i8::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 INT8 范围"))? as u8,
        ),
        5 => output.extend_from_slice(
            &i16::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 INT16 范围"))?
                .to_be_bytes(),
        ),
        6 => output.extend_from_slice(
            &i32::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 INT32 范围"))?
                .to_be_bytes(),
        ),
        7 => output.extend_from_slice(
            &i64::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 INT64 范围"))?
                .to_be_bytes(),
        ),
        8 => output.push(
            u8::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 UINT8 范围"))?,
        ),
        9 => output.extend_from_slice(
            &u16::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 UINT16 范围"))?
                .to_be_bytes(),
        ),
        10 | 14 => output.extend_from_slice(
            &u32::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 UINT32 范围"))?
                .to_be_bytes(),
        ),
        11 => output.extend_from_slice(
            &u64::try_from(integer_value(&value, field)?)
                .map_err(|_| format!("字段 {field} 超出 UINT64 范围"))?
                .to_be_bytes(),
        ),
        16 => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("字段 {field} 的 TIMEDATE64 需要 {{seconds,microseconds}}"))?;
            let seconds = object
                .get("seconds")
                .ok_or_else(|| format!("字段 {field} 缺少 seconds"))?;
            let microseconds = object
                .get("microseconds")
                .ok_or_else(|| format!("字段 {field} 缺少 microseconds"))?;
            output.extend_from_slice(
                &u32::try_from(integer_value(seconds, field)?)
                    .map_err(|_| format!("字段 {field}.seconds 超出 UINT32 范围"))?
                    .to_be_bytes(),
            );
            output.extend_from_slice(
                &u32::try_from(integer_value(microseconds, field)?)
                    .map_err(|_| format!("字段 {field}.microseconds 超出 UINT32 范围"))?
                    .to_be_bytes(),
            );
        }
        12 => output.extend_from_slice(&(numeric_value(&value, field)? as f32).to_be_bytes()),
        13 => output.extend_from_slice(&numeric_value(&value, field)?.to_be_bytes()),
        15 => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("字段 {field} 的 TIMEDATE48 需要 {{seconds,ticks}}"))?;
            let seconds = object
                .get("seconds")
                .ok_or_else(|| format!("字段 {field} 缺少 seconds"))?;
            let ticks = object
                .get("ticks")
                .ok_or_else(|| format!("字段 {field} 缺少 ticks"))?;
            output.extend_from_slice(
                &u32::try_from(integer_value(seconds, field)?)
                    .map_err(|_| format!("字段 {field}.seconds 超出 UINT32 范围"))?
                    .to_be_bytes(),
            );
            output.extend_from_slice(
                &u16::try_from(integer_value(ticks, field)?)
                    .map_err(|_| format!("字段 {field}.ticks 超出 UINT16 范围"))?
                    .to_be_bytes(),
            );
        }
        _ => return Err(format!("字段 {field} 使用不支持的类型 {}", element.type_id)),
    }
    Ok(())
}

fn field_items(element: &TrdpXmlElement, value: &Value) -> Result<Vec<Value>, String> {
    if element.type_id == 2 && (element.dynamic || element.array_size != 1) {
        if let Some(text) = value.as_str() {
            let values = text
                .as_bytes()
                .iter()
                .map(|byte| Value::from(*byte))
                .collect::<Vec<_>>();
            if !element.dynamic && values.len() != element.array_size as usize {
                return Err(format!(
                    "字段 {} 需要 {} 个 CHAR8，实际 {}",
                    element.name,
                    element.array_size,
                    values.len()
                ));
            }
            return Ok(values);
        }
    }

    if element.dynamic || element.array_size != 1 {
        let values = value
            .as_array()
            .ok_or_else(|| format!("字段 {} 需要 JSON 数组", element.name))?
            .clone();
        if !element.dynamic && values.len() != element.array_size as usize {
            return Err(format!(
                "字段 {} 需要 {} 个元素，实际 {}",
                element.name,
                element.array_size,
                values.len()
            ));
        }
        Ok(values)
    } else {
        Ok(vec![value.clone()])
    }
}

fn encode_dataset_inner(
    dataset_id: u32,
    values: &Map<String, Value>,
    imported: &TrdpXmlImport,
    visiting: &mut HashSet<u32>,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    if !visiting.insert(dataset_id) {
        return Err(format!("Dataset 嵌套循环: {dataset_id}"));
    }
    let dataset = imported
        .datasets
        .iter()
        .find(|dataset| dataset.id == dataset_id)
        .ok_or_else(|| format!("Dataset {dataset_id} 不存在"))?;

    for element in &dataset.elements {
        let value = values
            .get(&element.name)
            .ok_or_else(|| format!("Dataset {dataset_id} 缺少字段 {}", element.name))?;
        let items = field_items(element, value)?;
        for item in items {
            if element.type_id >= 1000 {
                let nested = item.as_object().ok_or_else(|| {
                    format!(
                        "Dataset {dataset_id} 字段 {} 需要嵌套 JSON object",
                        element.name
                    )
                })?;
                encode_dataset_inner(element.type_id, nested, imported, visiting, output)?;
            } else {
                encode_primitive(element, &item, output)?;
            }
        }
    }
    visiting.remove(&dataset_id);
    Ok(())
}

pub fn trdp_encode_dataset(path: String, dataset_id: u32, values: Value) -> Result<Value, String> {
    let imported = parse_xml(&path)?;
    let values = values
        .as_object()
        .ok_or("Dataset encode values 必须是 JSON object")?;
    let mut output = Vec::new();
    encode_dataset_inner(
        dataset_id,
        values,
        &imported,
        &mut HashSet::new(),
        &mut output,
    )?;
    Ok(json!({
        "dataset_id": dataset_id,
        "payload_bytes": output.len(),
        "payload_hex": encode_hex(&output)
    }))
}

pub fn trdp_decode_dataset(
    path: String,
    dataset_id: u32,
    payload_hex: String,
) -> Result<Value, String> {
    let imported = parse_xml(&path)?;
    let dataset = imported
        .datasets
        .iter()
        .find(|dataset| dataset.id == dataset_id)
        .ok_or_else(|| format!("Dataset {dataset_id} 不存在"))?;
    let payload = decode_hex(&payload_hex)?;
    let (fields, consumed) =
        decode_dataset_inner(dataset_id, &payload, &imported, &mut HashSet::new())?;
    Ok(json!({
        "dataset_id": dataset.id,
        "dataset_name": dataset.name,
        "consumed_bytes": consumed,
        "payload_bytes": payload.len(),
        "fields": fields
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn maps_numeric_tcnopen_types() {
        assert_eq!(type_id("5"), Some(5));
        assert_eq!(type_name(5, "5"), "INT16");
        assert_eq!(type_id("UINT32"), Some(10));
    }

    #[test]
    fn round_trips_timedate64_components() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(
            file,
            r#"<device><data-set name="time" id="1000"><element name="stamp" type="TIMEDATE64"/></data-set></device>"#
        )
        .expect("write");
        let path = file.path().to_string_lossy().to_string();
        let encoded = trdp_encode_dataset(
            path.clone(),
            1000,
            json!({"stamp": {"seconds": 7, "microseconds": 123456}}),
        )
        .expect("encode TIMEDATE64");
        assert_eq!(encoded["payload_hex"], "000000070001E240");
        let decoded = trdp_decode_dataset(
            path,
            1000,
            encoded["payload_hex"].as_str().unwrap().to_string(),
        )
        .expect("decode TIMEDATE64");
        assert_eq!(decoded["fields"]["stamp"]["value"]["seconds"], json!(7u32));
        assert_eq!(
            decoded["fields"]["stamp"]["value"]["microseconds"],
            json!(123456u32)
        );
    }

    #[test]
    fn supports_nested_dataset_id_1000() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(
            file,
            r#"<device><data-set name="child" id="1000"><element name="value" type="UINT16"/></data-set><data-set name="parent" id="1001"><element name="child" type="1000"/></data-set></device>"#
        )
        .expect("write");
        let path = file.path().to_string_lossy().to_string();
        let encoded = trdp_encode_dataset(path.clone(), 1001, json!({"child": {"value": 42}}))
            .expect("encode nested dataset 1000");
        assert_eq!(encoded["payload_hex"], "002A");
        let decoded = trdp_decode_dataset(
            path,
            1001,
            encoded["payload_hex"].as_str().unwrap().to_string(),
        )
        .expect("decode nested dataset 1000");
        assert_eq!(decoded["fields"]["child"]["value"]["value"]["value"], json!(42u16));
    }

    #[test]
    fn encodes_structured_values_to_wire_payload() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(
            file,
            r#"<device><data-set-list><data-set name="demo" id="1001"><element name="counter" type="10"/><element name="text" type="2" array-size="0"/></data-set></data-set-list></device>"#
        )
        .expect("write");
        let path = file.path().to_string_lossy().to_string();
        let encoded = trdp_encode_dataset(path.clone(), 1001, json!({"counter": 7, "text": "OK"}))
            .expect("encode");
        assert_eq!(encoded["payload_hex"], "000000074F4B");

        let decoded = trdp_decode_dataset(
            path,
            1001,
            encoded["payload_hex"].as_str().unwrap().to_string(),
        )
        .expect("decode");
        assert_eq!(decoded["fields"]["counter"]["value"], json!(7u32));
        assert_eq!(decoded["fields"]["text"]["value"], json!(["O", "K"]));
    }

    #[test]
    fn distinguishes_pd_and_md_telegrams() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(
            file,
            r#"<device><telegram name="pd" com-id="1001" data-set-id="1001"><pd-parameter cycle="100000"/></telegram><telegram name="md" com-id="2001" data-set-id="1001"><md-parameter/><source uri1="10.0.0.1"/><destination uri="10.0.0.2"/></telegram><data-set name="demo" id="1001"><element name="counter" type="10"/></data-set></device>"#
        )
        .expect("write");
        let imported = parse_xml(&file.path().to_string_lossy()).expect("import");
        assert_eq!(imported.telegrams.len(), 2);
        assert_eq!(imported.telegrams[0].traffic_kind, "pd");
        assert_eq!(imported.telegrams[1].traffic_kind, "md");
        assert_eq!(imported.telegrams[1].cycle_us, None);
        assert_eq!(imported.telegrams[1].timeout_us, None);
    }

    #[test]
    fn imports_official_style_xml_and_dynamic_array() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(
            file,
            r#"<device><bus-interface-list><bus-interface><pd-com-parameter port="17224"/><md-com-parameter udp-port="17225" tcp-port="17225"/><telegram-list><telegram name="demo" com-id="1001" data-set-id="1001"><pd-parameter cycle="100000"/></telegram></telegram-list></bus-interface></bus-interface-list><data-set-list><data-set name="demo" id="1001"><element name="counter" type="10"/><element name="text" type="2" array-size="0"/></data-set></data-set-list></device>"#
        )
        .expect("write");
        let path = file.path().to_string_lossy().to_string();
        let imported = parse_xml(&path).expect("import");
        assert_eq!(imported.datasets.len(), 1);
        assert_eq!(imported.telegrams.len(), 1);
        assert_eq!(imported.telegrams[0].com_id, 1001);
        assert_eq!(imported.telegrams[0].traffic_kind, "pd");
        assert_eq!(
            imported.telegrams[0].timeout_behavior.as_deref(),
            Some("zero")
        );
        assert!(imported.datasets[0].elements[1].dynamic);
        let payload = [0, 0, 0, 7, b'O', b'K'];
        let (fields, consumed) =
            decode_dataset_inner(1001, &payload, &imported, &mut HashSet::new()).expect("decode");
        assert_eq!(consumed, payload.len());
        assert_eq!(fields["counter"]["value"], json!(7u32));
    }
}
