use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const STANDARD_PD_PORT: u16 = 17224;
const STANDARD_MD_PORT: u16 = 17225;
const LINKTYPE_NULL: u32 = 0;
const LINKTYPE_ETHERNET: u32 = 1;
const LINKTYPE_RAW: u32 = 101;
const LINKTYPE_LINUX_SLL: u32 = 113;
const LINKTYPE_LINUX_SLL2: u32 = 276;

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
    pub link_type: Option<u32>,
    pub crc_valid: Option<bool>,
    pub protocol_valid: Option<bool>,
    pub reply_status: Option<i32>,
    pub user_status: Option<u16>,
    pub reply_timeout_us: Option<u32>,
    pub md_session_id: Option<String>,
    pub src_uri: Option<String>,
    pub dest_uri: Option<String>,
    pub sdt_detected: bool,
}

#[derive(Debug, Clone, Copy)]
struct CapturePorts<'a> {
    pd: &'a [u16],
    md: &'a [u16],
}

impl CapturePorts<'_> {
    fn accepts(self, source: u16, destination: u16) -> bool {
        self.pd.contains(&source)
            || self.pd.contains(&destination)
            || self.md.contains(&source)
            || self.md.contains(&destination)
    }
}

fn be16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
    ]))
}

fn be32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ]))
}

fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let bytes = [*data.get(offset)?, *data.get(offset + 1)?];
    Some(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let bytes = [
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ];
    Some(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
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

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&value[offset..offset + 2], 16).ok())
        .collect()
}

fn ipv4(data: &[u8]) -> String {
    format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3])
}

fn uuid_text(data: &[u8]) -> Option<String> {
    let bytes = data.get(..16)?;
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

fn fixed_text(data: &[u8]) -> Option<String> {
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    let value = String::from_utf8_lossy(&data[..end]).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn trdp_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320u32 & mask);
        }
    }
    !crc
}

fn network_offset(frame: &[u8], linktype: u32) -> Option<usize> {
    match linktype {
        LINKTYPE_ETHERNET => {
            let mut ether_type = be16(frame, 12)?;
            let mut offset = 14;
            while matches!(ether_type, 0x8100 | 0x88a8 | 0x9100) {
                ether_type = be16(frame, offset + 2)?;
                offset += 4;
            }
            (ether_type == 0x0800).then_some(offset)
        }
        LINKTYPE_LINUX_SLL => (frame.len() >= 16 && be16(frame, 14)? == 0x0800).then_some(16),
        LINKTYPE_LINUX_SLL2 => (frame.len() >= 20 && be16(frame, 0)? == 0x0800).then_some(20),
        LINKTYPE_NULL => (frame.len() >= 5 && frame.get(4)? >> 4 == 4).then_some(4),
        LINKTYPE_RAW => (frame.first()? >> 4 == 4).then_some(0),
        _ => None,
    }
}

fn decode_frame(
    frame: &[u8],
    linktype: u32,
    timestamp_us: u64,
    ports: CapturePorts<'_>,
) -> Option<TrdpPacket> {
    let ip = network_offset(frame, linktype)?;
    if frame.get(ip)? >> 4 != 4 {
        return None;
    }
    let ip_header_length = ((frame.get(ip)? & 0x0f) as usize) * 4;
    let ip_total_length = be16(frame, ip + 2)? as usize;
    if ip_header_length < 20
        || ip_total_length < ip_header_length
        || frame.len() < ip + ip_total_length
    {
        return None;
    }
    if be16(frame, ip + 6)? & 0x3fff != 0 {
        return None;
    }

    let ip_end = ip + ip_total_length;
    let protocol = *frame.get(ip + 9)?;
    let src_ip = ipv4(frame.get(ip + 12..ip + 16)?);
    let dest_ip = ipv4(frame.get(ip + 16..ip + 20)?);
    let transport_offset = ip + ip_header_length;
    let (transport, src_port, dest_port, payload_offset, payload_end) = match protocol {
        17 => {
            if transport_offset + 8 > ip_end {
                return None;
            }
            let source = be16(frame, transport_offset)?;
            let destination = be16(frame, transport_offset + 2)?;
            let udp_length = be16(frame, transport_offset + 4)? as usize;
            if udp_length < 8 || transport_offset + udp_length > ip_end {
                return None;
            }
            (
                "udp",
                source,
                destination,
                transport_offset + 8,
                transport_offset + udp_length,
            )
        }
        6 => {
            if transport_offset + 20 > ip_end {
                return None;
            }
            let source = be16(frame, transport_offset)?;
            let destination = be16(frame, transport_offset + 2)?;
            let header_length = ((*frame.get(transport_offset + 12)? >> 4) as usize) * 4;
            if header_length < 20 || transport_offset + header_length > ip_end {
                return None;
            }
            (
                "tcp",
                source,
                destination,
                transport_offset + header_length,
                ip_end,
            )
        }
        _ => return None,
    };
    if !ports.accepts(src_port, dest_port) || payload_end < payload_offset + 24 {
        return None;
    }

    let seq_count = be32(frame, payload_offset)?;
    let protocol_version = be16(frame, payload_offset + 4)?;
    let message_bytes = frame.get(payload_offset + 6..payload_offset + 8)?;
    if !message_bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    let msg_type = String::from_utf8_lossy(message_bytes).to_string();
    if !matches!(
        msg_type.as_str(),
        "Pd" | "Pp" | "Pr" | "Pe" | "Mn" | "Mr" | "Mp" | "Mq" | "Mc" | "Me"
    ) {
        return None;
    }

    let com_id = be32(frame, payload_offset + 8)?;
    let etb_topo_count = be32(frame, payload_offset + 12)?;
    let op_trn_topo_count = be32(frame, payload_offset + 16)?;
    let data_len = be32(frame, payload_offset + 20)?;
    let trdp_header_length = if msg_type.starts_with('M') { 116 } else { 40 };
    if payload_end < payload_offset + trdp_header_length {
        return None;
    }
    let fcs_offset = payload_offset + trdp_header_length - 4;
    let stored_fcs = u32::from_le_bytes(frame.get(fcs_offset..fcs_offset + 4)?.try_into().ok()?);
    let crc_valid = stored_fcs == trdp_crc32(frame.get(payload_offset..fcs_offset)?);
    let protocol_valid = protocol_version & 0xff00 == 0x0100;
    let data_start = payload_offset + trdp_header_length;
    let data_end = data_start
        .saturating_add(data_len as usize)
        .min(payload_end);
    let payload = frame.get(data_start..data_end).unwrap_or_default();

    let (reply_status, user_status, reply_timeout_us, md_session_id, src_uri, dest_uri) =
        if msg_type.starts_with('M') {
            let raw_reply_status = be32(frame, payload_offset + 24)? as i32;
            let (reply_status, user_status) = if raw_reply_status >= 0 {
                (Some(0), Some(raw_reply_status as u16))
            } else {
                (Some(raw_reply_status), Some(0))
            };
            (
                reply_status,
                user_status,
                be32(frame, payload_offset + 44),
                uuid_text(frame.get(payload_offset + 28..payload_offset + 44)?),
                fixed_text(frame.get(payload_offset + 48..payload_offset + 80)?),
                fixed_text(frame.get(payload_offset + 80..payload_offset + 112)?),
            )
        } else {
            (None, None, None, None, None, None)
        };

    Some(TrdpPacket {
        event: "packet".into(),
        link: "capture".into(),
        timestamp_us,
        src_ip,
        dest_ip,
        src_port,
        dest_port,
        transport: transport.into(),
        msg_type,
        com_id,
        seq_count,
        protocol_version,
        etb_topo_count,
        op_trn_topo_count,
        data_len,
        payload_hex: hex(payload),
        raw_frame_hex: hex(frame),
        link_type: Some(linktype),
        crc_valid: Some(crc_valid),
        protocol_valid: Some(protocol_valid),
        reply_status,
        user_status,
        reply_timeout_us,
        md_session_id,
        src_uri,
        dest_uri,
        sdt_detected: false,
    })
}

fn parse_pcap(data: &[u8], ports: CapturePorts<'_>) -> Result<Vec<TrdpPacket>, String> {
    if data.len() < 24 {
        return Err("pcap 文件过短".into());
    }
    let (little_endian, nanoseconds) = match &data[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        _ => return Err("不支持的 pcap magic".into()),
    };
    let linktype = read_u32(data, 20, little_endian).ok_or("pcap header invalid")?;
    let mut offset = 24usize;
    let mut packets = Vec::new();
    while offset + 16 <= data.len() {
        let seconds = read_u32(data, offset, little_endian).unwrap_or(0) as u64;
        let fraction = read_u32(data, offset + 4, little_endian).unwrap_or(0) as u64;
        let captured_length = read_u32(data, offset + 8, little_endian).unwrap_or(0) as usize;
        offset += 16;
        if offset + captured_length > data.len() {
            return Err("pcap packet length exceeds file size".into());
        }
        let timestamp_us = seconds.saturating_mul(1_000_000)
            + if nanoseconds {
                fraction / 1_000
            } else {
                fraction
            };
        if let Some(packet) = decode_frame(
            &data[offset..offset + captured_length],
            linktype,
            timestamp_us,
            ports,
        ) {
            packets.push(packet);
        }
        offset += captured_length;
    }
    Ok(packets)
}

#[derive(Debug, Clone)]
struct PcapNgInterface {
    linktype: u32,
    timestamp_resolution: u8,
    timestamp_base2: bool,
    link: String,
}

fn parse_idb_options(
    data: &[u8],
    block_start: usize,
    block_length: usize,
    little_endian: bool,
) -> (u8, bool, Option<String>) {
    let mut offset = block_start + 16;
    let end = block_start + block_length - 4;
    let mut resolution = 6;
    let mut base2 = false;
    let mut link = None;
    while offset + 4 <= end {
        let code = read_u16(data, offset, little_endian).unwrap_or(0);
        let length = read_u16(data, offset + 2, little_endian).unwrap_or(0) as usize;
        offset += 4;
        if code == 0 || offset + length > end {
            break;
        }
        if code == 2 && length > 0 {
            let value = String::from_utf8_lossy(&data[offset..offset + length])
                .trim_end_matches('\0')
                .to_string();
            if !value.is_empty() {
                link = Some(value);
            }
        } else if code == 9 && length >= 1 {
            let value = data[offset];
            resolution = value & 0x7f;
            base2 = value & 0x80 != 0;
        }
        offset += (length + 3) & !3;
    }
    (resolution, base2, link)
}

fn pcapng_timestamp_to_us(raw: u64, resolution: u8, base2: bool) -> u64 {
    if base2 {
        let denominator = 1u128.checked_shl(resolution as u32).unwrap_or(u128::MAX);
        if denominator == 0 {
            return 0;
        }
        return ((raw as u128).saturating_mul(1_000_000) / denominator) as u64;
    }
    match resolution.cmp(&6) {
        std::cmp::Ordering::Equal => raw,
        std::cmp::Ordering::Less => raw.saturating_mul(10u64.pow((6 - resolution) as u32)),
        std::cmp::Ordering::Greater => raw / 10u64.pow((resolution - 6).min(19) as u32),
    }
}

fn parse_pcapng(data: &[u8], ports: CapturePorts<'_>) -> Result<Vec<TrdpPacket>, String> {
    let mut offset = 0usize;
    let mut little_endian = true;
    let mut interfaces: Vec<PcapNgInterface> = Vec::new();
    let mut packets = Vec::new();

    while offset + 12 <= data.len() {
        let section_header = data.get(offset..offset + 4) == Some(&[0x0a, 0x0d, 0x0d, 0x0a]);
        if section_header {
            little_endian = match data.get(offset + 8..offset + 12) {
                Some([0x4d, 0x3c, 0x2b, 0x1a]) => true,
                Some([0x1a, 0x2b, 0x3c, 0x4d]) => false,
                _ => return Err("pcapng byte-order magic invalid".into()),
            };
            interfaces.clear();
        }

        let block_type = if section_header {
            0x0a0d0d0a
        } else {
            read_u32(data, offset, little_endian).ok_or("pcapng block invalid")?
        };
        let block_length =
            read_u32(data, offset + 4, little_endian).ok_or("pcapng length invalid")? as usize;
        if block_length < 12 || offset + block_length > data.len() {
            return Err("pcapng block length exceeds file size".into());
        }
        let trailing_length = read_u32(data, offset + block_length - 4, little_endian)
            .ok_or("pcapng trailing length invalid")? as usize;
        if trailing_length != block_length {
            return Err("pcapng block length mismatch".into());
        }

        match block_type {
            1 if block_length >= 20 => {
                let linktype = read_u16(data, offset + 8, little_endian).unwrap_or(1) as u32;
                let (timestamp_resolution, timestamp_base2, link) =
                    parse_idb_options(data, offset, block_length, little_endian);
                let interface_index = interfaces.len();
                interfaces.push(PcapNgInterface {
                    linktype,
                    timestamp_resolution,
                    timestamp_base2,
                    link: link.unwrap_or_else(|| format!("capture:{interface_index}")),
                });
            }
            6 if block_length >= 32 => {
                let interface_index =
                    read_u32(data, offset + 8, little_endian).unwrap_or(0) as usize;
                let timestamp_high = read_u32(data, offset + 12, little_endian).unwrap_or(0) as u64;
                let timestamp_low = read_u32(data, offset + 16, little_endian).unwrap_or(0) as u64;
                let captured_length =
                    read_u32(data, offset + 20, little_endian).unwrap_or(0) as usize;
                let packet_start = offset + 28;
                if packet_start + captured_length > offset + block_length - 4 {
                    return Err("pcapng packet length exceeds block size".into());
                }
                let interface =
                    interfaces
                        .get(interface_index)
                        .cloned()
                        .unwrap_or(PcapNgInterface {
                            linktype: LINKTYPE_ETHERNET,
                            timestamp_resolution: 6,
                            timestamp_base2: false,
                            link: format!("capture:{interface_index}"),
                        });
                let raw_timestamp = (timestamp_high << 32) | timestamp_low;
                let timestamp_us = pcapng_timestamp_to_us(
                    raw_timestamp,
                    interface.timestamp_resolution,
                    interface.timestamp_base2,
                );
                if let Some(mut packet) = decode_frame(
                    &data[packet_start..packet_start + captured_length],
                    interface.linktype,
                    timestamp_us,
                    ports,
                ) {
                    packet.link = interface.link;
                    packets.push(packet);
                }
            }
            _ => {}
        }
        offset += block_length;
    }
    Ok(packets)
}

pub fn trdp_open_capture(
    path: String,
    pd_ports: Option<Vec<u16>>,
    md_ports: Option<Vec<u16>>,
) -> Result<Vec<TrdpPacket>, String> {
    let data = fs::read(&path).map_err(|error| format!("读取抓包失败: {error}"))?;
    let pd_ports = pd_ports.unwrap_or_else(|| vec![STANDARD_PD_PORT]);
    let md_ports = md_ports.unwrap_or_else(|| vec![STANDARD_MD_PORT]);
    let ports = CapturePorts {
        pd: &pd_ports,
        md: &md_ports,
    };
    if data.starts_with(&[0x0a, 0x0d, 0x0d, 0x0a]) {
        parse_pcapng(&data, ports)
    } else {
        parse_pcap(&data, ports)
    }
}

fn append_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn append_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn append_pcapng_option(output: &mut Vec<u8>, code: u16, value: &[u8]) {
    append_u16(output, code);
    append_u16(output, value.len() as u16);
    output.extend_from_slice(value);
    let padding = (4 - (value.len() % 4)) % 4;
    output.resize(output.len() + padding, 0);
}

fn append_interface_description(output: &mut Vec<u8>, linktype: u32, link: &str) {
    let mut body = Vec::new();
    append_u16(&mut body, linktype as u16);
    append_u16(&mut body, 0);
    append_u32(&mut body, 65_535);
    append_pcapng_option(&mut body, 2, link.as_bytes());
    append_pcapng_option(&mut body, 9, &[6]);
    append_pcapng_option(&mut body, 0, &[]);
    let block_length = 12 + body.len();
    append_u32(output, 1);
    append_u32(output, block_length as u32);
    output.extend_from_slice(&body);
    append_u32(output, block_length as u32);
}

pub fn trdp_save_capture(path: String, packets: Vec<TrdpPacket>) -> Result<(), String> {
    let mut output = Vec::new();
    append_u32(&mut output, 0x0a0d0d0a);
    append_u32(&mut output, 28);
    append_u32(&mut output, 0x1a2b3c4d);
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&u64::MAX.to_le_bytes());
    append_u32(&mut output, 28);

    let mut interfaces: Vec<(String, u32)> = Vec::new();
    for packet in &packets {
        let link = if packet.link.trim().is_empty() {
            "capture".to_string()
        } else {
            packet.link.clone()
        };
        let linktype = packet.link_type.unwrap_or(LINKTYPE_ETHERNET);
        if !interfaces
            .iter()
            .any(|item| item.0 == link && item.1 == linktype)
        {
            interfaces.push((link, linktype));
        }
    }
    if interfaces.is_empty() {
        interfaces.push(("capture".into(), LINKTYPE_ETHERNET));
    }
    for (link, linktype) in &interfaces {
        append_interface_description(&mut output, *linktype, link);
    }

    for packet in packets {
        let Some(frame) = decode_hex(&packet.raw_frame_hex) else {
            continue;
        };
        if frame.is_empty() {
            continue;
        }
        let link = if packet.link.trim().is_empty() {
            "capture"
        } else {
            packet.link.as_str()
        };
        let linktype = packet.link_type.unwrap_or(LINKTYPE_ETHERNET);
        let interface_index = interfaces
            .iter()
            .position(|item| item.0 == link && item.1 == linktype)
            .unwrap_or(0) as u32;
        let padded_length = (frame.len() + 3) & !3;
        let block_length = 32 + padded_length;
        append_u32(&mut output, 6);
        append_u32(&mut output, block_length as u32);
        append_u32(&mut output, interface_index);
        append_u32(&mut output, (packet.timestamp_us >> 32) as u32);
        append_u32(&mut output, packet.timestamp_us as u32);
        append_u32(&mut output, frame.len() as u32);
        append_u32(&mut output, frame.len() as u32);
        output.extend_from_slice(&frame);
        output.resize(output.len() + (padded_length - frame.len()), 0);
        append_u32(&mut output, block_length as u32);
    }
    fs::write(Path::new(&path), output).map_err(|error| format!("保存 pcapng 失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_ports() -> (Vec<u16>, Vec<u16>) {
        (vec![STANDARD_PD_PORT], vec![STANDARD_MD_PORT])
    }

    fn finalize_udp_ipv4(frame: &mut [u8]) {
        let ip_total_length = u16::try_from(frame.len() - 14).expect("IPv4 test frame length");
        let udp_length = u16::try_from(frame.len() - 34).expect("UDP test frame length");
        frame[16..18].copy_from_slice(&ip_total_length.to_be_bytes());
        frame[38..40].copy_from_slice(&udp_length.to_be_bytes());
    }

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
        finalize_udp_ipv4(&mut frame);
        let (pd, md) = default_ports();
        assert!(decode_frame(
            &frame,
            LINKTYPE_ETHERNET,
            0,
            CapturePorts { pd: &pd, md: &md },
        )
        .is_none());
    }

    #[test]
    fn parses_pd_header() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 44];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[239, 1, 1, 1]);
        frame[34..36].copy_from_slice(&STANDARD_PD_PORT.to_be_bytes());
        frame[36..38].copy_from_slice(&STANDARD_PD_PORT.to_be_bytes());
        let payload = 42;
        frame[payload..payload + 4].copy_from_slice(&7u32.to_be_bytes());
        frame[payload + 4..payload + 6].copy_from_slice(&0x0100u16.to_be_bytes());
        frame[payload + 6..payload + 8].copy_from_slice(b"Pd");
        frame[payload + 8..payload + 12].copy_from_slice(&1001u32.to_be_bytes());
        frame[payload + 20..payload + 24].copy_from_slice(&4u32.to_be_bytes());
        frame[payload + 40..payload + 44].copy_from_slice(&[1, 2, 3, 4]);
        let crc = trdp_crc32(&frame[payload..payload + 36]);
        frame[payload + 36..payload + 40].copy_from_slice(&crc.to_le_bytes());
        finalize_udp_ipv4(&mut frame);
        let (pd, md) = default_ports();
        let packet = decode_frame(
            &frame,
            LINKTYPE_ETHERNET,
            123,
            CapturePorts { pd: &pd, md: &md },
        )
        .expect("packet");
        assert_eq!(packet.com_id, 1001);
        assert_eq!(packet.seq_count, 7);
        assert_eq!(packet.crc_valid, Some(true));
        assert_eq!(packet.protocol_valid, Some(true));
        assert_eq!(packet.payload_hex, "01020304");
    }

    #[test]
    fn parses_md_wire_status_uuid_timeout_and_uris() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 120];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[10, 0, 0, 2]);
        frame[34..36].copy_from_slice(&STANDARD_MD_PORT.to_be_bytes());
        frame[36..38].copy_from_slice(&STANDARD_MD_PORT.to_be_bytes());
        let payload = 42;
        frame[payload..payload + 4].copy_from_slice(&9u32.to_be_bytes());
        frame[payload + 4..payload + 6].copy_from_slice(&0x0100u16.to_be_bytes());
        frame[payload + 6..payload + 8].copy_from_slice(b"Mp");
        frame[payload + 8..payload + 12].copy_from_slice(&4001u32.to_be_bytes());
        frame[payload + 20..payload + 24].copy_from_slice(&4u32.to_be_bytes());
        frame[payload + 24..payload + 28].copy_from_slice(&42u32.to_be_bytes());
        for index in 0..16 {
            frame[payload + 28 + index] = index as u8;
        }
        frame[payload + 44..payload + 48].copy_from_slice(&5_000_000u32.to_be_bytes());
        frame[payload + 48..payload + 54].copy_from_slice(b"caller");
        frame[payload + 80..payload + 87].copy_from_slice(b"replier");
        let crc = trdp_crc32(&frame[payload..payload + 112]);
        frame[payload + 112..payload + 116].copy_from_slice(&crc.to_le_bytes());
        frame[payload + 116..payload + 120].copy_from_slice(&[1, 2, 3, 4]);
        finalize_udp_ipv4(&mut frame);

        let (pd, md) = default_ports();
        let ports = CapturePorts { pd: &pd, md: &md };
        let packet = decode_frame(&frame, LINKTYPE_ETHERNET, 123, ports).expect("packet");
        assert_eq!(packet.msg_type, "Mp");
        assert_eq!(packet.com_id, 4001);
        assert_eq!(packet.crc_valid, Some(true));
        assert_eq!(packet.protocol_valid, Some(true));
        assert_eq!(packet.reply_status, Some(0));
        assert_eq!(packet.user_status, Some(42));
        assert_eq!(packet.reply_timeout_us, Some(5_000_000));
        assert_eq!(
            packet.md_session_id.as_deref(),
            Some("00010203-0405-0607-0809-0a0b0c0d0e0f")
        );
        assert_eq!(packet.src_uri.as_deref(), Some("caller"));
        assert_eq!(packet.dest_uri.as_deref(), Some("replier"));
        assert_eq!(packet.payload_hex, "01020304");

        frame[payload + 24..payload + 28].copy_from_slice(&(-6i32).to_be_bytes());
        let crc = trdp_crc32(&frame[payload..payload + 112]);
        frame[payload + 112..payload + 116].copy_from_slice(&crc.to_le_bytes());
        let error_packet =
            decode_frame(&frame, LINKTYPE_ETHERNET, 124, ports).expect("error packet");
        assert_eq!(error_packet.reply_status, Some(-6));
        assert_eq!(error_packet.user_status, Some(0));
    }

    #[test]
    fn accepts_custom_pd_port() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 40];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
        frame[30..34].copy_from_slice(&[239, 1, 1, 1]);
        frame[34..36].copy_from_slice(&18000u16.to_be_bytes());
        frame[36..38].copy_from_slice(&18000u16.to_be_bytes());
        let payload = 42;
        frame[payload + 6..payload + 8].copy_from_slice(b"Pd");
        finalize_udp_ipv4(&mut frame);
        let pd = vec![18000];
        let md = vec![STANDARD_MD_PORT];
        assert!(decode_frame(
            &frame,
            LINKTYPE_ETHERNET,
            0,
            CapturePorts { pd: &pd, md: &md },
        )
        .is_some());
    }
    #[test]
    fn pcapng_round_trip_preserves_link_provenance() {
        fn pd_frame(com_id: u32) -> Vec<u8> {
            let mut frame = vec![0u8; 14 + 20 + 8 + 44];
            frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
            frame[14] = 0x45;
            frame[23] = 17;
            frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
            frame[30..34].copy_from_slice(&[239, 1, 1, 1]);
            frame[34..36].copy_from_slice(&STANDARD_PD_PORT.to_be_bytes());
            frame[36..38].copy_from_slice(&STANDARD_PD_PORT.to_be_bytes());
            let payload = 42;
            frame[payload + 4..payload + 6].copy_from_slice(&0x0100u16.to_be_bytes());
            frame[payload + 6..payload + 8].copy_from_slice(b"Pd");
            frame[payload + 8..payload + 12].copy_from_slice(&com_id.to_be_bytes());
            frame[payload + 20..payload + 24].copy_from_slice(&4u32.to_be_bytes());
            frame[payload + 40..payload + 44].copy_from_slice(&[1, 2, 3, 4]);
            let crc = trdp_crc32(&frame[payload..payload + 36]);
            frame[payload + 36..payload + 40].copy_from_slice(&crc.to_le_bytes());
            finalize_udp_ipv4(&mut frame);
            frame
        }

        let (pd, md) = default_ports();
        let ports = CapturePorts { pd: &pd, md: &md };
        let mut a = decode_frame(&pd_frame(2001), LINKTYPE_ETHERNET, 10, ports).expect("A");
        let mut b = decode_frame(&pd_frame(2002), LINKTYPE_ETHERNET, 20, ports).expect("B");
        a.link = "A".into();
        b.link = "B".into();

        let file = tempfile::NamedTempFile::new().expect("tempfile");
        let path = file.path().to_string_lossy().to_string();
        trdp_save_capture(path.clone(), vec![a, b]).expect("save");
        let reopened = trdp_open_capture(path, None, None).expect("open");
        assert_eq!(reopened.len(), 2);
        assert_eq!(reopened[0].link, "A");
        assert_eq!(reopened[1].link, "B");
        assert_eq!(reopened[0].link_type, Some(LINKTYPE_ETHERNET));
    }
}
