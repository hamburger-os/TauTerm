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
    pub com_id: u32,
    pub dataset_id: u32,
    pub cycle_us: Option<u32>,
    pub timeout_us: Option<u32>,
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
    let pattern = format!(
        r#"(?i)\b{}\s*=\s*[\"']([^\"']*)[\"']"#,
        regex::escape(name)
    );
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
        nested if nested > 1000 => return format!("Dataset {nested}"),
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
        Regex::new(r#"(?is)<pd-parameter\s+([^>]*)/?>"#).map_err(|error| error.to_string())?;
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
        let pd_attributes = pd_re
            .captures(body)
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
            com_id,
            dataset_id,
            cycle_us: attr_u32(pd_attributes, "cycle"),
            timeout_us: attr_u32(pd_attributes, "timeout"),
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
        11 | 16 => json!(u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))),
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
        } else if element.type_id > 1000 {
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
                    } else if trailing.type_id > 1000 {
                        fixed_dataset_width(trailing.type_id, imported, &mut HashSet::new())?
                    } else {
                        return None;
                    };
                    sum.checked_add(width.checked_mul(trailing.array_size as usize)?)
                });

        let item_width = if let Some(width) = primitive_width(element.type_id) {
            width
        } else if element.type_id > 1000 {
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
            let value = if element.type_id > 1000 {
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
        assert!(imported.datasets[0].elements[1].dynamic);
        let payload = [0, 0, 0, 7, b'O', b'K'];
        let (fields, consumed) =
            decode_dataset_inner(1001, &payload, &imported, &mut HashSet::new()).expect("decode");
        assert_eq!(consumed, payload.len());
        assert_eq!(fields["counter"]["value"], json!(7u32));
    }
}
