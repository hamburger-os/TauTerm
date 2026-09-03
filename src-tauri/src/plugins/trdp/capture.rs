use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const PD_PORT: u16 = 17224;
const MD_PORT: u16 = 17225;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TrdpPacket {
    pub event: String,
    pub link: String,
    pub timestamp_us: u64,
    pub src_ip: String,
    pub dest_ip: String,
    pub src_port: u16,
    pub dest_port: u16,
    pub transport: String,
    pub msg_type: String,
    pub com_id: u32,
    pub seq_count: u32,
    pub protocol_version: u16,
    pub etb_topo_count: u32,
    pub op_trn_topo_count: u32,
    pub data_len: u32,
    pub payload_hex: String,
    pub raw_frame_hex: String,
    pub sdt_detected: bool,
}

fn be16(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?]))
}

fn be32(data: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *data.get(pos)?,
        *data.get(pos + 1)?,
        *data.get(pos + 2)?,
        *data.get(pos + 3)?,
    ]))
}

fn hex(data: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(data.len() * 2);
    for &byte in data {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn ipv4(data: &[u8]) -> String {
    format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3])
}

fn ethernet_ipv4_offset(frame: &[u8], linktype: u16) -> Option<usize> {
    match linktype {
        1 => {
            let mut ether_type = be16(frame, 12)?;
            let mut offset = 14usize;
            if matches!(ether_type, 0x8100 | 0x88a8) {
                ether_type = be16(frame, 16)?;
                offset = 18;
            }
            (ether_type == 0x0800).then_some(offset)
        }
        113 => Some(16), // Linux cooked v1
        276 => Some(20), // Linux cooked v2
        _ => None,
    }
}

fn decode_frame(frame: &[u8], linktype: u16, timestamp_us: u64) -> Option<TrdpPacket> {
    let ip = ethernet_ipv4_offset(frame, linktype)?;
    if (*frame.get(ip)? >> 4) != 4 {
        return None;
    }
    let ihl = ((*frame.get(ip)? & 0x0f) as usize) * 4;
    if ihl < 20 || frame.len() < ip + ihl {
        return None;
    }

    let protocol = *frame.get(ip + 9)?;
    let src_ip = ipv4(frame.get(ip + 12..ip + 16)?);
    let dest_ip = ipv4(frame.get(ip + 16..ip + 20)?);
    let transport_offset = ip + ihl;
    let (transport, src_port, dest_port, payload_offset) = match protocol {
        17 => (
            "udp",
            be16(frame, transport_offset)?,
            be16(frame, transport_offset + 2)?,
            transport_offset + 8,
        ),
        6 => {
            let header_length = ((*frame.get(transport_offset + 12)? >> 4) as usize) * 4;
            if header_length < 20 {
                return None;
            }
            (
                "tcp",
                be16(frame, transport_offset)?,
                be16(frame, transport_offset + 2)?,
                transport_offset + header_length,
            )
        }
        _ => return None,
    };

    if ![src_port, dest_port]
        .iter()
        .any(|port| matches!(*port, PD_PORT | MD_PORT))
        || frame.len() < payload_offset + 24
    {
        return None;
    }

    let message_bytes = frame.get(payload_offset + 6..payload_offset + 8)?;
    if !message_bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    let msg_type = String::from_utf8_lossy(message_bytes).to_string();
    // IEC 61375-2-3 / TCNOpen: PD header = 40 bytes, MD header = 116 bytes.
    let header_length = if msg_type.starts_with('M') { 116 } else { 40 };
    let data_len = be32(frame, payload_offset + 20)?;
    let data_start = payload_offset.saturating_add(header_length).min(frame.len());
    let data_end = data_start.saturating_add(data_len as usize).min(frame.len());

    Some(TrdpPacket {
        event: "packet".to_string(),
        link: "capture".to_string(),
        timestamp_us,
        src_ip,
        dest_ip,
        src_port,
        dest_port,
        transport: transport.to_string(),
        msg_type,
        com_id: be32(frame, payload_offset + 8)?,
        seq_count: be32(frame, payload_offset)?,
        protocol_version: be16(frame, payload_offset + 4)?,
        etb_topo_count: be32(frame, payload_offset + 12)?,
        op_trn_topo_count: be32(frame, payload_offset + 16)?,
        data_len,
        payload_hex: hex(frame.get(data_start..data_end).unwrap_or_default()),
        raw_frame_hex: hex(frame),
        // SDT is a payload-layer safety protocol. The generic packet parser does
        // not claim validation. XML import reports explicit SDT configuration.
        sdt_detected: false,
    })
}

fn read_u16(bytes: &[u8], pos: usize, little: bool) -> Option<u16> {
    let raw = [*bytes.get(pos)?, *bytes.get(pos + 1)?];
    Some(if little {
        u16::from_le_bytes(raw)
    } else {
        u16::from_be_bytes(raw)
    })
}

fn read_u32(bytes: &[u8], pos: usize, little: bool) -> Option<u32> {
    let raw = [
        *bytes.get(pos)?,
        *bytes.get(pos + 1)?,
        *bytes.get(pos + 2)?,
        *bytes.get(pos + 3)?,
    ];
    Some(if little {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

fn parse_pcap(bytes: &[u8]) -> Result<Vec<TrdpPacket>, String> {
    if bytes.len() < 24 {
        return Err("pcap 文件过短".to_string());
    }
    let (little, nanos) = match &bytes[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        _ => return Err("不支持的 pcap magic".to_string()),
    };
    let linktype = read_u32(bytes, 20, little).ok_or("pcap header invalid")? as u16;
    let mut position = 24usize;
    let mut packets = Vec::new();
    while position + 16 <= bytes.len() {
        let seconds = read_u32(bytes, position, little).unwrap_or(0) as u64;
        let fraction = read_u32(bytes, position + 4, little).unwrap_or(0) as u64;
        let captured = read_u32(bytes, position + 8, little).unwrap_or(0) as usize;
        position += 16;
        if position + captured > bytes.len() {
            break;
        }
        let timestamp = seconds.saturating_mul(1_000_000)
            + if nanos { fraction / 1_000 } else { fraction };
        if let Some(packet) = decode_frame(&bytes[position..position + captured], linktype, timestamp) {
            packets.push(packet);
        }
        position += captured;
    }
    Ok(packets)
}

fn parse_pcapng(bytes: &[u8]) -> Result<Vec<TrdpPacket>, String> {
    let mut position = 0usize;
    let mut little = true;
    let mut interfaces: Vec<u16> = Vec::new();
    let mut packets = Vec::new();
    while position + 12 <= bytes.len() {
        if bytes.get(position..position + 4) == Some(&[0x0a, 0x0d, 0x0d, 0x0a]) {
            little = match bytes.get(position + 8..position + 12) {
                Some([0x4d, 0x3c, 0x2b, 0x1a]) => true,
                Some([0x1a, 0x2b, 0x3c, 0x4d]) => false,
                _ => return Err("pcapng byte-order magic invalid".to_string()),
            };
            interfaces.clear();
        }
        let block_type = read_u32(bytes, position, little).ok_or("pcapng block invalid")?;
        let total = read_u32(bytes, position + 4, little).ok_or("pcapng length invalid")? as usize;
        if total < 12 || position + total > bytes.len() {
            break;
        }
        match block_type {
            1 if total >= 20 => interfaces.push(read_u16(bytes, position + 8, little).unwrap_or(1)),
            6 if total >= 32 => {
                let interface = read_u32(bytes, position + 8, little).unwrap_or(0) as usize;
                let timestamp_high = read_u32(bytes, position + 12, little).unwrap_or(0) as u64;
                let timestamp_low = read_u32(bytes, position + 16, little).unwrap_or(0) as u64;
                let captured = read_u32(bytes, position + 20, little).unwrap_or(0) as usize;
                let start = position + 28;
                if start + captured <= position + total {
                    let timestamp = (timestamp_high << 32) | timestamp_low;
                    let linktype = interfaces.get(interface).copied().unwrap_or(1);
                    if let Some(packet) = decode_frame(&bytes[start..start + captured], linktype, timestamp) {
                        packets.push(packet);
                    }
                }
            }
            _ => {}
        }
        position += total;
    }
    Ok(packets)
}

#[tauri::command]
pub fn trdp_open_capture(path: String) -> Result<Vec<TrdpPacket>, String> {
    let bytes = fs::read(&path).map_err(|error| format!("读取抓包失败: {error}"))?;
    if bytes.starts_with(&[0x0a, 0x0d, 0x0d, 0x0a]) {
        parse_pcapng(&bytes)
    } else {
        parse_pcap(&bytes)
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

fn append_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[tauri::command]
pub fn trdp_save_capture(path: String, packets: Vec<TrdpPacket>) -> Result<(), String> {
    let mut output = Vec::new();
    // Section Header Block (little-endian).
    append_u32(&mut output, 0x0a0d0d0a);
    append_u32(&mut output, 28);
    append_u32(&mut output, 0x1a2b3c4d);
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&u64::MAX.to_le_bytes());
    append_u32(&mut output, 28);
    // Interface Description Block: Ethernet, snaplen 65535.
    append_u32(&mut output, 1);
    append_u32(&mut output, 20);
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    append_u32(&mut output, 65_535);
    append_u32(&mut output, 20);

    for packet in packets {
        let Some(frame) = decode_hex(&packet.raw_frame_hex) else {
            continue;
        };
        if frame.is_empty() {
            continue;
        }
        let padded = (frame.len() + 3) & !3;
        let total = 32 + padded;
        append_u32(&mut output, 6);
        append_u32(&mut output, total as u32);
        append_u32(&mut output, 0);
        append_u32(&mut output, (packet.timestamp_us >> 32) as u32);
        append_u32(&mut output, packet.timestamp_us as u32);
        append_u32(&mut output, frame.len() as u32);
        append_u32(&mut output, frame.len() as u32);
        output.extend_from_slice(&frame);
        output.resize(output.len() + (padded - frame.len()), 0);
        append_u32(&mut output, total as u32);
    }
    fs::write(Path::new(&path), output).map_err(|error| format!("保存 pcapng 失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_trdp_ports() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 40];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[10, 0, 0, 2]);
        frame[34..36].copy_from_slice(&1000u16.to_be_bytes());
        frame[36..38].copy_from_slice(&1001u16.to_be_bytes());
        assert!(decode_frame(&frame, 1, 0).is_none());
    }

    #[test]
    fn parses_pd_header() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 44];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[239, 1, 1, 1]);
        frame[34..36].copy_from_slice(&PD_PORT.to_be_bytes());
        frame[36..38].copy_from_slice(&PD_PORT.to_be_bytes());
        let payload = 42;
        frame[payload..payload + 4].copy_from_slice(&7u32.to_be_bytes());
        frame[payload + 4..payload + 6].copy_from_slice(&0x0100u16.to_be_bytes());
        frame[payload + 6..payload + 8].copy_from_slice(b"Pd");
        frame[payload + 8..payload + 12].copy_from_slice(&1001u32.to_be_bytes());
        frame[payload + 20..payload + 24].copy_from_slice(&4u32.to_be_bytes());
        frame[payload + 40..payload + 44].copy_from_slice(&[1, 2, 3, 4]);
        let packet = decode_frame(&frame, 1, 123).expect("TRDP PD packet");
        assert_eq!(packet.com_id, 1001);
        assert_eq!(packet.seq_count, 7);
        assert_eq!(packet.payload_hex, "01020304");
    }
}
