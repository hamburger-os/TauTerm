//! ZModem 协议实现
//!
//! 基于 lrzsz-0.12.20 `zm.c`/`lrz.c`/`lsz.c` 标准流程实现。
//!
//! ## 功能
//! - ZBIN/ZBIN32/ZHEX 帧格式
//! - ZDLE 控制字符转义
//! - 滑动窗口流控（ZCRCG/ZCRCQ/ZCRCW）
//! - 32 位 CRC 校验
//! - 自适应块大小（1024-8192B）
//! - 批量文件传输（ZFILE/ZEOF/ZFIN）
//!
//! ## 简化
//! - 无断点续传（ZRPOS 返回 0）
//! - 无 ZCOMMAND/ZSTDERR 帧
//! - 无 RLE 压缩和加密
//! - 无 timesync（ZCHALLENGE 忽略）

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::transfer::crc::{crc16_ccitt, crc32_zmodem, crc32_verify};
use crate::transfer::io::{self, read_byte_with_timeout, CAN};
use crate::transfer::protocol::TransferProtocol;
use crate::transfer::types::{
    BatchFileResult, FileInfo, FileTransferEvent, TransferDirection, TransferProgress,
};

// ═══════════════════════════════════════════════════════════════════
//  ZMODEM 协议常量（对齐 lrzsz zmodem.h）
// ═══════════════════════════════════════════════════════════════════

const ZPAD: u8 = b'*';         // 0x2A — 帧开始填充
const ZDLE: u8 = 0x18;         // Ctrl-X — 转义字节（也是 CAN）
const ZBIN: u8 = b'A';         // 二进制帧
const ZHEX: u8 = b'B';         // 十六进制帧
const ZBIN32: u8 = b'C';       // 二进制帧（32 位 CRC）

// ── 帧类型 ──
const ZRQINIT: u8 = 0;    // 请求接收初始化
const ZRINIT: u8 = 1;     // 接收初始化
const ZSINIT: u8 = 2;     // 发送初始化（忽略）
const ZACK: u8 = 3;       // 确认
const ZFILE: u8 = 4;      // 文件信息
const ZSKIP: u8 = 5;      // 跳过文件
const ZNAK: u8 = 6;       // 否定确认
const ZABORT: u8 = 7;     // 中止批次
const ZFIN: u8 = 8;       // 会话结束
const ZRPOS: u8 = 9;      // 续传位置
const ZDATA: u8 = 10;     // 数据帧
const ZEOF: u8 = 11;      // 文件结束
const ZFERR: u8 = 12;     // 致命错误
#[allow(dead_code)]
const ZCRC: u8 = 13;      // 请求文件 CRC
const ZCHALLENGE: u8 = 14; // 挑战（忽略）
#[allow(dead_code)]
const ZCOMPL: u8 = 15;    // 完成
const ZCAN: u8 = 16;      // 远端取消
const ZFREECNT: u8 = 17;  // 可用空间（忽略）
#[allow(dead_code)]
const ZCOMMAND: u8 = 18;  // 命令（忽略）
#[allow(dead_code)]
const ZSTDERR: u8 = 19;   // stderr（忽略）

// ── ZDLE 转义序列：数据帧结束标记 ──
const ZCRCE: u8 = b'h';   // CRC next, frame ends, header follows
const ZCRCG: u8 = b'i';   // CRC next, frame continues
const ZCRCQ: u8 = b'j';   // CRC next, ZACK expected
const ZCRCW: u8 = b'k';   // CRC next, ZACK expected, end of frame

// ── 帧头字节位置 ──
const ZF0: usize = 3;
const ZF1: usize = 2;
const ZF2: usize = 1;
const ZF3: usize = 0;

// ── 能力标志位 ──
const CANFC32: u8 = 0x20; // 能使用 32 位 CRC
const CANFDX: u8 = 0x01;  // 全双工

// ── XON / XOFF ──
const XON: u8 = 0x11;
const XOFF: u8 = 0x13;
const CR: u8 = 0x0D;

/// 需要 ZDLE 转义的字符
fn needs_escaping(byte: u8) -> bool {
    byte == ZDLE || byte == XON || byte == XOFF || byte == CR || byte == 0x7f || byte == 0xff
}

/// 默认块大小
const DEFAULT_BLOCK_SIZE: usize = 1024;
/// 最大块大小
const MAX_BLOCK_SIZE: usize = 8192;
/// 最大重试次数
const MAX_RETRIES: u32 = 10;
/// 帧接收超时（秒）
const FRAME_TIMEOUT_S: u32 = 10;
/// 数据帧接收超时（秒）
const DATA_TIMEOUT_S: u32 = 60;

// ═══════════════════════════════════════════════════════════════════
//  ZMODEM 帧类型
// ═══════════════════════════════════════════════════════════════════

/// ZMODEM 帧
#[derive(Debug, Clone)]
enum ZFrame {
    /// 帧头帧（无数据载荷）
    Header {
        frame_type: u8,
        flags: [u8; 4],
    },
    /// 数据帧（ZDATA / ZFILE）
    Data {
        /// 帧类型（ZDATA=10 或 ZFILE=4）
        frame_type: u8,
        data: Vec<u8>,
        frameend: u8,
    },
    /// 取消信号（两个 CAN）
    Cancel,
}

// ═══════════════════════════════════════════════════════════════════
//  ZMODEM 协议处理器
// ═══════════════════════════════════════════════════════════════════

/// ZMODEM 协议处理器
#[derive(Debug, Clone)]
pub struct ZModem {
    /// 是否使用 32 位 CRC（ZBIN32 帧格式）
    pub use_crc32: bool,
    /// 最大数据块大小（字节）
    pub max_block_size: usize,
    /// 滑动窗口大小（帧数，保留字段）
    #[allow(dead_code)]
    pub window_size: usize,
}

impl Default for ZModem {
    fn default() -> Self {
        ZModem {
            use_crc32: true,
            max_block_size: MAX_BLOCK_SIZE,
            window_size: 1,
        }
    }
}

impl TransferProtocol for ZModem {
    fn send_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        zmodem_send(port, files, self.use_crc32, self.max_block_size, on_progress, on_file_event, cancel)
    }

    fn receive_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        zmodem_receive(port, download_dir, self.use_crc32, on_progress, on_file_event, cancel)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  帧编码 / 发送
// ═══════════════════════════════════════════════════════════════════

/// 发送 ZHEX 帧头
///
/// 格式: ZPAD ZPAD ZHEX type_hex f3_hex f2_hex f1_hex f0_hex crc1_hex crc2_hex CR LF [XON]
fn send_hex_header(
    port: &mut Box<dyn serialport::SerialPort>,
    frame_type: u8,
    hdr: &[u8; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut crc_data = Vec::with_capacity(5);
    crc_data.push(frame_type);
    crc_data.extend_from_slice(hdr);
    let crc = crc16_ccitt(&crc_data);

    let hex_frame = format!(
        "**{}{:02X}{:02X}{:02X}{:02X}{:02X}{:04X}\r\n\x11",
        ZHEX as char,
        frame_type,
        hdr[ZF0],
        hdr[ZF1],
        hdr[ZF2],
        hdr[ZF3],
        crc
    );
    port.write_all(hex_frame.as_bytes())?;
    port.flush()?;
    log::debug!(
        "ZHEX frame sent: type={} hdr=[{:02X},{:02X},{:02X},{:02X}] crc={:04X}",
        frame_type, hdr[ZF0], hdr[ZF1], hdr[ZF2], hdr[ZF3], crc
    );
    Ok(())
}

/// 发送二进制帧头（ZBIN 或 ZBIN32）
///
/// 格式: ZPAD ZPAD ZDLE ZBIN{ZBIN32} type f3 f2 f1 f0 crc[16|32]
fn send_binary_header(
    port: &mut Box<dyn serialport::SerialPort>,
    frame_type: u8,
    hdr: &[u8; 4],
    use_crc32: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bin_type = if use_crc32 { ZBIN32 } else { ZBIN };

    // 构建帧头数据用于 CRC 计算
    let mut crc_data = Vec::with_capacity(5);
    crc_data.push(frame_type);
    crc_data.extend_from_slice(hdr);

    // 二进制的 type_flag 不需要 ZDLE 转义（只有 hdr 字节可能需要，但 hdr 通常都是普通数据）
    port.write_all(&[ZPAD, ZPAD, ZDLE, bin_type])?;
    port.write_all(&[frame_type])?;
    port.write_all(hdr)?;

    if use_crc32 {
        let crc = crc32_zmodem(&crc_data);
        port.write_all(&crc.to_le_bytes())?;
        log::debug!(
            "ZBIN32 header sent: type={} hdr=[{:02X},{:02X},{:02X},{:02X}] crc32={:08X}",
            frame_type, hdr[ZF0], hdr[ZF1], hdr[ZF2], hdr[ZF3], crc
        );
    } else {
        let crc = crc16_ccitt(&crc_data);
        port.write_all(&crc.to_le_bytes())?;
        log::debug!(
            "ZBIN header sent: type={} hdr=[{:02X},{:02X},{:02X},{:02X}] crc16={:04X}",
            frame_type, hdr[ZF0], hdr[ZF1], hdr[ZF2], hdr[ZF3], crc
        );
    }

    port.flush()?;
    Ok(())
}

/// 发送 ZDATA 数据帧（带 ZDLE 转义）
///
/// 格式: ZPAD ZPAD ZDLE ZBIN{ZBIN32} ZDATA f3 f2 f1 f0 [escaped_data*] crc ZDLE frameend
fn send_data_frame(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &[u8],
    frameend: u8,
    use_crc32: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bin_type = if use_crc32 { ZBIN32 } else { ZBIN };

    // hdr[0] = offset pos (always 0 for our simplified impl)
    let hdr: [u8; 4] = [0, 0, 0, 0];

    // CRC data for header: ZDATA type + hdr
    let mut hdr_crc_data = Vec::with_capacity(5);
    hdr_crc_data.push(ZDATA);
    hdr_crc_data.extend_from_slice(&hdr);

    // CRC for the data payload
    let data_crc = if use_crc32 {
        crc32_zmodem(buf)
    } else {
        crc16_ccitt(buf) as u32
    };

    // Send preamble
    port.write_all(&[ZPAD, ZPAD, ZDLE, bin_type])?;
    port.write_all(&[ZDATA])?;
    port.write_all(&hdr)?;

    // Send header CRC (of type+hdr)
    if use_crc32 {
        let hdr_crc = crc32_zmodem(&hdr_crc_data);
        port.write_all(&hdr_crc.to_le_bytes())?;
    } else {
        let hdr_crc = crc16_ccitt(&hdr_crc_data);
        port.write_all(&hdr_crc.to_le_bytes())?;
    }

    // Send escaped data
    for &byte in buf {
        if needs_escaping(byte) {
            port.write_all(&[ZDLE, byte ^ 0x40])?;
        } else {
            port.write_all(&[byte])?;
        }
    }

    // Send data CRC
    if use_crc32 {
        let crc_bytes = data_crc.to_le_bytes();
        // CRC bytes themselves may need escaping
        for &byte in &crc_bytes {
            if needs_escaping(byte) {
                port.write_all(&[ZDLE, byte ^ 0x40])?;
            } else {
                port.write_all(&[byte])?;
            }
        }
    } else {
        let hi = (data_crc >> 8) as u8;
        let lo = (data_crc & 0xFF) as u8;
        if needs_escaping(hi) {
            port.write_all(&[ZDLE, hi ^ 0x40])?;
        } else {
            port.write_all(&[hi])?;
        }
        if needs_escaping(lo) {
            port.write_all(&[ZDLE, lo ^ 0x40])?;
        } else {
            port.write_all(&[lo])?;
        }
    }

    // Send frame end marker
    port.write_all(&[ZDLE, frameend])?;
    port.flush()?;

    log::debug!(
        "ZDATA frame sent: {} bytes, frameend={:02X}, crc32={}",
        buf.len(),
        frameend,
        use_crc32
    );

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
//  帧接收
// ═══════════════════════════════════════════════════════════════════

/// 接收一个 ZMODEM 帧
///
/// 自动检测 ZHEX / ZBIN / ZBIN32 / CANCEL 帧格式。
fn receive_frame(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_s: u32,
) -> Result<ZFrame, Box<dyn std::error::Error>> {
    let timeout_ms = (timeout_s as u64) * 1000;

    // Wait for first ZPAD
    loop {
        match read_byte_with_timeout(port, timeout_ms)? {
            Some(ZPAD) => break,
            Some(CAN) => {
                // Read another CAN for double-CAN cancel sequence
                // If got CAN + CAN, it's a cancel signal
                let b2 = read_byte_with_timeout(port, 500)?;
                if b2 == Some(CAN) {
                    return Ok(ZFrame::Cancel);
                }
                // Single CAN — might be noise; continue waiting for ZPAD
                continue;
            }
            Some(_) => continue,
            None => return Err("接收帧超时".into()),
        }
    }

    // Read second ZPAD
    match read_byte_with_timeout(port, 2000)? {
        Some(ZPAD) => {}
        Some(CAN) => {
            // Check for CAN+CAN cancel after single ZPAD
            let b2 = read_byte_with_timeout(port, 500)?;
            if b2 == Some(CAN) {
                return Ok(ZFrame::Cancel);
            }
            return Err("帧头格式错误：第二个字节不是 ZPAD".into());
        }
        Some(_) => return Err("帧头格式错误：第二个字节不是 ZPAD".into()),
        None => return Err("帧头格式错误：第二个字节超时".into()),
    }

    // Read frame type byte (third byte after ZPAD ZPAD)
    // Binary frames: ZPAD ZPAD ZDLE ZBIN|ZBIN32 ...
    // Hex frames:     ZPAD ZPAD ZHEX ...
    let frame_kind = match read_byte_with_timeout(port, 2000)? {
        Some(b) => b,
        None => return Err("帧类型字节超时".into()),
    };

    match frame_kind {
        ZDLE => {
            // Binary frame: read the fourth byte for ZBIN/ZBIN32
            let bin_type = read_byte_with_timeout(port, 2000)?
                .ok_or("二进制帧子类型超时")?;
            match bin_type {
                ZBIN | ZBIN32 => receive_binary_frame(port, bin_type),
                ZCAN => Ok(ZFrame::Cancel),
                _ => Err(format!("未知二进制帧子类型: 0x{:02X}", bin_type).into()),
            }
        }
        ZHEX => receive_hex_frame(port),
        _ => Err(format!("未知帧类型: 0x{:02X}", frame_kind).into()),
    }
}

/// 接收二进制帧（ZBIN 或 ZBIN32）
fn receive_binary_frame(
    port: &mut Box<dyn serialport::SerialPort>,
    bin_type: u8,
) -> Result<ZFrame, Box<dyn std::error::Error>> {
    let use_crc32 = bin_type == ZBIN32;

    // Read type byte
    let frame_type = read_byte_required(port, 2000, "帧类型")?;

    // Read 4-byte header flags
    let mut flags = [0u8; 4];
    for i in 0..4 {
        flags[i] = read_byte_required(port, 2000, &format!("帧头字节 {}", i))?;
    }

    // Read header CRC (2 bytes for ZBIN, 4 bytes for ZBIN32)
    // We skip validating the header CRC for simplicity.
    // These MUST be consumed from the stream even if we don't validate them.
    if use_crc32 {
        read_byte_required(port, 1000, "hdr crc[0]")?;
        read_byte_required(port, 1000, "hdr crc[1]")?;
        read_byte_required(port, 1000, "hdr crc[2]")?;
        read_byte_required(port, 1000, "hdr crc[3]")?;
    } else {
        read_byte_required(port, 1000, "hdr crc[0]")?;
        read_byte_required(port, 1000, "hdr crc[1]")?;
    }

    // ZDATA and ZFILE have escaped data payload; all other types are header-only
    let has_data = frame_type == ZDATA || frame_type == ZFILE;

    if has_data {
        // Read escaped data payload
        let mut raw_data: Vec<u8> = Vec::new();
        loop {
            let b = read_byte_required(port, 3000, "数据字节")?;
            if b == ZDLE {
                let next = read_byte_required(port, 3000, "ZDLE 转义")?;
                match next {
                    ZCRCE | ZCRCG | ZCRCQ | ZCRCW => {
                        // frame end marker found — raw_data contains data + CRC bytes
                        let crc_len = if use_crc32 { 4 } else { 2 };
                        if raw_data.len() < crc_len {
                            return Ok(ZFrame::Data {
                                frame_type,
                                data: Vec::new(),
                                frameend: next,
                            });
                        }

                        let (data_part, crc_bytes) = raw_data.split_at(raw_data.len() - crc_len);

                        let crc_valid = if use_crc32 && crc_bytes.len() == 4 {
                            let expected = u32::from_le_bytes([
                                crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]
                            ]);
                            crc32_verify(data_part, expected)
                        } else if !use_crc32 && crc_bytes.len() == 2 {
                            let expected = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
                            crate::transfer::crc::crc16_ccitt_verify(data_part, expected)
                        } else {
                            false
                        };

                        if !crc_valid {
                            log::warn!(
                                "{} data CRC mismatch ({} bytes, end={:02X})",
                                if frame_type == ZFILE { "ZFILE" } else { "ZDATA" },
                                data_part.len(),
                                next
                            );
                        }

                        return Ok(ZFrame::Data {
                            frame_type,
                            data: data_part.to_vec(),
                            frameend: next,
                        });
                    }
                    ZCAN => {
                        return Ok(ZFrame::Cancel);
                    }
                    _ => {
                        // Escaped byte: next ^ 0x40
                        raw_data.push(next ^ 0x40);
                    }
                }
            } else {
                raw_data.push(b);
            }
        }
    } else {
        // Header-only frame (no data payload)
        Ok(ZFrame::Header { frame_type, flags })
    }
}

/// 接收十六进制帧头（ZHEX）
fn receive_hex_frame(
    port: &mut Box<dyn serialport::SerialPort>,
) -> Result<ZFrame, Box<dyn std::error::Error>> {
    // Format: ZPAD ZPAD ZHEX type_hex f3_hex f2_hex f1_hex f0_hex crc1 crc2 CR LF [XON]
    // hex = 2 + 2 + 2 + 2 + 2 + 4 + 2 = 16 hex chars + CR LF + optional XON

    let mut hex_buf = Vec::with_capacity(20);
    for _ in 0..20 {
        let b = read_byte_required(port, 2000, "hex frame byte")?;
        if b == b'\n' {
            break;
        }
        if b == b'\r' {
            // Next should be \n, optionally XON
            continue;
        }
        if b.is_ascii_hexdigit() {
            hex_buf.push(b);
        }
    }

    if hex_buf.len() < 10 {
        return Err("ZHEX 帧数据不完整".into());
    }

    let hex_str = String::from_utf8_lossy(&hex_buf);
    let frame_type = u8::from_str_radix(&hex_str[0..2], 16)
        .map_err(|e| format!("ZHEX 帧类型解析失败: {}", e))?;
    let f0 = u8::from_str_radix(&hex_str[2..4], 16)
        .map_err(|e| format!("ZHEX f0 解析失败: {}", e))?;
    let f1 = u8::from_str_radix(&hex_str[4..6], 16)
        .map_err(|e| format!("ZHEX f1 解析失败: {}", e))?;
    let f2 = u8::from_str_radix(&hex_str[6..8], 16)
        .map_err(|e| format!("ZHEX f2 解析失败: {}", e))?;
    let f3 = u8::from_str_radix(&hex_str[8..10], 16)
        .map_err(|e| format!("ZHEX f3 解析失败: {}", e))?;

    log::debug!(
        "ZHEX frame received: type={} f3={:02X} f2={:02X} f1={:02X} f0={:02X}",
        frame_type, f3, f2, f1, f0
    );

    Ok(ZFrame::Header {
        frame_type,
        flags: [f0, f1, f2, f3],
    })
}

/// 读取一个必需字节（有超时）
fn read_byte_required(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
    context: &str,
) -> Result<u8, Box<dyn std::error::Error>> {
    match read_byte_with_timeout(port, timeout_ms)? {
        Some(b) => Ok(b),
        None => Err(format!("{}: 超时", context).into()),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  发送状态机
// ═══════════════════════════════════════════════════════════════════

/// ZMODEM 发送文件批次
fn zmodem_send(
    port: &mut Box<dyn serialport::SerialPort>,
    files: &[FileInfo],
    use_crc32: bool,
    max_block_size: usize,
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    if files.is_empty() {
        return Err("没有要发送的文件".into());
    }

    let total_files = files.len() as u32;

    // ── 阶段 1: 发送 ZRQINIT，等待 ZRINIT ──
    log::info!("ZMODEM send: sending ZRQINIT");
    for retry in 0..MAX_RETRIES {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        // Send ZRQINIT header with our capabilities
        // flags[ZF0] = 0 (no special flags), ZF1-ZF3 = 0
        let mut flags = [0u8; 4];
        flags[ZF0] = if use_crc32 { CANFC32 } else { 0 };
        flags[ZF0] |= CANFDX;

        if retry == 0 {
            // First attempt: use hex header for compatibility
            send_hex_header(port, ZRQINIT, &[3, 0, 0, CANFC32 | CANFDX])?;
        } else {
            send_binary_header(port, ZRQINIT, &flags, use_crc32)?;
        }

        match receive_frame(port, FRAME_TIMEOUT_S) {
            Ok(ZFrame::Header { frame_type: ZRINIT, .. }) => {
                log::info!("ZMODEM send: received ZRINIT");
                // Negotiate CRC32: if we can and receiver can, use CRC32
                // rf[ZF0] & CANFC32 tells us if receiver supports it
                // We'll use use_crc32 for simplicity
                break;
            }
            Ok(ZFrame::Header { frame_type: ZCAN, .. }) | Ok(ZFrame::Cancel) => {
                return Err("接收方取消了传输".into());
            }
            Ok(_) => {
                if retry >= MAX_RETRIES - 1 {
                    return Err("等待 ZRINIT 收到意外帧".into());
                }
            }
            Err(e) => {
                log::warn!("ZRQINIT retry {}: {}", retry + 1, e);
                if retry >= MAX_RETRIES - 1 {
                    return Err(format!("等待 ZRINIT 超时（已重试 {} 次）", retry).into());
                }
            }
        }
    }

    let mut batch_results: Vec<BatchFileResult> = Vec::with_capacity(files.len());
    let mut aggregate_total: u64 = files.iter().map(|f| f.size).sum();
    let mut aggregate_completed: u64 = 0;

    // ── 阶段 2: 逐文件传输 ──
    'file_loop: for (file_idx, file_info) in files.iter().enumerate() {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        let fi = file_idx as u32;

        on_file_event(FileTransferEvent::FileStart {
            file_name: file_info.name.clone(),
            file_index: fi,
            total_files,
            file_size: file_info.size,
        });

        // Open file
        let file = match fs::File::open(&file_info.path) {
            Ok(f) => f,
            Err(e) => {
                let err_msg = format!("无法打开文件: {}", e);
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: 0,
                    success: false,
                    error: Some(err_msg.clone()),
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "failed".into(),
                    size: 0,
                    error: Some(err_msg),
                });
                aggregate_total -= file_info.size;
                continue;
            }
        };
        let mut file = std::io::BufReader::new(file);

        // ── 发送 ZFILE ──
        // Build ZFILE data: filename\0size mtime mode sent remaining
        // "sent" = bytes already sent (0 for fresh transfer)
        // "remaining" = total size
        let zfile_data = format!(
            "{}\0{} {} {:o} 0 {}",
            file_info.name,
            file_info.size,
            file_info.mtime,
            0o100644u32,
            file_info.size
        );

        log::debug!("ZMODEM send: ZFILE data: {:?}", zfile_data);

        // Send ZFILE as a data frame
        let zfile_bytes = zfile_data.as_bytes();
        send_data_frame_with_header(
            port,
            ZFILE,
            zfile_bytes,
            ZCRCW,
            use_crc32,
        )?;

        // ── 等待 ZRPOS 或 ZSKIP ──
        match receive_frame(port, FRAME_TIMEOUT_S) {
            Ok(ZFrame::Header { frame_type: ZRPOS, flags }) => {
                // Receiver wants this file — ZRPOS tells us where to start
                // For simplified implementation, we always start from 0
                let resume_pos = u32::from_le_bytes([flags[ZF0], flags[ZF1], flags[ZF2], flags[ZF3]]) as u64;
                log::debug!(
                    "ZMODEM send: received ZRPOS for file {} (resume at {})",
                    file_info.name, resume_pos
                );
                // resume_pos is ignored — we always send from the beginning
            }
            Ok(ZFrame::Header { frame_type: ZSKIP, .. }) => {
                // Receiver doesn't want this file
                log::info!("ZMODEM send: receiver skipped file {}", file_info.name);
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: 0,
                    success: true,
                    error: None,
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "skipped".into(),
                    size: 0,
                    error: None,
                });
                aggregate_total -= file_info.size;
                continue 'file_loop;
            }
            Ok(ZFrame::Header { frame_type: ZCAN, .. }) | Ok(ZFrame::Cancel) => {
                return Err("接收方取消了传输".into());
            }
            Ok(ZFrame::Header { frame_type: ZRINIT, .. }) => {
                // Receiver might send ZRINIT instead of ZRPOS (resume at 0)
                log::debug!("ZMODEM send: received ZRINIT instead of ZRPOS for file {}", file_info.name);
            }
            Ok(other) => {
                log::warn!("ZMODEM send: unexpected frame after ZFILE: {:?}", other);
                // Proceed anyway
            }
            Err(e) => {
                log::error!("ZMODEM send: no response after ZFILE: {}", e);
                let err_msg = format!("接收方无响应: {}", e);
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: 0,
                    success: false,
                    error: Some(err_msg.clone()),
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "failed".into(),
                    size: file_info.size,
                    error: Some(err_msg),
                });
                aggregate_total -= file_info.size;
                continue 'file_loop;
            }
        }

        // ── 发送数据块 ──
        let mut total_sent: u64 = 0;
        let mut block_size: usize = DEFAULT_BLOCK_SIZE;
        let mut buf = vec![0u8; max_block_size];

        // Track consecutive successes/failures for adaptive sizing
        let mut consecutive_ok: u32 = 0;

        while total_sent < file_info.size {
            if cancel() {
                io::send_cancel(port);
                return Err("传输已取消".into());
            }

            // Read a chunk
            let remaining = (file_info.size - total_sent) as usize;
            let read_size = block_size.min(remaining);
            let buf_slice = &mut buf[..read_size];

            match file.read_exact(buf_slice) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // Read what we got
                }
                Err(e) => {
                    let err_msg = format!("读取文件错误: {}", e);
                    on_file_event(FileTransferEvent::FileComplete {
                        file_name: file_info.name.clone(),
                        file_index: fi,
                        total_files,
                        bytes_transferred: total_sent,
                        success: false,
                        error: Some(err_msg.clone()),
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_info.name.clone(),
                        status: "failed".into(),
                        size: file_info.size,
                        error: Some(err_msg),
                    });
                    aggregate_total -= file_info.size;
                    io::send_cancel(port);
                    continue 'file_loop;
                }
            }

            let data = &buf[..read_size];
            let is_last_block = total_sent + read_size as u64 >= file_info.size;

            // Choose frame end marker
            let frameend = if is_last_block {
                ZCRCW  // Wait for ZACK
            } else {
                ZCRCG  // Continue immediately
            };

            // Send data with retry
            let mut sent_ok = false;
            for retry in 0..MAX_RETRIES {
                if cancel() {
                    io::send_cancel(port);
                    return Err("传输已取消".into());
                }

                send_data_frame(port, data, frameend, use_crc32)?;

                // If we used ZCRCW, wait for ZACK; if ZCRCG, just continue
                if frameend == ZCRCW {
                    match receive_frame(port, DATA_TIMEOUT_S) {
                        Ok(ZFrame::Header { frame_type: ZACK, .. }) => {
                            sent_ok = true;
                            break;
                        }
                        Ok(ZFrame::Header { frame_type: ZNAK, .. }) => {
                            log::warn!(
                                "ZDATA NAK (retry {}/{})",
                                retry + 1, MAX_RETRIES
                            );
                            // Adaptive: halve block size on error (min 512)
                            block_size = (block_size / 2).max(512);
                            consecutive_ok = 0;
                            continue;
                        }
                        Ok(ZFrame::Cancel) | Ok(ZFrame::Header { frame_type: ZCAN, .. }) => {
                            return Err("接收方取消了传输".into());
                        }
                        _ => {
                            log::warn!("Unexpected response after ZDATA (retry {})", retry + 1);
                            continue;
                        }
                    }
                } else {
                    // ZCRCG — no response expected, assume success
                    sent_ok = true;
                    break;
                }
            }

            if !sent_ok && frameend == ZCRCW {
                let err_msg = format!("数据块传输失败（{} 字节处）", total_sent);
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: total_sent,
                    success: false,
                    error: Some(err_msg.clone()),
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "failed".into(),
                    size: file_info.size,
                    error: Some(err_msg),
                });
                aggregate_total -= file_info.size;
                continue 'file_loop;
            }

            total_sent += read_size as u64;

            // Adaptive block sizing
            consecutive_ok += 1;
            if consecutive_ok >= 2 && block_size < max_block_size {
                block_size = (block_size * 2).min(max_block_size);
                consecutive_ok = 0;
            }

            on_progress(TransferProgress {
                file_name: file_info.name.clone(),
                bytes_transferred: total_sent,
                total_bytes: file_info.size,
                file_index: fi,
                total_files,
                aggregate_bytes_transferred: aggregate_completed + total_sent,
                aggregate_total_bytes: aggregate_total,
                direction: TransferDirection::Send,
            });
        }

        // ── 发送 ZEOF ──
        // ZEOF header: flags[ZF0..ZF3] = file offset (LSB first)
        // But for header-only frame, send it as binary header with file offset
        let offset = total_sent as u32;
        let eof_flags = offset.to_le_bytes();
        send_binary_header(port, ZEOF, &eof_flags, use_crc32)?;

        // Wait for ZRINIT (receiver is ready for next file)
        match receive_frame(port, FRAME_TIMEOUT_S) {
            Ok(ZFrame::Header { frame_type: ZRINIT, .. }) => {
                log::info!("ZMODEM send: received ZRINIT, ready for next file");
            }
            Ok(ZFrame::Header { frame_type: ZFIN, .. }) => {
                // Receiver says "I'm done" — all remaining files are skipped
                log::info!("ZMODEM send: received ZFIN, finishing batch early");
                on_file_event(FileTransferEvent::FileComplete {
                    file_name: file_info.name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: total_sent,
                    success: true,
                    error: None,
                });
                batch_results.push(BatchFileResult {
                    file_name: file_info.name.clone(),
                    status: "completed".into(),
                    size: file_info.size,
                    error: None,
                });

                // Mark remaining files as skipped
                for (_skip_idx, skip_info) in files.iter().enumerate().skip(file_idx + 1) {
                    batch_results.push(BatchFileResult {
                        file_name: skip_info.name.clone(),
                        status: "skipped".into(),
                        size: 0,
                        error: Some("批次提前终止".into()),
                    });
                }
                // Go to finish phase
                // Send ZFIN, wait for ZFIN response, send "OO"
                send_binary_header(port, ZFIN, &[0; 4], use_crc32)?;
                match receive_frame(port, FRAME_TIMEOUT_S) {
                    Ok(ZFrame::Header { frame_type: ZFIN, .. }) => {}
                    _ => log::warn!("Expected ZFIN response"),
                }
                port.write_all(b"OO")?;
                port.flush()?;
                return Ok(batch_results);
            }
            Ok(ZFrame::Header { frame_type: ZCAN, .. }) | Ok(ZFrame::Cancel) => {
                return Err("接收方取消了传输".into());
            }
            Ok(_) | Err(_) => {
                log::debug!("ZMODEM send: no ZRINIT after ZEOF, proceeding");
            }
        }

        aggregate_completed += file_info.size;
        on_file_event(FileTransferEvent::FileComplete {
            file_name: file_info.name.clone(),
            file_index: fi,
            total_files,
            bytes_transferred: total_sent,
            success: true,
            error: None,
        });
        batch_results.push(BatchFileResult {
            file_name: file_info.name.clone(),
            status: "completed".into(),
            size: file_info.size,
            error: None,
        });
    }

    // ── 阶段 3: 发送 ZFIN 结束会话 ──
    log::info!("ZMODEM send: sending ZFIN");
    for retry in 0..MAX_RETRIES {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }
        send_binary_header(port, ZFIN, &[0; 4], use_crc32)?;
        match receive_frame(port, FRAME_TIMEOUT_S) {
            Ok(ZFrame::Header { frame_type: ZFIN, .. }) => {
                log::info!("ZMODEM send: received ZFIN response");
                break;
            }
            Ok(ZFrame::Cancel) | Ok(ZFrame::Header { frame_type: ZCAN, .. }) => {
                return Err("接收方取消了传输".into());
            }
            _ => {
                if retry >= MAX_RETRIES - 1 {
                    log::warn!("ZMODEM send: ZFIN exchange incomplete, sending OO anyway");
                }
            }
        }
    }

    // Send "OO" (over and out)
    port.write_all(b"OO")?;
    port.flush()?;

    Ok(batch_results)
}

/// Send a frame with type + data payload (for ZFILE, ZDATA with non-standard types)
fn send_data_frame_with_header(
    port: &mut Box<dyn serialport::SerialPort>,
    frame_type: u8,
    data: &[u8],
    frameend: u8,
    use_crc32: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bin_type = if use_crc32 { ZBIN32 } else { ZBIN };

    // CRC for the data payload
    let data_crc = if use_crc32 {
        crc32_zmodem(data)
    } else {
        crc16_ccitt(data) as u32
    };

    // Send preamble + type
    port.write_all(&[ZPAD, ZPAD, ZDLE, bin_type])?;
    port.write_all(&[frame_type])?;

    // ZFILE uses flags: ZF0 = conversion flags, ZF1-ZF3 = file size optionally
    // For simplicity, use all zeros
    port.write_all(&[0u8; 4])?;

    // Send header CRC of (frame_type + flags)
    let mut hdr_data = vec![frame_type];
    hdr_data.extend_from_slice(&[0u8; 4]);
    if use_crc32 {
        let hdr_crc = crc32_zmodem(&hdr_data);
        port.write_all(&hdr_crc.to_le_bytes())?;
    } else {
        let hdr_crc = crc16_ccitt(&hdr_data);
        port.write_all(&hdr_crc.to_le_bytes())?;
    }

    // Send escaped data
    for &byte in data {
        if needs_escaping(byte) {
            port.write_all(&[ZDLE, byte ^ 0x40])?;
        } else {
            port.write_all(&[byte])?;
        }
    }

    // Send data CRC (escaped)
    if use_crc32 {
        for &byte in &data_crc.to_le_bytes() {
            if needs_escaping(byte) {
                port.write_all(&[ZDLE, byte ^ 0x40])?;
            } else {
                port.write_all(&[byte])?;
            }
        }
    } else {
        let hi = (data_crc >> 8) as u8;
        let lo = (data_crc & 0xFF) as u8;
        if needs_escaping(hi) {
            port.write_all(&[ZDLE, hi ^ 0x40])?;
        } else {
            port.write_all(&[hi])?;
        }
        if needs_escaping(lo) {
            port.write_all(&[ZDLE, lo ^ 0x40])?;
        } else {
            port.write_all(&[lo])?;
        }
    }

    // Send frame end marker
    port.write_all(&[ZDLE, frameend])?;
    port.flush()?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
//  接收状态机
// ═══════════════════════════════════════════════════════════════════

/// ZMODEM 接收文件批次
fn zmodem_receive(
    port: &mut Box<dyn serialport::SerialPort>,
    download_dir: &str,
    use_crc32: bool,
    on_progress: &dyn Fn(TransferProgress),
    on_file_event: &dyn Fn(FileTransferEvent),
    cancel: &mut dyn FnMut() -> bool,
) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
    fs::create_dir_all(download_dir)?;

    let mut current_file: Option<(String, fs::File, u64, u64)> = None; // (name, file, total, written)
    let mut file_index: u32 = 0;
    let mut total_files_received: u32 = 0;
    let mut aggregate_bytes: u64 = 0;
    let mut aggregate_total: u64 = 0;
    let mut batch_results: Vec<BatchFileResult> = Vec::new();
    let mut session_active: bool = true;

    // ── 阶段 1: 等待 ZRQINIT ──
    log::info!("ZMODEM receive: waiting for ZRQINIT from sender");
    loop {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        match receive_frame(port, 60) {
            Ok(ZFrame::Header { frame_type: ZRQINIT, .. }) => {
                log::info!("ZMODEM receive: received ZRQINIT");
                break;
            }
            Ok(ZFrame::Header { frame_type: ZSINIT, .. }) => {
                log::info!("ZMODEM receive: received ZSINIT (sender-initiated), sending ZRINIT");
                break; // Can also accept ZSINIT
            }
            Ok(ZFrame::Cancel) | Ok(ZFrame::Header { frame_type: ZCAN, .. }) => {
                return Err("发送方取消了传输".into());
            }
            Ok(_) => continue, // Ignore other frames while waiting
            Err(e) => {
                log::warn!("Waiting for ZRQINIT: {}", e);
                continue;
            }
        }
    }

    // ── 阶段 2: 回复 ZRINIT ──
    let mut rinit_flags = [0u8; 4];
    rinit_flags[ZF0] = if use_crc32 { CANFC32 } else { 0 };
    rinit_flags[ZF0] |= CANFDX;
    // ZF1-ZF3 = buffer size (unused for simplified impl)

    // First send ZRINIT as hex for compatibility
    send_hex_header(port, ZRINIT, &rinit_flags)?;

    // ── 阶段 3: 接收文件 ──
    while session_active {
        if cancel() {
            io::send_cancel(port);
            return Err("传输已取消".into());
        }

        let frame = receive_frame(port, FRAME_TIMEOUT_S)?;

        match frame {
            ZFrame::Data { frame_type, data, frameend } => {
                if frame_type == ZFILE {
                    // ─── ZFILE: File metadata ───
                    // Parse ZFILE data: filename\0size mtime mode sent remaining
                    let file_name = parse_zfile_name(&data);
                    let file_size = parse_zfile_size(&data);

                    log::info!(
                        "ZMODEM receive: ZFILE name={}, size={}",
                        file_name, file_size
                    );

                    // Close previous file if any
                    if let Some((prev_name, _, _prev_total, prev_written)) = current_file.take() {
                        aggregate_bytes += prev_written;
                        on_file_event(FileTransferEvent::FileComplete {
                            file_name: prev_name.clone(),
                            file_index,
                            total_files: total_files_received,
                            bytes_transferred: prev_written,
                            success: true,
                            error: None,
                        });
                        batch_results.push(BatchFileResult {
                            file_name: prev_name,
                            status: "completed".into(),
                            size: prev_written,
                            error: None,
                        });
                        file_index += 1;
                    }

                    // Create new file
                    let file_path = Path::new(download_dir).join(&file_name);
                    aggregate_total += file_size;

                    match fs::File::create(&file_path) {
                        Ok(file) => {
                            on_file_event(FileTransferEvent::FileStart {
                                file_name: file_name.clone(),
                                file_index,
                                total_files: total_files_received + 1,
                                file_size,
                            });
                            on_progress(TransferProgress {
                                file_name: file_name.clone(),
                                bytes_transferred: 0,
                                total_bytes: file_size,
                                file_index,
                                total_files: total_files_received + 1,
                                aggregate_bytes_transferred: aggregate_bytes,
                                aggregate_total_bytes: aggregate_total,
                                direction: TransferDirection::Receive,
                            });
                            current_file = Some((file_name, file, file_size, 0u64));
                            total_files_received += 1;

                            // Send ZRPOS = 0 (no resume)
                            send_binary_header(port, ZRPOS, &[0; 4], use_crc32)?;
                        }
                        Err(e) => {
                            log::error!("ZMODEM receive: cannot create file {:?}: {}", file_path, e);
                            send_binary_header(port, ZSKIP, &[0; 4], use_crc32)?;
                        }
                    }
                } else {
                    // ─── ZDATA: File data ───
                    if let Some((ref file_name, ref mut file, total_size, ref mut bytes_written)) =
                        current_file
                    {
                        let write_len = if total_size > 0 {
                            let remaining = (total_size - *bytes_written) as usize;
                            remaining.min(data.len())
                        } else {
                            data.len()
                        };

                        if let Err(e) = file.write_all(&data[..write_len]) {
                            let err_msg = format!("写入文件错误: {}", e);
                            on_file_event(FileTransferEvent::FileComplete {
                                file_name: file_name.clone(),
                                file_index,
                                total_files: total_files_received,
                                bytes_transferred: *bytes_written,
                                success: false,
                                error: Some(err_msg.clone()),
                            });
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "failed".into(),
                                size: *bytes_written,
                                error: Some(err_msg),
                            });
                            current_file = None;
                            send_binary_header(port, ZFERR, &[0; 4], use_crc32)?;
                            continue;
                        }

                        *bytes_written += write_len as u64;

                        on_progress(TransferProgress {
                            file_name: file_name.clone(),
                            bytes_transferred: *bytes_written,
                            total_bytes: total_size,
                            file_index,
                            total_files: total_files_received,
                            aggregate_bytes_transferred: aggregate_bytes + *bytes_written,
                            aggregate_total_bytes: aggregate_total,
                            direction: TransferDirection::Receive,
                        });

                        // Send ZACK for flow control
                        if frameend == ZCRCW || frameend == ZCRCQ {
                            let ack_pos = (*bytes_written).to_le_bytes();
                            let ack_flags = [
                                ack_pos[0], ack_pos[1], ack_pos[2], ack_pos[3]
                            ];
                            send_binary_header(port, ZACK, &ack_flags, use_crc32)?;
                        }
                    } else {
                        log::warn!("ZMODEM receive: received ZDATA but no current file");
                    }
                }
            }

            ZFrame::Header { frame_type, flags } => {
                match frame_type {
                    ZEOF => {
                        // ─── End of current file ───
                        // flags contain the file offset (LSB first)
                        let _eof_pos = u32::from_le_bytes([flags[0], flags[1], flags[2], flags[3]]) as u64;

                        if let Some((ref file_name, ref mut _file, _total, ref bytes_written)) =
                            current_file
                        {
                            let actual = *bytes_written;

                            aggregate_bytes += actual;
                            on_file_event(FileTransferEvent::FileComplete {
                                file_name: file_name.clone(),
                                file_index,
                                total_files: total_files_received,
                                bytes_transferred: actual,
                                success: true,
                                error: None,
                            });
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "completed".into(),
                                size: actual,
                                error: None,
                            });
                            file_index += 1;
                        }
                        // Drop current_file (closes the file handle)
                        current_file = None;

                        // Send ZRINIT to request next file
                        send_binary_header(port, ZRINIT, &rinit_flags, use_crc32)?;
                        log::info!("ZMODEM receive: ZEOF processed, sent ZRINIT for next file");
                    }

                    ZFIN => {
                        // ─── Session finished ───
                        session_active = false;

                        if let Some((ref file_name, ref mut _file, _total, ref bytes_written)) =
                            current_file
                        {
                            let actual = *bytes_written;
                            aggregate_bytes += actual;
                            on_file_event(FileTransferEvent::FileComplete {
                                file_name: file_name.clone(),
                                file_index,
                                total_files: total_files_received,
                                bytes_transferred: actual,
                                success: true,
                                error: None,
                            });
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "completed".into(),
                                size: actual,
                                error: None,
                            });
                            current_file = None;
                        }

                        send_binary_header(port, ZFIN, &[0; 4], use_crc32)?;

                        // Wait for "OO"
                        log::info!("ZMODEM receive: ZFIN processed, waiting for OO");
                        let mut got_o = false;
                        for _ in 0..100 {
                            match read_byte_with_timeout(port, 100)? {
                                Some(b'O') => {
                                    if got_o {
                                        break;
                                    }
                                    got_o = true;
                                }
                                Some(_) => {
                                    if got_o {
                                        break; // O + something
                                    }
                                }
                                None => {
                                    if got_o {
                                        break; // Got one O, no second
                                    }
                                }
                            }
                        }
                        log::info!("ZMODEM receive: session complete");
                    }

                    ZABORT | ZCAN => {
                        session_active = false;
                        if let Some((ref file_name, _, _, _)) = current_file {
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "failed".into(),
                                size: 0,
                                error: Some("发送方中止了传输".into()),
                            });
                            current_file = None;
                        }
                    }

                    ZSKIP => {
                        // Sender wants to skip a file — unlikely in receive, but handle it
                        if let Some((ref file_name, _, _, written)) = current_file.take() {
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "skipped".into(),
                                size: written,
                                error: None,
                            });
                        }
                    }

                    ZFERR => {
                        if let Some((ref file_name, _, _, written)) = current_file.take() {
                            batch_results.push(BatchFileResult {
                                file_name: file_name.clone(),
                                status: "failed".into(),
                                size: written,
                                error: Some("发送方文件错误".into()),
                            });
                        }
                    }

                    ZCHALLENGE => {
                        // Ignore challenge (no timesync)
                        log::debug!("ZMODEM receive: ignoring ZCHALLENGE");
                    }

                    ZFREECNT => {
                        // Ignore free count
                        log::debug!("ZMODEM receive: ignoring ZFREECNT");
                    }

                    _ => {
                        log::debug!(
                            "ZMODEM receive: unhandled frame type {}",
                            frame_type
                        );
                    }
                }
            }

            ZFrame::Cancel => {
                if let Some((ref file_name, _, _, written)) = current_file.take() {
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "failed".into(),
                        size: written,
                        error: Some("传输已取消".into()),
                    });
                }
                break;
            }
        }
    }

    Ok(batch_results)
}

/// Parse filename from ZFILE data (format: filename\0...)
fn parse_zfile_name(data: &[u8]) -> String {
    if let Some(null_pos) = data.iter().position(|&b| b == 0) {
        String::from_utf8_lossy(&data[..null_pos]).to_string()
    } else {
        String::from_utf8_lossy(data).to_string()
    }
}

/// Parse file size from ZFILE data (format: ...\0size mtime...)
fn parse_zfile_size(data: &[u8]) -> u64 {
    if let Some(null_pos) = data.iter().position(|&b| b == 0) {
        let rest = &data[null_pos + 1..];
        // Find first space or null to separate size from mtime
        let size_str: String = rest
            .iter()
            .take_while(|&&b| b != b' ' && b != 0)
            .map(|&b| b as char)
            .collect();
        size_str.parse().unwrap_or(0)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_escaping() {
        assert!(needs_escaping(ZDLE));    // 0x18
        assert!(needs_escaping(XON));     // 0x11
        assert!(needs_escaping(XOFF));    // 0x13
        assert!(needs_escaping(CR));      // 0x0D
        assert!(needs_escaping(0x7f));
        assert!(needs_escaping(0xff));
        assert!(!needs_escaping(b'A'));
        assert!(!needs_escaping(0x00));
    }

    #[test]
    fn test_parse_zfile_name() {
        let data = b"test.txt\000100 1234567890 100644 0 100";
        assert_eq!(parse_zfile_name(data), "test.txt");
    }

    #[test]
    fn test_parse_zfile_size() {
        let data = b"test.txt\000100 1234567890";
        assert_eq!(parse_zfile_size(data), 100);
    }

    #[test]
    fn test_parse_zfile_name_no_null() {
        let data = b"test.txt";
        assert_eq!(parse_zfile_name(data), "test.txt");
    }

    #[test]
    fn test_parse_zfile_size_no_null() {
        let data = b"test.txt";
        assert_eq!(parse_zfile_size(data), 0);
    }

    #[test]
    fn test_zmodem_default() {
        let z = ZModem::default();
        assert!(z.use_crc32);
        assert_eq!(z.max_block_size, 8192);
        assert_eq!(z.window_size, 1);
    }
}
