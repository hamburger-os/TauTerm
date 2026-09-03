use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrdpXmlElement {
    pub name: String,
    pub data_type: String,
    pub array_size: u32,
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

fn parse_xml(path: &str) -> Result<TrdpXmlImport, String> {
    let xml = fs::read_to_string(path).map_err(|error| format!("读取 TRDP XML 失败: {error}"))?;
    let dataset_re = Regex::new(r#"(?is)<data-set\b([^>]*)>(.*?)</data-set>"#).map_err(|error| error.to_string())?;
    let element_re = Regex::new(r#"(?is)<element\b([^>]*)/?>"#).map_err(|error| error.to_string())?;
    let telegram_re = Regex::new(r#"(?is)<telegram\b([^>]*)>(.*?)</telegram>"#).map_err(|error| error.to_string())?;
    let pd_re = Regex::new(r#"(?is)<pd-parameter\b([^>]*)/?>"#).map_err(|error| error.to_string())?;
    let source_re = Regex::new(r#"(?is)<source\b([^>]*)"#).map_err(|error| error.to_string())?;
    let destination_re = Regex::new(r#"(?is)<destination\b([^>]*)"#).map_err(|error| error.to_string())?;

    let mut warnings = Vec::new();
    let mut datasets = Vec::new();
    for capture in dataset_re.captures_iter(&xml) {
        let tag = capture.get(1).map(|value| value.as_str()).unwrap_or_default();
        let body = capture.get(2).map(|value| value.as_str()).unwrap_or_default();
        let Some(id) = attr_u32(tag, "id") else {
            warnings.push("忽略缺少数字 id 的 <data-set>".to_string());
            continue;
        };
        let name = attr(tag, "name").unwrap_or_else(|| format!("Dataset {id}"));
        let elements = element_re
            .captures_iter(body)
            .map(|element| {
                let attributes = element.get(1).map(|value| value.as_str()).unwrap_or_default();
                TrdpXmlElement {
                    name: attr(attributes, "name").unwrap_or_else(|| "unnamed".to_string()),
                    data_type: attr(attributes, "type").unwrap_or_else(|| "INVALID".to_string()),
                    array_size: attr_u32(attributes, "array-size").unwrap_or(1),
                    unit: attr(attributes, "unit"),
                    scale: attr(attributes, "scale").and_then(|value| value.parse().ok()),
                    offset: attr(attributes, "offset").and_then(|value| value.parse().ok()),
                }
            })
            .collect();
        datasets.push(TrdpXmlDataset { id, name, elements });
    }

    let mut telegrams = Vec::new();
    for capture in telegram_re.captures_iter(&xml) {
        let tag = capture.get(1).map(|value| value.as_str()).unwrap_or_default();
        let body = capture.get(2).map(|value| value.as_str()).unwrap_or_default();
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
            .filter_map(|value| attr(value.as_str(), "uri1").or_else(|| attr(value.as_str(), "uri")))
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

    let sdt_detected = xml.to_ascii_lowercase().contains("<sdt-parameter")
        || xml.to_ascii_lowercase().contains("sdtv2")
        || xml.to_ascii_lowercase().contains("sdtv4");
    if sdt_detected {
        warnings.push(
            "检测到 SDT 配置：TauTerm 首版仅展示元数据，不执行 SDTv2/SDTv4 安全验证。".to_string(),
        );
    }
    Ok(TrdpXmlImport {
        path: path.to_string(),
        datasets,
        telegrams,
        sdt_detected,
        warnings,
    })
}

#[tauri::command]
pub fn trdp_import_xml(path: String) -> Result<TrdpXmlImport, String> {
    parse_xml(&path)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let compact: String = value.chars().filter(|character| !character.is_whitespace()).collect();
    if compact.len() % 2 != 0 {
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

fn primitive_width(data_type: &str) -> Option<usize> {
    match data_type.to_ascii_uppercase().as_str() {
        "BOOL8" | "BITSET8" | "CHAR8" | "INT8" | "UINT8" => Some(1),
        "UTF16" | "INT16" | "UINT16" => Some(2),
        "INT32" | "UINT32" | "REAL32" | "TIMEDATE32" => Some(4),
        "INT64" | "UINT64" | "REAL64" | "TIMEDATE64" => Some(8),
        "TIMEDATE48" => Some(6),
        _ => None,
    }
}

fn decode_primitive(data_type: &str, bytes: &[u8]) -> Value {
    match data_type.to_ascii_uppercase().as_str() {
        "BOOL8" => json!(bytes.first().copied().unwrap_or(0) != 0),
        "BITSET8" | "UINT8" => json!(bytes.first().copied().unwrap_or(0)),
        "CHAR8" => json!(bytes.first().map(|value| char::from(*value).to_string()).unwrap_or_default()),
        "INT8" => json!(bytes.first().copied().unwrap_or(0) as i8),
        "UTF16" | "UINT16" => json!(u16::from_be_bytes([bytes[0], bytes[1]])),
        "INT16" => json!(i16::from_be_bytes([bytes[0], bytes[1]])),
        "UINT32" | "TIMEDATE32" => json!(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        "INT32" => json!(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        "REAL32" => json!(f32::from_bits(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))),
        "UINT64" | "TIMEDATE64" => json!(u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))),
        "INT64" => json!(i64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))),
        "REAL64" => json!(f64::from_bits(u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8])))),
        "TIMEDATE48" => json!({
            "seconds": u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            "ticks": u16::from_be_bytes([bytes[4], bytes[5]])
        }),
        _ => json!(null),
    }
}

#[tauri::command]
pub fn trdp_decode_dataset(path: String, dataset_id: u32, payload_hex: String) -> Result<Value, String> {
    let imported = parse_xml(&path)?;
    let dataset = imported
        .datasets
        .iter()
        .find(|dataset| dataset.id == dataset_id)
        .ok_or_else(|| format!("Dataset {dataset_id} 不存在"))?;
    let payload = decode_hex(&payload_hex)?;
    let mut offset = 0usize;
    let mut fields = Vec::new();
    for element in &dataset.elements {
        let Some(width) = primitive_width(&element.data_type) else {
            fields.push(json!({
                "name": element.name,
                "type": element.data_type,
                "value": null,
                "error": "nested/unknown dataset requires recursive schema support"
            }));
            continue;
        };
        let mut values = Vec::new();
        for _ in 0..element.array_size.max(1) {
            if offset + width > payload.len() {
                return Err(format!(
                    "Dataset {} payload 长度不足：字段 {} 需要 {} bytes，当前 offset {} / {}",
                    dataset.id, element.name, width, offset, payload.len()
                ));
            }
            let raw_value = decode_primitive(&element.data_type, &payload[offset..offset + width]);
            values.push(raw_value);
            offset += width;
        }
        let raw = if values.len() == 1 { values.remove(0) } else { Value::Array(values) };
        let display = match raw.as_f64() {
            Some(number) if element.scale.is_some() || element.offset.is_some() => json!(
                number * element.scale.unwrap_or(1.0) + element.offset.unwrap_or(0) as f64
            ),
            _ => raw.clone(),
        };
        fields.push(json!({
            "name": element.name,
            "type": element.data_type,
            "unit": element.unit,
            "raw": raw,
            "value": display
        }));
    }
    Ok(json!({
        "dataset_id": dataset.id,
        "dataset_name": dataset.name,
        "consumed_bytes": offset,
        "payload_bytes": payload.len(),
        "fields": fields
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_network_order_uints() {
        assert_eq!(decode_primitive("UINT16", &[0x12, 0x34]), json!(0x1234u16));
        assert_eq!(decode_primitive("UINT32", &[0, 0, 0, 9]), json!(9u32));
    }
}
