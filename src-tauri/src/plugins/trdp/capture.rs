use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

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

#[derive(Debug, Clone)]
struct StoredFrame {
    link: String,
    timestamp_us: u64,
    bytes: Vec<u8>,
    link_type: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrdpCaptureResult {
    pub capture_id: String,
    pub frame_count: usize,
    pub packet_count: usize,
    pub dropped_frames: u64,
    pub packets: Vec<TrdpPacket>,
}

#[derive(Debug)]
struct StoredCapture {
    frames: Vec<StoredFrame>,
    source_path: Option<PathBuf>,
    pd_ports: Vec<u16>,
    md_ports: Vec<u16>,
    packets: Vec<TrdpPacket>,
    dropped_frames: u64,
    live: bool,
}

const LIVE_FRAME_LIMIT: usize = 50_000;
const MAX_CAPTURE_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_PCAPNG_BLOCK_BYTES: usize = 64 * 1024 * 1024;
const OPEN_PACKET_PREVIEW_LIMIT: usize = 5_000;
const TCP_FLOW_LIMIT: usize = 64;
const TCP_BUFFER_CAP: usize = 131_072;

fn capture_store() -> &'static Mutex<HashMap<String, StoredCapture>> {
    static STORE: OnceLock<Mutex<HashMap<String, StoredCapture>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn create_live_capture() -> String {
    let id = Uuid::new_v4().to_string();
    if let Ok(mut store) = capture_store().lock() {
        store.insert(
            id.clone(),
            StoredCapture {
                frames: Vec::new(),
                source_path: None,
                pd_ports: Vec::new(),
                md_ports: Vec::new(),
                packets: Vec::new(),
                dropped_frames: 0,
                live: true,
            },
        );
    }
    id
}

pub fn release_capture(capture_id: &str) {
    if let Ok(mut store) = capture_store().lock() {
        store.remove(capture_id);
    }
}

pub fn append_live_capture(
    capture_id: &str,
    link: String,
    timestamp_us: u64,
    link_type: u32,
    bytes: Vec<u8>,
    packets: Vec<TrdpPacket>,
) -> Option<(usize, usize, u64)> {
    let mut store = capture_store().lock().ok()?;
    let capture = store.get_mut(capture_id)?;
    if !capture.live {
        return None;
    }
    capture.frames.push(StoredFrame {
        link,
        timestamp_us,
        bytes,
        link_type,
    });
    if capture.frames.len() > LIVE_FRAME_LIMIT {
        let overflow = capture.frames.len() - LIVE_FRAME_LIMIT;
        capture.frames.drain(..overflow);
        capture.dropped_frames = capture.dropped_frames.saturating_add(overflow as u64);
    }
    capture.packets.extend(packets);
    if capture.packets.len() > LIVE_FRAME_LIMIT {
        let overflow = capture.packets.len() - LIVE_FRAME_LIMIT;
        capture.packets.drain(..overflow);
    }
    Some((
        capture.frames.len(),
        capture.packets.len(),
        capture.dropped_frames,
    ))
}

pub fn capture_packets(
    capture_id: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<TrdpPacket>, String> {
    let page_limit = limit.min(5_000);
    let offline = {
        let store = capture_store().lock().map_err(|error| error.to_string())?;
        let capture = store
            .get(capture_id)
            .ok_or_else(|| "TRDP capture 不存在或已释放".to_string())?;
        if let Some(path) = &capture.source_path {
            Some((
                path.clone(),
                capture.pd_ports.clone(),
                capture.md_ports.clone(),
            ))
        } else {
            let start = offset.min(capture.packets.len());
            let end = start.saturating_add(page_limit).min(capture.packets.len());
            return Ok(capture.packets[start..end].to_vec());
        }
    };

    let (path, pd_ports, md_ports) =
        offline.ok_or_else(|| "TRDP capture source unavailable".to_string())?;
    capture_packets_from_file(&path, pd_ports, md_ports, offset, page_limit)
}

#[derive(Debug, Clone)]
struct CapturePorts {
    pd: Vec<u16>,
    md: Vec<u16>,
}

impl CapturePorts {
    fn new(pd: Vec<u16>, md: Vec<u16>) -> Self {
        Self { pd, md }
    }

    fn accepts(&self, source: u16, destination: u16) -> bool {
        self.pd.contains(&source)
            || self.pd.contains(&destination)
            || self.md.contains(&source)
            || self.md.contains(&destination)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct TcpFlowKey {
    link: String,
    source_ip: u32,
    destination_ip: u32,
    source_port: u16,
    destination_port: u16,
}

#[derive(Debug, Default)]
struct TcpFlow {
    expected_sequence: u32,
    initialized: bool,
    buffer: Vec<u8>,
}

struct IpFrame<'a> {
    frame: &'a [u8],
    linktype: u32,
    timestamp_us: u64,
    link: &'a str,
    source_ip: u32,
    destination_ip: u32,
    source_text: String,
    destination_text: String,
    transport_offset: usize,
    ip_end: usize,
}

#[derive(Debug)]
pub struct TrdpStreamDecoder {
    ports: CapturePorts,
    tcp_flows: HashMap<TcpFlowKey, TcpFlow>,
}

impl TrdpStreamDecoder {
    pub fn new(pd_ports: Vec<u16>, md_ports: Vec<u16>) -> Self {
        Self {
            ports: CapturePorts::new(pd_ports, md_ports),
            tcp_flows: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.tcp_flows.clear();
    }

    pub fn feed_frame(
        &mut self,
        frame: &[u8],
        linktype: u32,
        timestamp_us: u64,
        link: &str,
    ) -> Vec<TrdpPacket> {
        let Some(ip) = network_offset(frame, linktype) else {
            return Vec::new();
        };
        if frame.get(ip).copied().unwrap_or_default() >> 4 != 4 {
            return Vec::new();
        }
        let ip_header_length = ((frame[ip] & 0x0f) as usize) * 4;
        let Some(ip_total_length) = be16(frame, ip + 2).map(usize::from) else {
            return Vec::new();
        };
        if ip_header_length < 20
            || ip_total_length < ip_header_length
            || frame.len() < ip + ip_total_length
            || be16(frame, ip + 6).is_none_or(|fragment| fragment & 0x3fff != 0)
        {
            return Vec::new();
        }

        let ip_end = ip + ip_total_length;
        let Some(source_bytes) = frame.get(ip + 12..ip + 16) else {
            return Vec::new();
        };
        let Some(destination_bytes) = frame.get(ip + 16..ip + 20) else {
            return Vec::new();
        };
        let Some(source_ip) = be32(frame, ip + 12) else {
            return Vec::new();
        };
        let Some(destination_ip) = be32(frame, ip + 16) else {
            return Vec::new();
        };
        let context = IpFrame {
            frame,
            linktype,
            timestamp_us,
            link,
            source_ip,
            destination_ip,
            source_text: ipv4(source_bytes),
            destination_text: ipv4(destination_bytes),
            transport_offset: ip + ip_header_length,
            ip_end,
        };

        match frame.get(ip + 9).copied() {
            Some(17) => self.feed_udp(&context),
            Some(6) => self.feed_tcp(&context),
            _ => Vec::new(),
        }
    }

    fn feed_udp(&self, context: &IpFrame<'_>) -> Vec<TrdpPacket> {
        if context.transport_offset + 8 > context.ip_end {
            return Vec::new();
        }
        let Some(source_port) = be16(context.frame, context.transport_offset) else {
            return Vec::new();
        };
        let Some(destination_port) = be16(context.frame, context.transport_offset + 2) else {
            return Vec::new();
        };
        let Some(udp_length) = be16(context.frame, context.transport_offset + 4).map(usize::from)
        else {
            return Vec::new();
        };
        if !self.ports.accepts(source_port, destination_port)
            || udp_length < 8
            || context.transport_offset + udp_length > context.ip_end
        {
            return Vec::new();
        }
        let payload_start = context.transport_offset + 8;
        let payload_end = context.transport_offset + udp_length;
        let Some(packet) = decode_trdp_payload(
            &context.frame[payload_start..payload_end],
            PacketOrigin {
                link: context.link,
                timestamp_us: context.timestamp_us,
                source_ip: &context.source_text,
                destination_ip: &context.destination_text,
                source_port,
                destination_port,
                transport: "udp",
                linktype: context.linktype,
            },
            true,
        ) else {
            return Vec::new();
        };
        vec![packet]
    }

    fn feed_tcp(&mut self, context: &IpFrame<'_>) -> Vec<TrdpPacket> {
        if context.transport_offset + 20 > context.ip_end {
            return Vec::new();
        }
        let Some(source_port) = be16(context.frame, context.transport_offset) else {
            return Vec::new();
        };
        let Some(destination_port) = be16(context.frame, context.transport_offset + 2) else {
            return Vec::new();
        };
        if !self.ports.accepts(source_port, destination_port) {
            return Vec::new();
        }
        let header_length = ((context.frame[context.transport_offset + 12] >> 4) as usize) * 4;
        if header_length < 20 || context.transport_offset + header_length > context.ip_end {
            return Vec::new();
        }
        let Some(sequence) = be32(context.frame, context.transport_offset + 4) else {
            return Vec::new();
        };
        let flags = context.frame[context.transport_offset + 13];
        let payload = &context.frame[context.transport_offset + header_length..context.ip_end];
        let payload_sequence = sequence.wrapping_add(u32::from(flags & 0x02 != 0));
        let key = TcpFlowKey {
            link: context.link.to_string(),
            source_ip: context.source_ip,
            destination_ip: context.destination_ip,
            source_port,
            destination_port,
        };

        if self.tcp_flows.len() >= TCP_FLOW_LIMIT && !self.tcp_flows.contains_key(&key) {
            if let Some(oldest) = self.tcp_flows.keys().next().cloned() {
                self.tcp_flows.remove(&oldest);
            }
        }
        let flow = self.tcp_flows.entry(key.clone()).or_default();
        if flags & 0x06 != 0 {
            flow.buffer.clear();
            flow.expected_sequence = payload_sequence;
            flow.initialized = true;
        }
        if !flow.initialized {
            flow.expected_sequence = payload_sequence;
            flow.initialized = true;
        }

        let mut trim = 0usize;
        if !payload.is_empty() {
            let delta = payload_sequence.wrapping_sub(flow.expected_sequence) as i32;
            if delta > 0 {
                flow.buffer.clear();
                flow.expected_sequence = payload_sequence;
            } else if delta < 0 {
                let overlap = flow.expected_sequence.wrapping_sub(payload_sequence) as usize;
                if overlap >= payload.len() {
                    trim = payload.len();
                } else {
                    trim = overlap;
                }
            }
            if trim < payload.len() {
                let append = &payload[trim..];
                if flow.buffer.len().saturating_add(append.len()) > TCP_BUFFER_CAP {
                    flow.buffer.clear();
                }
                let remaining = TCP_BUFFER_CAP.saturating_sub(flow.buffer.len());
                let append = if append.len() > remaining {
                    &append[append.len() - remaining..]
                } else {
                    append
                };
                flow.buffer.extend_from_slice(append);
                flow.expected_sequence = flow.expected_sequence.wrapping_add(append.len() as u32);
            }
        }

        let mut packets = Vec::new();
        loop {
            let start = (0..=flow.buffer.len().saturating_sub(24))
                .find(|offset| valid_md_type(&flow.buffer[*offset..]));
            let Some(start) = start else {
                if flow.buffer.len() > 23 {
                    let keep = flow.buffer.split_off(flow.buffer.len() - 23);
                    flow.buffer = keep;
                }
                break;
            };
            if start > 0 {
                flow.buffer.drain(..start);
            }
            if flow.buffer.len() < 116 {
                break;
            }
            let Some(data_length) = be32(&flow.buffer, 20).map(|value| value as usize) else {
                break;
            };
            if data_length > BRIDGE_MAX_CAPTURE_PAYLOAD {
                flow.buffer.clear();
                break;
            }
            let total = 116usize.saturating_add(data_length);
            if flow.buffer.len() < total {
                break;
            }
            let telegram = flow.buffer[..total].to_vec();
            flow.buffer.drain(..total);
            if let Some(packet) = decode_trdp_payload(
                &telegram,
                PacketOrigin {
                    link: context.link,
                    timestamp_us: context.timestamp_us,
                    source_ip: &context.source_text,
                    destination_ip: &context.destination_text,
                    source_port,
                    destination_port,
                    transport: "tcp",
                    linktype: context.linktype,
                    },
                true,
            ) {
                packets.push(packet);
            }
        }

        if flags & 0x05 != 0 {
            self.tcp_flows.remove(&key);
        }
        packets
    }
}

const BRIDGE_MAX_CAPTURE_PAYLOAD: usize = 65_536;

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

pub fn decode_raw_frame_hex(value: &str) -> Option<Vec<u8>> {
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

fn valid_md_type(data: &[u8]) -> bool {
    data.len() >= 24
        && data[6] == b'M'
        && matches!(data[7], b'n' | b'r' | b'p' | b'q' | b'c' | b'e')
}

fn valid_pd_type(data: &[u8]) -> bool {
    data.len() >= 24 && data[6] == b'P' && matches!(data[7], b'd' | b'p' | b'r' | b'e')
}

struct PacketOrigin<'a> {
    link: &'a str,
    timestamp_us: u64,
    source_ip: &'a str,
    destination_ip: &'a str,
    source_port: u16,
    destination_port: u16,
    transport: &'a str,
    linktype: u32,
}

fn decode_trdp_payload(
    trdp: &[u8],
    origin: PacketOrigin<'_>,
    allow_truncated_data: bool,
) -> Option<TrdpPacket> {
    let header_length = if valid_md_type(trdp) {
        116usize
    } else if valid_pd_type(trdp) {
        40usize
    } else {
        return None;
    };
    if trdp.len() < header_length {
        return None;
    }
    let seq_count = be32(trdp, 0)?;
    let protocol_version = be16(trdp, 4)?;
    let msg_type = String::from_utf8_lossy(trdp.get(6..8)?).to_string();
    let com_id = be32(trdp, 8)?;
    let etb_topo_count = be32(trdp, 12)?;
    let op_trn_topo_count = be32(trdp, 16)?;
    let data_len = be32(trdp, 20)?;
    let fcs_offset = header_length - 4;
    let stored_fcs = u32::from_le_bytes(trdp.get(fcs_offset..fcs_offset + 4)?.try_into().ok()?);
    let crc_valid = stored_fcs == trdp_crc32(trdp.get(..fcs_offset)?);
    let declared_end = header_length.checked_add(data_len as usize);
    let data_complete = declared_end.is_some_and(|end| end <= trdp.len());
    if !allow_truncated_data && !data_complete {
        return None;
    }
    let data_end = declared_end.unwrap_or(trdp.len()).min(trdp.len());
    let payload = trdp.get(header_length..data_end).unwrap_or_default();
    let protocol_valid = protocol_version & 0xff00 == 0x0100 && data_complete;

    let (reply_status, user_status, reply_timeout_us, md_session_id, src_uri, dest_uri) =
        if msg_type.starts_with('M') {
            let raw_reply_status = be32(trdp, 24)? as i32;
            let (reply_status, user_status) = if raw_reply_status >= 0 {
                (Some(0), Some(raw_reply_status as u16))
            } else {
                (Some(raw_reply_status), Some(0))
            };
            (
                reply_status,
                user_status,
                be32(trdp, 44),
                uuid_text(trdp.get(28..44)?),
                fixed_text(trdp.get(48..80)?),
                fixed_text(trdp.get(80..112)?),
            )
        } else {
            (None, None, None, None, None, None)
        };

    Some(TrdpPacket {
        event: "packet".into(),
        link: origin.link.to_string(),
        timestamp_us: origin.timestamp_us,
        src_ip: origin.source_ip.to_string(),
        dest_ip: origin.destination_ip.to_string(),
        src_port: origin.source_port,
        dest_port: origin.destination_port,
        transport: origin.transport.to_string(),
        msg_type,
        com_id,
        seq_count,
        protocol_version,
        etb_topo_count,
        op_trn_topo_count,
        data_len,
        payload_hex: hex(payload),
        link_type: Some(origin.linktype),
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

#[cfg(test)]
fn decode_frame(
    frame: &[u8],
    linktype: u32,
    timestamp_us: u64,
    ports: &CapturePorts,
) -> Option<TrdpPacket> {
    let mut decoder = TrdpStreamDecoder::new(ports.pd.clone(), ports.md.clone());
    decoder
        .feed_frame(frame, linktype, timestamp_us, "capture")
        .into_iter()
        .next()
}

enum CaptureRecord {
    SectionStart,
    Frame(StoredFrame),
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<bool, String> {
    let mut offset = 0usize;
    while offset < buffer.len() {
        match reader.read(&mut buffer[offset..]) {
            Ok(0) if offset == 0 => return Ok(false),
            Ok(0) => return Err("抓包文件在记录中途结束".to_string()),
            Ok(read) => offset += read,
            Err(error) => return Err(format!("读取抓包失败: {error}")),
        }
    }
    Ok(true)
}

fn visit_pcap<R: Read, F>(reader: &mut R, visitor: &mut F) -> Result<(), String>
where
    F: FnMut(CaptureRecord) -> Result<(), String>,
{
    let mut header = [0u8; 24];
    if !read_exact_or_eof(reader, &mut header)? {
        return Err("pcap 文件过短".into());
    }
    let (little_endian, nanoseconds) = match &header[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        _ => return Err("不支持的 pcap magic".into()),
    };
    let linktype = read_u32(&header, 20, little_endian).ok_or("pcap header invalid")?;
    visitor(CaptureRecord::SectionStart)?;

    loop {
        let mut record = [0u8; 16];
        if !read_exact_or_eof(reader, &mut record)? {
            break;
        }
        let seconds = read_u32(&record, 0, little_endian).unwrap_or(0) as u64;
        let fraction = read_u32(&record, 4, little_endian).unwrap_or(0) as u64;
        let captured_length =
            read_u32(&record, 8, little_endian).ok_or("pcap packet length invalid")? as usize;
        if captured_length > MAX_CAPTURE_FRAME_BYTES {
            return Err(format!(
                "pcap packet 过大: {captured_length} bytes（上限 {MAX_CAPTURE_FRAME_BYTES}）"
            ));
        }

        let mut bytes = vec![0u8; captured_length];
        if captured_length > 0 && !read_exact_or_eof(reader, &mut bytes)? {
            return Err("pcap packet data missing".into());
        }
        let timestamp_us = seconds.saturating_mul(1_000_000)
            + if nanoseconds {
                fraction / 1_000
            } else {
                fraction
            };
        visitor(CaptureRecord::Frame(StoredFrame {
            link: "capture".to_string(),
            timestamp_us,
            bytes,
            link_type: linktype,
        }))?;
    }
    Ok(())
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

fn visit_pcapng<R: Read, F>(reader: &mut R, visitor: &mut F) -> Result<(), String>
where
    F: FnMut(CaptureRecord) -> Result<(), String>,
{
    let mut little_endian = true;
    let mut have_section = false;
    let mut section_index = 0usize;
    let mut interfaces: Vec<PcapNgInterface> = Vec::new();

    loop {
        let mut prefix = [0u8; 8];
        if !read_exact_or_eof(reader, &mut prefix)? {
            break;
        }
        let section_header = prefix[..4] == [0x0a, 0x0d, 0x0d, 0x0a];
        let mut initial = Vec::with_capacity(12);
        initial.extend_from_slice(&prefix);

        if section_header {
            let mut byte_order_magic = [0u8; 4];
            if !read_exact_or_eof(reader, &mut byte_order_magic)? {
                return Err("pcapng Section Header 缺少 byte-order magic".into());
            }
            initial.extend_from_slice(&byte_order_magic);
            little_endian = match byte_order_magic {
                [0x4d, 0x3c, 0x2b, 0x1a] => true,
                [0x1a, 0x2b, 0x3c, 0x4d] => false,
                _ => return Err("pcapng byte-order magic invalid".into()),
            };
            have_section = true;
        } else if !have_section {
            return Err("pcapng 缺少 Section Header".into());
        }

        let block_length =
            read_u32(&initial, 4, little_endian).ok_or("pcapng length invalid")? as usize;
        if block_length < 12
            || !block_length.is_multiple_of(4)
            || block_length > MAX_PCAPNG_BLOCK_BYTES
        {
            return Err(format!("pcapng block length invalid: {block_length}"));
        }
        if initial.len() > block_length {
            return Err("pcapng block length smaller than header".into());
        }

        let mut block = vec![0u8; block_length];
        block[..initial.len()].copy_from_slice(&initial);
        if block_length > initial.len() && !read_exact_or_eof(reader, &mut block[initial.len()..])?
        {
            return Err("pcapng block truncated".into());
        }
        let trailing_length = read_u32(&block, block_length - 4, little_endian)
            .ok_or("pcapng trailing length invalid")? as usize;
        if trailing_length != block_length {
            return Err("pcapng block length mismatch".into());
        }

        let block_type = if section_header {
            0x0a0d0d0a
        } else {
            read_u32(&block, 0, little_endian).ok_or("pcapng block invalid")?
        };
        match block_type {
            0x0a0d0d0a => {
                interfaces.clear();
                visitor(CaptureRecord::SectionStart)?;
                section_index = section_index.saturating_add(1);
            }
            1 if block_length >= 20 => {
                let linktype = read_u16(&block, 8, little_endian).unwrap_or(1) as u32;
                let (timestamp_resolution, timestamp_base2, link) =
                    parse_idb_options(&block, 0, block_length, little_endian);
                let interface_index = interfaces.len();
                interfaces.push(PcapNgInterface {
                    linktype,
                    timestamp_resolution,
                    timestamp_base2,
                    link: link.unwrap_or_else(|| {
                        format!(
                            "capture:{}:{}",
                            section_index.saturating_sub(1),
                            interface_index
                        )
                    }),
                });
            }
            6 if block_length >= 32 => {
                let interface_index = read_u32(&block, 8, little_endian).unwrap_or(0) as usize;
                let timestamp_high = read_u32(&block, 12, little_endian).unwrap_or(0) as u64;
                let timestamp_low = read_u32(&block, 16, little_endian).unwrap_or(0) as u64;
                let captured_length = read_u32(&block, 20, little_endian).unwrap_or(0) as usize;
                if captured_length > MAX_CAPTURE_FRAME_BYTES {
                    return Err(format!(
                        "pcapng packet 过大: {captured_length} bytes（上限 {MAX_CAPTURE_FRAME_BYTES}）"
                    ));
                }
                let packet_start = 28usize;
                if packet_start.saturating_add(captured_length) > block_length - 4 {
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
                            link: format!(
                                "capture:{}:{}",
                                section_index.saturating_sub(1),
                                interface_index
                            ),
                        });
                let raw_timestamp = (timestamp_high << 32) | timestamp_low;
                let timestamp_us = pcapng_timestamp_to_us(
                    raw_timestamp,
                    interface.timestamp_resolution,
                    interface.timestamp_base2,
                );
                visitor(CaptureRecord::Frame(StoredFrame {
                    link: interface.link,
                    timestamp_us,
                    bytes: block[packet_start..packet_start + captured_length].to_vec(),
                    link_type: interface.linktype,
                }))?;
            }
            3 if block_length >= 16 => {
                let interface = interfaces.first().cloned().unwrap_or(PcapNgInterface {
                    linktype: LINKTYPE_ETHERNET,
                    timestamp_resolution: 6,
                    timestamp_base2: false,
                    link: format!("capture:{}:0", section_index.saturating_sub(1)),
                });
                let original_length = read_u32(&block, 8, little_endian).unwrap_or(0) as usize;
                let available = block_length.saturating_sub(16);
                let captured_length = original_length.min(available);
                if captured_length > MAX_CAPTURE_FRAME_BYTES {
                    return Err(format!(
                        "pcapng simple packet 过大: {captured_length} bytes"
                    ));
                }
                visitor(CaptureRecord::Frame(StoredFrame {
                    link: interface.link,
                    timestamp_us: 0,
                    bytes: block[12..12 + captured_length].to_vec(),
                    link_type: interface.linktype,
                }))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn visit_capture_file<F>(path: &Path, mut visitor: F) -> Result<(), String>
where
    F: FnMut(CaptureRecord) -> Result<(), String>,
{
    let file = File::open(path).map_err(|error| format!("读取抓包失败: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut magic = [0u8; 4];
    if !read_exact_or_eof(&mut reader, &mut magic)? {
        return Err("抓包文件为空".into());
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("抓包 seek 失败: {error}"))?;
    if magic == [0x0a, 0x0d, 0x0d, 0x0a] {
        visit_pcapng(&mut reader, &mut visitor)
    } else {
        visit_pcap(&mut reader, &mut visitor)
    }
}

fn capture_packets_from_file(
    path: &Path,
    pd_ports: Vec<u16>,
    md_ports: Vec<u16>,
    offset: usize,
    limit: usize,
) -> Result<Vec<TrdpPacket>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut decoder = TrdpStreamDecoder::new(pd_ports, md_ports);
    let end = offset.saturating_add(limit);
    let mut index = 0usize;
    let mut page = Vec::with_capacity(limit);
    visit_capture_file(path, |record| {
        match record {
            CaptureRecord::SectionStart => decoder.reset(),
            CaptureRecord::Frame(frame) => {
                for packet in decoder.feed_frame(
                    &frame.bytes,
                    frame.link_type,
                    frame.timestamp_us,
                    &frame.link,
                ) {
                    if index >= offset && index < end {
                        page.push(packet);
                    }
                    index = index.saturating_add(1);
                }
            }
        }
        Ok(())
    })?;
    Ok(page)
}

pub fn trdp_open_capture(
    path: String,
    pd_ports: Option<Vec<u16>>,
    md_ports: Option<Vec<u16>>,
) -> Result<TrdpCaptureResult, String> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.is_file() {
        return Err(format!("抓包文件不存在: {}", path_buf.display()));
    }
    let ports = CapturePorts::new(
        pd_ports.unwrap_or_else(|| vec![STANDARD_PD_PORT]),
        md_ports.unwrap_or_else(|| vec![STANDARD_MD_PORT]),
    );
    let mut decoder = TrdpStreamDecoder::new(ports.pd.clone(), ports.md.clone());
    let mut packet_preview = std::collections::VecDeque::with_capacity(OPEN_PACKET_PREVIEW_LIMIT);
    let mut packet_count = 0usize;
    let mut frame_count = 0usize;
    visit_capture_file(&path_buf, |record| {
        match record {
            CaptureRecord::SectionStart => decoder.reset(),
            CaptureRecord::Frame(frame) => {
                frame_count = frame_count.saturating_add(1);
                for packet in decoder.feed_frame(
                    &frame.bytes,
                    frame.link_type,
                    frame.timestamp_us,
                    &frame.link,
                ) {
                    packet_count = packet_count.saturating_add(1);
                    if packet_preview.len() == OPEN_PACKET_PREVIEW_LIMIT {
                        packet_preview.pop_front();
                    }
                    packet_preview.push_back(packet);
                }
            }
        }
        Ok(())
    })?;
    let preview = packet_preview.into_iter().collect::<Vec<_>>();

    let capture_id = Uuid::new_v4().to_string();
    let result = TrdpCaptureResult {
        capture_id: capture_id.clone(),
        frame_count,
        packet_count,
        dropped_frames: 0,
        packets: preview.clone(),
    };
    capture_store()
        .lock()
        .map_err(|error| error.to_string())?
        .insert(
            capture_id,
            StoredCapture {
                frames: Vec::new(),
                source_path: Some(path_buf),
                pd_ports: ports.pd,
                md_ports: ports.md,
                packets: preview,
                dropped_frames: 0,
                live: false,
            },
        );
    Ok(result)
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

struct PcapNgWriter {
    writer: BufWriter<File>,
    interfaces: Vec<(String, u32)>,
}

impl PcapNgWriter {
    fn new(path: &Path) -> Result<Self, String> {
        let file = File::create(path).map_err(|error| format!("创建 pcapng 失败: {error}"))?;
        let mut writer = Self {
            writer: BufWriter::new(file),
            interfaces: Vec::new(),
        };
        let mut header = Vec::with_capacity(28);
        append_u32(&mut header, 0x0a0d0d0a);
        append_u32(&mut header, 28);
        append_u32(&mut header, 0x1a2b3c4d);
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&u64::MAX.to_le_bytes());
        append_u32(&mut header, 28);
        writer
            .writer
            .write_all(&header)
            .map_err(|error| format!("写入 pcapng header 失败: {error}"))?;
        Ok(writer)
    }

    fn ensure_interface(&mut self, link: &str, linktype: u32) -> Result<u32, String> {
        if let Some(index) = self
            .interfaces
            .iter()
            .position(|item| item.0 == link && item.1 == linktype)
        {
            return Ok(index as u32);
        }

        let mut body = Vec::new();
        append_u16(&mut body, linktype as u16);
        append_u16(&mut body, 0);
        append_u32(&mut body, 65_535);
        append_pcapng_option(&mut body, 2, link.as_bytes());
        append_pcapng_option(&mut body, 9, &[6]);
        append_pcapng_option(&mut body, 0, &[]);
        let block_length = 12 + body.len();
        let mut block = Vec::with_capacity(block_length);
        append_u32(&mut block, 1);
        append_u32(&mut block, block_length as u32);
        block.extend_from_slice(&body);
        append_u32(&mut block, block_length as u32);
        self.writer
            .write_all(&block)
            .map_err(|error| format!("写入 pcapng interface 失败: {error}"))?;
        self.interfaces.push((link.to_string(), linktype));
        Ok((self.interfaces.len() - 1) as u32)
    }

    fn write_frame(&mut self, raw: &StoredFrame) -> Result<(), String> {
        if raw.bytes.is_empty() {
            return Ok(());
        }
        let link = if raw.link.trim().is_empty() {
            "capture"
        } else {
            raw.link.as_str()
        };
        let interface_index = self.ensure_interface(link, raw.link_type)?;
        let padded_length = (raw.bytes.len() + 3) & !3;
        let block_length = 32 + padded_length;
        let mut header = Vec::with_capacity(28);
        append_u32(&mut header, 6);
        append_u32(&mut header, block_length as u32);
        append_u32(&mut header, interface_index);
        append_u32(&mut header, (raw.timestamp_us >> 32) as u32);
        append_u32(&mut header, raw.timestamp_us as u32);
        append_u32(&mut header, raw.bytes.len() as u32);
        append_u32(&mut header, raw.bytes.len() as u32);
        self.writer
            .write_all(&header)
            .and_then(|_| self.writer.write_all(&raw.bytes))
            .map_err(|error| format!("写入 pcapng frame 失败: {error}"))?;
        let padding = padded_length - raw.bytes.len();
        if padding > 0 {
            self.writer
                .write_all(&[0u8; 3][..padding])
                .map_err(|error| format!("写入 pcapng padding 失败: {error}"))?;
        }
        self.writer
            .write_all(&(block_length as u32).to_le_bytes())
            .map_err(|error| format!("写入 pcapng block footer 失败: {error}"))
    }

    fn finish(mut self) -> Result<(), String> {
        if self.interfaces.is_empty() {
            let _ = self.ensure_interface("capture", LINKTYPE_ETHERNET)?;
        }
        self.writer
            .flush()
            .map_err(|error| format!("刷新 pcapng 失败: {error}"))
    }
}

fn save_frames(path: &Path, frames: Vec<StoredFrame>) -> Result<(), String> {
    let mut writer = PcapNgWriter::new(path)?;
    for frame in &frames {
        writer.write_frame(frame)?;
    }
    writer.finish()
}

fn convert_capture_to_pcapng(source: &Path, destination: &Path) -> Result<(), String> {
    let mut writer = PcapNgWriter::new(destination)?;
    visit_capture_file(source, |record| {
        if let CaptureRecord::Frame(frame) = record {
            writer.write_frame(&frame)?;
        }
        Ok(())
    })?;
    writer.finish()
}

pub fn trdp_save_capture(path: String, capture_id: String) -> Result<(), String> {
    enum SaveSource {
        Live(Vec<StoredFrame>),
        Offline(PathBuf),
    }

    let source = {
        let store = capture_store().lock().map_err(|error| error.to_string())?;
        let capture = store
            .get(&capture_id)
            .ok_or_else(|| "TRDP capture 不存在或已释放".to_string())?;
        match &capture.source_path {
            Some(path) => SaveSource::Offline(path.clone()),
            None => SaveSource::Live(capture.frames.clone()),
        }
    };

    let destination = PathBuf::from(path);
    match source {
        SaveSource::Live(frames) => save_frames(&destination, frames),
        SaveSource::Offline(source) => {
            let same_file = fs::canonicalize(&source)
                .ok()
                .zip(fs::canonicalize(&destination).ok())
                .is_some_and(|(left, right)| left == right);
            if !same_file {
                return convert_capture_to_pcapng(&source, &destination);
            }

            let file_name = destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("capture.pcapng");
            let token = Uuid::new_v4();
            let temporary = destination.with_file_name(format!(".{file_name}.tauterm-{token}.tmp"));
            let backup = destination.with_file_name(format!(".{file_name}.tauterm-{token}.backup"));

            convert_capture_to_pcapng(&source, &temporary)?;
            fs::rename(&destination, &backup)
                .map_err(|error| format!("暂存原抓包失败: {error}"))?;
            match fs::rename(&temporary, &destination) {
                Ok(()) => {
                    let _ = fs::remove_file(&backup);
                    Ok(())
                }
                Err(error) => {
                    let restore = fs::rename(&backup, &destination);
                    let _ = fs::remove_file(&temporary);
                    match restore {
                        Ok(()) => Err(format!(
                            "写回 pcapng 失败，原抓包已恢复: {error}"
                        )),
                        Err(restore_error) => Err(format!(
                            "写回 pcapng 失败且自动恢复失败: {error}; 原文件保留于 {} ({restore_error})",
                            backup.display()
                        )),
                    }
                }
            }
        }
    }
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
            &CapturePorts::new(pd.clone(), md.clone()),
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
            &CapturePorts::new(pd.clone(), md.clone()),
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
        let ports = CapturePorts::new(pd.clone(), md.clone());
        let packet = decode_frame(&frame, LINKTYPE_ETHERNET, 123, &ports).expect("packet");
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
            decode_frame(&frame, LINKTYPE_ETHERNET, 124, &ports).expect("error packet");
        assert_eq!(error_packet.reply_status, Some(-6));
        assert_eq!(error_packet.user_status, Some(0));
    }

    #[test]
    fn marks_truncated_trdp_payload_as_protocol_invalid() {
        let mut frame = vec![0u8; 14 + 20 + 8 + 42];
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
        frame[payload + 8..payload + 12].copy_from_slice(&1001u32.to_be_bytes());
        frame[payload + 20..payload + 24].copy_from_slice(&4u32.to_be_bytes());
        let crc = trdp_crc32(&frame[payload..payload + 36]);
        frame[payload + 36..payload + 40].copy_from_slice(&crc.to_le_bytes());
        frame[payload + 40..payload + 42].copy_from_slice(&[1, 2]);
        finalize_udp_ipv4(&mut frame);

        let (pd, md) = default_ports();
        let packet = decode_frame(
            &frame,
            LINKTYPE_ETHERNET,
            0,
            &CapturePorts::new(pd.clone(), md.clone()),
        )
        .expect("truncated packet remains inspectable");
        assert_eq!(packet.crc_valid, Some(true));
        assert_eq!(packet.protocol_valid, Some(false));
        assert_eq!(packet.payload_hex, "0102");
        assert_eq!(packet.data_len, 4);
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
            &CapturePorts::new(pd.clone(), md.clone()),
        )
        .is_some());
    }
    #[test]
    fn reassembles_segmented_md_tcp_with_the_canonical_decoder() {
        fn tcp_frame(sequence: u32, payload: &[u8]) -> Vec<u8> {
            let mut frame = vec![0u8; 14 + 20 + 20 + payload.len()];
            frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
            frame[14] = 0x45;
            let ip_total = u16::try_from(20 + 20 + payload.len()).expect("ip length");
            frame[16..18].copy_from_slice(&ip_total.to_be_bytes());
            frame[23] = 6;
            frame[26..30].copy_from_slice(&[10, 0, 0, 1]);
            frame[30..34].copy_from_slice(&[10, 0, 0, 2]);
            frame[34..36].copy_from_slice(&STANDARD_MD_PORT.to_be_bytes());
            frame[36..38].copy_from_slice(&STANDARD_MD_PORT.to_be_bytes());
            frame[38..42].copy_from_slice(&sequence.to_be_bytes());
            frame[46] = 0x50;
            frame[47] = 0x18;
            frame[54..].copy_from_slice(payload);
            frame
        }

        let mut telegram = [0u8; 120];
        telegram[0..4].copy_from_slice(&17u32.to_be_bytes());
        telegram[4..6].copy_from_slice(&0x0100u16.to_be_bytes());
        telegram[6..8].copy_from_slice(b"Mp");
        telegram[8..12].copy_from_slice(&7201u32.to_be_bytes());
        telegram[20..24].copy_from_slice(&4u32.to_be_bytes());
        for (index, byte) in telegram[28..44].iter_mut().enumerate() {
            *byte = index as u8;
        }
        let crc = trdp_crc32(&telegram[..112]);
        telegram[112..116].copy_from_slice(&crc.to_le_bytes());
        telegram[116..120].copy_from_slice(&[1, 2, 3, 4]);

        let mut decoder = TrdpStreamDecoder::new(vec![STANDARD_PD_PORT], vec![STANDARD_MD_PORT]);
        let first = tcp_frame(1_000, &telegram[..60]);
        let second = tcp_frame(1_060, &telegram[60..]);
        assert!(decoder
            .feed_frame(&first, LINKTYPE_ETHERNET, 10, "A")
            .is_empty());
        let packets = decoder.feed_frame(&second, LINKTYPE_ETHERNET, 20, "A");
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].transport, "tcp");
        assert_eq!(packets[0].msg_type, "Mp");
        assert_eq!(packets[0].com_id, 7201);
        assert_eq!(packets[0].payload_hex, "01020304");
        assert_eq!(packets[0].link, "A");
        assert_eq!(packets[0].crc_valid, Some(true));
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
        let ports = CapturePorts::new(pd.clone(), md.clone());
        let frame_a = pd_frame(2001);
        let frame_b = pd_frame(2002);
        let mut a = decode_frame(&frame_a, LINKTYPE_ETHERNET, 10, &ports).expect("A");
        let mut b = decode_frame(&frame_b, LINKTYPE_ETHERNET, 20, &ports).expect("B");
        a.link = "A".into();
        b.link = "B".into();

        let file = tempfile::NamedTempFile::new().expect("tempfile");
        let path = file.path().to_string_lossy().to_string();
        let frames = vec![
            StoredFrame {
                link: a.link.clone(),
                timestamp_us: a.timestamp_us,
                bytes: frame_a,
                link_type: a.link_type.unwrap_or(LINKTYPE_ETHERNET),
            },
            StoredFrame {
                link: b.link.clone(),
                timestamp_us: b.timestamp_us,
                bytes: frame_b,
                link_type: b.link_type.unwrap_or(LINKTYPE_ETHERNET),
            },
        ];
        save_frames(Path::new(&path), frames).expect("save");
        let reopened = trdp_open_capture(path, None, None).expect("open");
        assert_eq!(reopened.packet_count, 2);
        assert_eq!(reopened.packets[0].link, "A");
        assert_eq!(reopened.packets[1].link, "B");
        assert_eq!(reopened.packets[0].link_type, Some(LINKTYPE_ETHERNET));
    }
}
