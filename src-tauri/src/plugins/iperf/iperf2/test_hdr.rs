//! iperf2 协议结构（老架构，2.1.x/2.2.x 兼容）——字节级序列化，全部网络序
//!
//! 依据 iperf 2.2.1 官方源码（include/payloads.h / src/Settings.cpp）实现：
//! - 控制连接：客户端发 `client_hdr_v1`（24B，无 magic）；服务器对 TCP 测试
//!   回 `client_hdr_ack`（flags 含 HEADER_EXTEND 时）
//! - 数据连接（TCP）：首包 = `client_hdr_v1` + `client_hdrext`（64B），之后纯数据
//! - UDP 数据包：`UDP_datagram`（16B）+ `client_hdr_v1` + `client_hdrext` + 载荷
//!   （UDP 无状态，每包携带测试头）
//! - 服务器统计回报：`server_hdr`（UDP 结束包 = `UDP_datagram` + `server_hdr`）
//!
//! 所有字段 4 字节对齐 packed（C 结构 `#pragma pack(push,4)` 对应）。

// ── 常量（对齐 2.2.1 include/payloads.h） ─────────────

/// flags 高位：v1 头存在（2.2.1 普通 kTest_Normal 测试不设；仅 -d/-r 模式）
pub const HEADER_VERSION1: u32 = 0x8000_0000;
/// flags 低位：RUN_NOW（2.2.1 payloads.h:91；-d dualtest 时随 VERSION1 置位，
/// 服务端解析后立即反向 connect 客户端监听端口）
pub const HEADER_RUN_NOW: u32 = 0x0000_0001;
/// flags：存在 client_hdrext 扩展（普通测试必设）
pub const HEADER_EXTEND: u32 = 0x4000_0000;
/// flags：UDP 测试（2.2.1 普通 UDP 测试不设；仅 L2/isoch/L4S/反向等模式）
#[allow(dead_code)] // 特殊模式预留
pub const HEADER_UDPTESTS: u32 = 0x2000_0000;
/// flags：64 位包序号（UDP 默认设置）
pub const HEADER_SEQNO64B: u32 = 0x0800_0000;
/// flags：v2 头（双向/反向等）
#[allow(dead_code)] // -d/-r 下轮实现
pub const HEADER_VERSION2: u32 = 0x0400_0000;
/// flags：测试头长度以 (flags & LEN_MASK) >> 1 编码
pub const HEADER_LEN_BIT: u32 = 0x0001_0000;
/// 测试头长度掩码（还原需 >>1）
pub const HEADER_LEN_MASK: u32 = 0x0000_01FE;
/// 服务器接受的测试头最大长度（payloads.h MAX_HEADER_LEN）
pub const MAX_HEADER_LEN: usize = 256;

/// 消息类型（MsgType 枚举）
#[allow(dead_code)] // 预留
pub const MSG_CLIENTHDR: i32 = 0x1;
pub const MSG_CLIENTHDRACK: i32 = 0x2;
#[allow(dead_code)] // 2.2.1 客户端对 typelen 零填充，不再使用
pub const MSG_CLIENTTCPHDR: i32 = 0x3;
#[allow(dead_code)] // 预留
pub const MSG_SERVERHDR: i32 = 0x4;

/// TCP 首包测试头长度（v1 24 + extend 40）
pub const TCP_FIRST_PAYLOAD_LEN: usize = 24 + 40;
/// UDP 每包测试头长度（UDP_datagram 16 + v1 24 + extend 40；flags 长度字段计入）
pub const UDP_PACKET_HDR_LEN: usize = 16 + 24 + 40;

/// 本实现声明的 iperf 版本 hex（对齐 2.2.1：IPERF_VERSION_MAJORHEX / MINORHEX）
pub const OUR_VERSION_MAJOR_HEX: u32 = 0x0002_0002;
pub const OUR_VERSION_MINOR_HEX: u32 = 0x0001_0001;

/// 默认数据缓冲区长度（-l 未指定时服务器端使用的默认 UDP 载荷）
pub const DEFAULT_BUF_LEN: i32 = 0; // 0 = 服务器用默认/MTU 探测

// ── client_hdr_v1（24B） ─────────────────────────────

/// 客户端基础测试头（TCP 首包/UDP 每包携带）
#[derive(Debug, Clone)]
pub struct ClientHdrV1 {
    /// flags 位图（高位版本/扩展标志）
    pub flags: u32,
    pub num_threads: i32,
    /// 端口（普通测试 = 服务器控制端口；-d/-r 时为客户端监听端口）
    pub m_port: i32,
    /// 缓冲区长度（-l；0 = 未设置）
    pub m_buf_len: i32,
    /// 窗口/带宽组合字段（2.2.1 从不写入，恒为 0）
    pub m_win_band: i32,
    /// 测试量（10ms 单位；time 模式为负值 = -(秒 × 100)）
    pub m_amount: i32,
}

impl ClientHdrV1 {
    /// 构造客户端测试头
    ///
    /// 对齐 2.2.1 `Settings_GenerateClientHdr`：
    /// - TCP 普通测试 flags = `EXTEND | LEN_BIT | ((64<<1) & MASK)` = `0x4001_0080`
    /// - UDP 普通测试 flags = `EXTEND | SEQNO64B | LEN_BIT | ((80<<1) & MASK)` = `0x4801_00A0`
    ///   （长度字段计入 UDP_datagram 16B；普通测试不设 VERSION1/UDPTESTS）
    /// - `time_secs`: 测试时长秒（mAmount = -(secs × 100)，time 模式）
    pub fn new_client(udp: bool, num_threads: u32, port: u16, time_secs: u32) -> Self {
        let mut flags = HEADER_EXTEND | HEADER_LEN_BIT;
        let hdr_len = if udp {
            flags |= HEADER_SEQNO64B;
            UDP_PACKET_HDR_LEN
        } else {
            TCP_FIRST_PAYLOAD_LEN
        };
        flags |= ((hdr_len as u32) << 1) & HEADER_LEN_MASK;
        Self {
            flags,
            num_threads: num_threads as i32,
            m_port: port as i32,
            m_buf_len: DEFAULT_BUF_LEN,
            m_win_band: 0,
            // 防御：i32 溢出上限为 ~2147 万秒（约 248 天），远超 UI 允许范围，仍截断保护
            m_amount: -(time_secs.min(i32::MAX as u32 / 100) as i32 * 100),
        }
    }

    /// 附加方向模式 flags（对齐 2.2.1 Settings.cpp:2689 附近：非普通模式
    /// `flags |= HEADER_VERSION1`，DualTest 另加 `RUN_NOW`）
    pub fn with_direction(mut self, direction: super::types::TestDirection) -> Self {
        self.flags |= direction.header_flags();
        self
    }

    /// 服务端依据 flags 判定接待模式（对齐 2.2.1 Listener.cpp:920-931：
    /// VERSION1 + RUN_NOW → DualTest；VERSION1 无 RUN_NOW → TradeOff；
    /// 无 VERSION1 → Normal）
    pub fn server_mode(&self) -> super::types::ServerTestMode {
        if self.flags & HEADER_VERSION1 == 0 {
            super::types::ServerTestMode::Normal
        } else if self.flags & HEADER_RUN_NOW != 0 {
            super::types::ServerTestMode::DualTest
        } else {
            super::types::ServerTestMode::TradeOff
        }
    }

    /// 序列化（24B，网络序）
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(24);
        for v in [
            self.flags,
            self.num_threads as u32,
            self.m_port as u32,
            self.m_buf_len as u32,
            self.m_win_band as u32,
            self.m_amount as u32,
        ] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf
    }

    /// 反序列化（至少 24B）
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < 24 {
            return Err(format!("client_hdr_v1 长度不足: {} < 24", data.len()));
        }
        let rd = |off: usize| i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            flags: u32::from_be_bytes(data[0..4].try_into().unwrap()),
            num_threads: rd(4),
            m_port: rd(8),
            m_buf_len: rd(12),
            m_win_band: rd(16),
            m_amount: rd(20),
        })
    }

    /// 是否 time 模式（mAmount 高位为 1）
    pub fn is_time_mode(&self) -> bool {
        self.m_amount < 0
    }

    /// time 模式下的测试秒数
    ///
    /// 饱和处理：`m_amount = i32::MIN` 时取负会溢出（debug 构建 panic，且该
    /// 字段来自网络对端可远程触发）——经 i64 中间值换算，上限约 2147 万秒
    /// （~248 天，与真实 2.2.1 release 回绕行为一致）。
    pub fn time_secs(&self) -> u32 {
        if self.is_time_mode() {
            (i64::from(self.m_amount).unsigned_abs() / 100) as u32
        } else {
            0
        }
    }

    pub fn is_udp(&self) -> bool {
        self.flags & HEADER_UDPTESTS != 0
    }
}

// ── client_hdrext（40B） ─────────────────────────────

/// 客户端扩展头（flags 含 HEADER_EXTEND 时随 v1 发送）
#[derive(Debug, Clone)]
pub struct ClientHdrExt {
    pub typelen_type: i32,
    pub typelen_length: i32,
    pub upperflags: u16,
    pub lowerflags: u16,
    pub version_u: u32,
    pub version_l: u32,
    pub reserved: u16,
    pub tos: u16,
    pub l_rate: u32,
    pub u_rate: u32,
    pub tcp_write_prefetch: u32,
    pub barrier_usecs: u32,
}

impl ClientHdrExt {
    pub const SIZE: usize = 40;

    /// 构造默认扩展头（版本 + 可选带宽）
    pub fn new_client(bandwidth_bps: Option<u64>) -> Self {
        let (l_rate, u_rate) = match bandwidth_bps {
            Some(bw) => (bw as u32, ((bw >> 32) as u32) << 8),
            None => (0, 0),
        };
        Self {
            // 2.2.1 客户端对 typelen 零填充（memset 后不写）
            typelen_type: 0,
            typelen_length: 0,
            upperflags: 0,
            lowerflags: 0,
            version_u: OUR_VERSION_MAJOR_HEX,
            version_l: OUR_VERSION_MINOR_HEX,
            reserved: 0,
            tos: 0,
            l_rate,
            u_rate,
            tcp_write_prefetch: 0,
            barrier_usecs: 0,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.typelen_type.to_be_bytes());
        buf.extend_from_slice(&self.typelen_length.to_be_bytes());
        buf.extend_from_slice(&self.upperflags.to_be_bytes());
        buf.extend_from_slice(&self.lowerflags.to_be_bytes());
        buf.extend_from_slice(&self.version_u.to_be_bytes());
        buf.extend_from_slice(&self.version_l.to_be_bytes());
        buf.extend_from_slice(&self.reserved.to_be_bytes());
        buf.extend_from_slice(&self.tos.to_be_bytes());
        buf.extend_from_slice(&self.l_rate.to_be_bytes());
        buf.extend_from_slice(&self.u_rate.to_be_bytes());
        buf.extend_from_slice(&self.tcp_write_prefetch.to_be_bytes());
        buf.extend_from_slice(&self.barrier_usecs.to_be_bytes());
        buf
    }

    #[allow(dead_code)] // 预留：服务端解析 TCP 首包
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < Self::SIZE {
            return Err(format!("client_hdrext 长度不足: {} < {}", data.len(), Self::SIZE));
        }
        let rd = |off: usize| i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            typelen_type: rd(0),
            typelen_length: rd(4),
            upperflags: u16::from_be_bytes(data[8..10].try_into().unwrap()),
            lowerflags: u16::from_be_bytes(data[10..12].try_into().unwrap()),
            version_u: u32::from_be_bytes(data[12..16].try_into().unwrap()),
            version_l: u32::from_be_bytes(data[16..20].try_into().unwrap()),
            reserved: u16::from_be_bytes(data[20..22].try_into().unwrap()),
            tos: u16::from_be_bytes(data[22..24].try_into().unwrap()),
            l_rate: u32::from_be_bytes(data[24..28].try_into().unwrap()),
            u_rate: u32::from_be_bytes(data[28..32].try_into().unwrap()),
            tcp_write_prefetch: u32::from_be_bytes(data[32..36].try_into().unwrap()),
            barrier_usecs: u32::from_be_bytes(data[36..40].try_into().unwrap()),
        })
    }
}

impl ClientHdrV1 {
    #[allow(dead_code)] // typelen 零填充后不再引用（保留作文档性常量）
    pub const fn serialize_size() -> usize {
        24
    }
}

// ── client_hdr_ack（24B） ────────────────────────────

/// 服务器回执（TCP 测试 + flags&EXTEND 时发送；携带服务器版本）
#[derive(Debug, Clone)]
pub struct ClientHdrAck {
    pub typelen_type: i32,
    pub typelen_length: i32,
    pub flags: u32,
    pub version_u: u32,
    pub version_l: u32,
    pub reserved1: u32,
    pub reserved2: u32,
}

impl ClientHdrAck {
    /// 非 trip-time 路径 28B（typelen 8 + flags/version_u/version_l/reserved1/reserved2 各 4）
    pub const SIZE: usize = 28;

    pub fn new_server() -> Self {
        Self {
            typelen_type: MSG_CLIENTHDRACK,
            typelen_length: Self::SIZE as i32,
            flags: 0,
            version_u: OUR_VERSION_MAJOR_HEX,
            version_l: OUR_VERSION_MINOR_HEX,
            reserved1: 0,
            reserved2: 0,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        for v in [
            self.typelen_type,
            self.typelen_length,
            self.flags as i32,
            self.version_u as i32,
            self.version_l as i32,
            self.reserved1 as i32,
            self.reserved2 as i32,
        ] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < Self::SIZE {
            return Err(format!("client_hdr_ack 长度不足: {} < {}", data.len(), Self::SIZE));
        }
        let rd = |off: usize| i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            typelen_type: rd(0),
            typelen_length: rd(4),
            flags: rd(8) as u32,
            version_u: rd(12) as u32,
            version_l: rd(16) as u32,
            reserved1: rd(20) as u32,
            reserved2: rd(24) as u32,
        })
    }

    pub fn is_ack(&self) -> bool {
        self.typelen_type == MSG_CLIENTHDRACK
    }
}

// ── UDP_datagram（16B） ──────────────────────────────

/// UDP 数据包头（网络序；id 低 32 位 + id2 高 32 位 + 发送时间戳）
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    pub id: u32,
    pub tv_sec: u32,
    pub tv_usec: u32,
    pub id2: u32,
}

impl UdpDatagram {
    pub const SIZE: usize = 16;

    /// 构造（id 低 32 位 + 高 32 位）
    pub fn new(seqno: u64) -> Self {
        Self {
            id: seqno as u32,
            tv_sec: 0,
            tv_usec: 0,
            id2: (seqno >> 32) as u32,
        }
    }

    /// 填充发送时间戳
    pub fn stamp(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        self.tv_sec = now.as_secs() as u32;
        self.tv_usec = now.subsec_micros();
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        for v in [self.id, self.tv_sec, self.tv_usec, self.id2] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < Self::SIZE {
            return Err(format!("UDP_datagram 长度不足: {} < {}", data.len(), Self::SIZE));
        }
        let rd = |off: usize| u32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            id: rd(0),
            tv_sec: rd(4),
            tv_usec: rd(8),
            id2: rd(12),
        })
    }

    /// 64 位序号
    pub fn seqno(&self) -> u64 {
        ((self.id2 as u64) << 32) | self.id as u64
    }

    /// 发送时间戳（毫秒）
    pub fn send_ms(&self) -> f64 {
        self.tv_sec as f64 * 1000.0 + self.tv_usec as f64 / 1000.0
    }
}

// ── server_hdr（服务器统计回报，40B） ─────────────────

/// 服务器统计头（UDP 结束包 = UDP_datagram + server_hdr_v1）
///
/// 对齐 2.2.1 payloads.h `server_hdr_v1`：
/// - `total_len1` = 总接收字节**高 32 位**，`total_len2` = 低 32 位
/// - `jitter1` = 抖动整数秒，`jitter2` = 微秒小数（rMillion 定点）
/// - 全部字段网络序
#[derive(Debug, Clone)]
pub struct ServerHdrV1 {
    pub flags: i32,
    /// 总接收字节高 32 位
    pub total_len1: i32,
    /// 总接收字节低 32 位
    pub total_len2: i32,
    pub stop_sec: i32,
    pub stop_usec: i32,
    /// 丢包数（低 32 位）
    pub error_cnt: i32,
    /// 乱序包数
    pub outorder_cnt: i32,
    /// 总包数（低 32 位）
    pub datagrams: i32,
    /// 抖动整数秒
    pub jitter1: i32,
    /// 抖动微秒小数
    pub jitter2: i32,
}

impl ServerHdrV1 {
    pub const SIZE: usize = 40;

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        for v in [
            self.flags,
            self.total_len1,
            self.total_len2,
            self.stop_sec,
            self.stop_usec,
            self.error_cnt,
            self.outorder_cnt,
            self.datagrams,
            self.jitter1,
            self.jitter2,
        ] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < Self::SIZE {
            return Err(format!("server_hdr 长度不足: {} < {}", data.len(), Self::SIZE));
        }
        let rd = |off: usize| i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        Ok(Self {
            flags: rd(0),
            total_len1: rd(4),
            total_len2: rd(8),
            stop_sec: rd(12),
            stop_usec: rd(16),
            error_cnt: rd(20),
            outorder_cnt: rd(24),
            datagrams: rd(28),
            jitter1: rd(32),
            jitter2: rd(36),
        })
    }

    /// 抖动（秒）
    pub fn jitter_secs(&self) -> f64 {
        self.jitter1 as f64 + self.jitter2 as f64 / 1_000_000.0
    }

    /// 抖动（毫秒）
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_secs() * 1000.0
    }

    /// 总接收字节（高 32 + 低 32）
    #[allow(dead_code)] // 测试与调试用
    pub fn total_bytes(&self) -> u64 {
        ((self.total_len1 as u32 as u64) << 32) | self.total_len2 as u32 as u64
    }

    /// 由统计值构造（抖动毫秒 → 秒.微秒 定点）
    pub fn from_stats(total_bytes: u64, error_cnt: u32, datagrams: u32, jitter_ms: f64) -> Self {
        let jitter_secs = (jitter_ms.max(0.0)) / 1000.0;
        let jitter_int = jitter_secs.floor() as i32;
        let jitter_frac = ((jitter_secs - jitter_secs.floor()) * 1_000_000.0).round() as i32;
        Self {
            flags: 0,
            total_len1: (total_bytes >> 32) as i32,
            total_len2: total_bytes as i32,
            stop_sec: 0,
            stop_usec: 0,
            error_cnt: error_cnt as i32,
            outorder_cnt: 0,
            datagrams: datagrams as i32,
            jitter1: jitter_int,
            jitter2: jitter_frac,
        }
    }
}

// ── 组合头 ───────────────────────────────────────────

/// TCP 首包（base + extend，64B）——写在数据连接头部
pub fn tcp_first_payload(base: &ClientHdrV1, ext: &ClientHdrExt) -> Vec<u8> {
    let mut buf = base.serialize();
    buf.extend_from_slice(&ext.serialize());
    buf
}

/// UDP 每包测试头（base + extend，64B；UDP_datagram 16B 由调用方置于包首）
pub fn udp_packet_header(base: &ClientHdrV1, ext: &ClientHdrExt) -> Vec<u8> {
    let mut buf = base.serialize();
    buf.extend_from_slice(&ext.serialize());
    buf
}

/// 按 2.2.1 `Settings_ClientTestHdrLen` 计算 TCP 测试头长度（字节）
///
/// - flags 含 `HEADER_LEN_BIT`：`(flags & LEN_MASK) >> 1`（超过 `MAX_HEADER_LEN` 拒绝）
/// - 否则按 VERSION1/EXTEND→24、VERSION2/EXTEND→+40 推算
/// - 非 v1/v2 头（如端口探测）返回 `None`
pub fn client_test_hdr_len(flags: u32) -> Option<usize> {
    if flags & (HEADER_VERSION1 | HEADER_VERSION2 | HEADER_EXTEND) == 0 {
        return None;
    }
    if flags & HEADER_LEN_BIT != 0 {
        let len = ((flags & HEADER_LEN_MASK) >> 1) as usize;
        if !(24..=MAX_HEADER_LEN).contains(&len) {
            return None;
        }
        Some(len)
    } else {
        let mut peeklen = 0;
        if flags & (HEADER_VERSION1 | HEADER_EXTEND) != 0 {
            peeklen += 24;
        }
        if flags & (HEADER_VERSION2 | HEADER_EXTEND) != 0 {
            peeklen += 40;
        }
        Some(peeklen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_roundtrip_tcp() {
        let h = ClientHdrV1::new_client(false, 2, 5001, 10);
        assert!(!h.is_udp());
        assert!(h.is_time_mode());
        assert_eq!(h.time_secs(), 10);
        assert_eq!(h.m_port, 5001);
        assert_eq!(h.num_threads, 2);
        // 2.2.1 普通 TCP 测试:EXTEND | LEN_BIT | (64<<1)
        assert_eq!(h.flags, 0x4001_0080);
        assert_eq!(client_test_hdr_len(h.flags), Some(64));
        let bytes = h.serialize();
        assert_eq!(bytes.len(), 24);
        let back = ClientHdrV1::deserialize(&bytes).unwrap();
        assert_eq!(back.m_port, 5001);
        assert_eq!(back.m_amount, -1000);
    }

    #[test]
    fn v1_roundtrip_udp() {
        let h = ClientHdrV1::new_client(true, 1, 5001, 5);
        // 2.2.1 普通 UDP 测试:EXTEND | SEQNO64B | LEN_BIT | (80<<1)（长度含 UDP_datagram）
        // 注意:普通 UDP 测试不含 HEADER_UDPTESTS 标志(传输层决定协议类型)
        assert_eq!(h.flags, 0x4801_00A0);
        assert_eq!(h.flags & HEADER_SEQNO64B, HEADER_SEQNO64B);
        assert_eq!(h.flags & HEADER_UDPTESTS, 0);
        let back = ClientHdrV1::deserialize(&h.serialize()).unwrap();
        assert_eq!(back.flags, 0x4801_00A0);
        assert_eq!(back.time_secs(), 5);
    }

    #[test]
    fn v1_rejects_short() {
        assert!(ClientHdrV1::deserialize(&[0u8; 23]).is_err());
    }

    #[test]
    fn ext_roundtrip() {
        let e = ClientHdrExt::new_client(Some(100_000_000));
        assert_eq!(e.version_u, OUR_VERSION_MAJOR_HEX);
        assert_eq!(e.version_l, OUR_VERSION_MINOR_HEX);
        assert_eq!(e.l_rate, 100_000_000);
        // 2.2.1 对 typelen 零填充
        assert_eq!(e.typelen_type, 0);
        assert_eq!(e.typelen_length, 0);
        let bytes = e.serialize();
        assert_eq!(bytes.len(), ClientHdrExt::SIZE);
        let back = ClientHdrExt::deserialize(&bytes).unwrap();
        assert_eq!(back.l_rate, 100_000_000);
        assert_eq!(back.typelen_type, 0);
    }

    #[test]
    fn ack_roundtrip() {
        let a = ClientHdrAck::new_server();
        assert!(a.is_ack());
        assert_eq!(a.serialize().len(), 28);
        let back = ClientHdrAck::deserialize(&a.serialize()).unwrap();
        assert!(back.is_ack());
        assert_eq!(back.version_u, OUR_VERSION_MAJOR_HEX);
        assert_eq!(back.version_l, OUR_VERSION_MINOR_HEX);
    }

    #[test]
    fn hdr_len_fallback() {
        // 无 LEN_BIT:EXTEND → 24+40=64;EXTEND|VERSION1 → 64
        assert_eq!(client_test_hdr_len(HEADER_EXTEND), Some(64));
        assert_eq!(client_test_hdr_len(HEADER_EXTEND | HEADER_VERSION1), Some(64));
        // 非法/探测连接（无任何版本/扩展标志）
        assert_eq!(client_test_hdr_len(0), None);
        // UDP 头含 EXTEND+LEN_BIT → 按长度字段还原 80（TCP 服务端不会收到）
        assert_eq!(client_test_hdr_len(0x4801_00A0), Some(80));
        // 长度字段上限:mask 仅 8 位,最大可表达 255(MAX_HEADER_LEN 守卫为防御性)
        assert_eq!(client_test_hdr_len(HEADER_EXTEND | HEADER_LEN_BIT | (0x1FE)), Some(255));
        // LEN_BIT 置位但长度字段为 0 → 非法
        assert_eq!(client_test_hdr_len(HEADER_EXTEND | HEADER_LEN_BIT), None);
    }

    #[test]
    fn udp_datagram_roundtrip() {
        let mut d = UdpDatagram::new(0x1_0000_0005);
        d.stamp();
        assert_eq!(d.seqno(), 0x1_0000_0005);
        assert!(d.tv_sec > 1_700_000_000);
        let back = UdpDatagram::deserialize(&d.serialize()).unwrap();
        assert_eq!(back.seqno(), 0x1_0000_0005);
        assert_eq!(back.tv_sec, d.tv_sec);
    }

    #[test]
    fn server_hdr_roundtrip() {
        let s = ServerHdrV1 {
            flags: 0,
            total_len1: 0,
            total_len2: 1000,
            stop_sec: 10,
            stop_usec: 0,
            error_cnt: 3,
            outorder_cnt: 0,
            datagrams: 100,
            jitter1: 0,
            jitter2: 1500,
        };
        let back = ServerHdrV1::deserialize(&s.serialize()).unwrap();
        assert_eq!(back.error_cnt, 3);
        assert_eq!(back.datagrams, 100);
        assert_eq!(back.total_bytes(), 1000);
        assert_eq!(back.jitter_ms(), 1.5);
        // from_stats 编码：1.5ms → jitter1=0, jitter2=1500；高/低 32 位拆分
        let f = ServerHdrV1::from_stats(0x1_0000_0000 + 42, 7, 200, 1.5);
        assert_eq!(f.total_len1, 1);
        assert_eq!(f.total_len2, 42);
        assert_eq!(f.total_bytes(), 0x1_0000_0000 + 42);
        assert_eq!(f.jitter1, 0);
        assert_eq!(f.jitter2, 1500);
        assert_eq!(f.jitter_ms(), 1.5);
    }

    #[test]
    fn tcp_first_payload_size() {
        let base = ClientHdrV1::new_client(false, 1, 5002, 10);
        let ext = ClientHdrExt::new_client(None);
        assert_eq!(tcp_first_payload(&base, &ext).len(), 64);
    }

    #[test]
    fn direction_flags_match_iperf2_wire_format() {
        // 对齐 2.2.1 Settings.cpp:2689：非普通模式 VERSION1；DualTest 另加 RUN_NOW
        let normal = ClientHdrV1::new_client(false, 1, 5001, 10);
        assert_eq!(normal.flags & HEADER_VERSION1, 0);
        assert_eq!(normal.server_mode(), super::super::types::ServerTestMode::Normal);

        let tradeoff = ClientHdrV1::new_client(false, 1, 5001, 10)
            .with_direction(super::super::types::TestDirection::TradeOff);
        assert_eq!(tradeoff.flags & HEADER_VERSION1, HEADER_VERSION1);
        assert_eq!(tradeoff.flags & HEADER_RUN_NOW, 0);
        assert_eq!(
            tradeoff.server_mode(),
            super::super::types::ServerTestMode::TradeOff
        );

        let dual = ClientHdrV1::new_client(false, 1, 5001, 10)
            .with_direction(super::super::types::TestDirection::DualTest);
        assert_eq!(dual.flags & HEADER_VERSION1, HEADER_VERSION1);
        assert_eq!(dual.flags & HEADER_RUN_NOW, HEADER_RUN_NOW);
        assert_eq!(
            dual.server_mode(),
            super::super::types::ServerTestMode::DualTest
        );
        // 序列化回环后模式判定保持
        let back = ClientHdrV1::deserialize(&dual.serialize()).unwrap();
        assert_eq!(
            back.server_mode(),
            super::super::types::ServerTestMode::DualTest
        );
    }

    #[test]
    fn time_secs_saturates_on_extreme_amount() {
        // 恶意客户端可发 m_amount = i32::MIN（远程触发）：取负不得溢出 panic
        let mut h = ClientHdrV1::new_client(false, 1, 5001, 10);
        h.m_amount = i32::MIN;
        assert!(h.is_time_mode());
        assert_eq!(h.time_secs(), 21_474_836); // ~248 天（对齐 2.2.1 release 回绕语义）
    }
}
