//! YModem 协议实现
//!
//! 基于 YModem 规范实现文件收发：
//! - 块 0：文件元数据（文件名 + 大小，以 NULL 结尾）
//! - 数据块：1024 字节 + CRC-16
//! - EOT / 批次结束（空块 0）

use std::fs;
use std::io::{Read, Seek, Write};
use std::time::Duration;

/// 传输方向
#[derive(Debug, Clone, PartialEq)]
pub enum TransferDirection {
    Send,
    Receive,
}

/// 传输进度
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub file_name: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    #[allow(dead_code)]
    pub direction: TransferDirection,
}

/// CRC-16/CCITT 查找表
const CRC_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut j = 0;
        while j < 8 {
            crc = if (crc & 0x8000) != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// 计算 CRC-16/CCITT
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc = (crc << 8) ^ CRC_TABLE[((crc >> 8) as u8 ^ byte) as usize];
    }
    crc
}

/// YModem 常量
const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const CAN: u8 = 0x18;
const C: u8 = 0x43;
const DATA_BLOCK_SIZE: usize = 1024;
const BLOCK0_SIZE: usize = 128;
const MAX_RETRIES: u32 = 10;

/// YModem 发送器 — 直接操作串口
pub struct YModemSender;

impl YModemSender {
    /// 通过串口发送文件（YModem 协议）
    pub fn send(
        port: &mut Box<dyn serialport::SerialPort>,
        file_paths: &[String],
        on_progress: impl Fn(TransferProgress),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 阶段 1：等待接收方发送 'C'（CRC 模式请求）
        // 标准 YModem 接收方会持续发送 'C'（每秒一次），直到发送方响应。
        // 发送前已清空缓冲区，此处严格匹配 'C' 字符。
        // 收到非 'C' 字节说明设备未进入 YModem 接收模式，需用户先在设备端执行接收命令。
        let mut c_count = 0u32;
        for retry in 0..MAX_RETRIES * 3 {
            if cancel() { return Err("传输已取消".into()); }
            match read_byte_with_timeout(port, 1000)? {
                Some(C) => {
                    c_count += 1;
                    if c_count >= 1 { break; }  // 收到至少一个 'C' 即继续
                }
                Some(NAK) => continue,
                Some(CAN) => return Err("接收方取消了传输".into()),
                Some(other) => {
                    // 收到非协议字节 → 设备可能未进入 YModem 模式
                    if retry >= MAX_RETRIES {
                        return Err(format!(
                            "未检测到设备 YModem 就绪信号（收到 0x{:02X} 而非 0x43 'C'）。\n请先在设备终端中执行 YModem 接收命令（如 loady、rb、rz 等），待设备开始发送 'C' 后再启动文件传输。",
                            other
                        ).into());
                    }
                }
                None => {
                    if retry == MAX_RETRIES * 3 - 1 {
                        return Err("等待设备 YModem 就绪信号超时。请先在设备终端中执行接收命令（如 loady、rb）。".into());
                    }
                }
            }
        }

        // 阶段 2：发送文件
        for (_file_idx, file_path) in file_paths.iter().enumerate() {
            if cancel() { return Err("传输已取消".into()); }

            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let metadata = fs::metadata(file_path)?;
            let file_size = metadata.len();
            let mut file = fs::File::open(file_path)?;

            // 发送块 0（文件元数据）
            let mut block0 = [0u8; BLOCK0_SIZE];
            let meta_str = format!("{}\0{}", file_name, file_size);
            let meta_bytes = meta_str.as_bytes();
            let copy_len = meta_bytes.len().min(BLOCK0_SIZE);
            block0[..copy_len].copy_from_slice(&meta_bytes[..copy_len]);

            Self::send_block(port, 0, &block0, BLOCK0_SIZE, cancel)?;

            // 发送数据块
            let mut block_num: u8 = 1;
            let mut buf = [0u8; DATA_BLOCK_SIZE];
            let mut total_sent: u64 = 0;

            loop {
                if cancel() { return Err("传输已取消".into()); }

                let n = file.read(&mut buf)?;
                if n == 0 { break; }

                // 填充不足的数据块
                let block_data = if n < DATA_BLOCK_SIZE {
                    let mut padded = [0u8; DATA_BLOCK_SIZE];
                    padded[..n].copy_from_slice(&buf[..n]);
                    padded
                } else {
                    let mut arr = [0u8; DATA_BLOCK_SIZE];
                    arr.copy_from_slice(&buf[..DATA_BLOCK_SIZE]);
                    arr
                };

                Self::send_block(port, block_num, &block_data, DATA_BLOCK_SIZE, cancel)?;

                total_sent += n as u64;
                on_progress(TransferProgress {
                    file_name: file_name.clone(),
                    bytes_transferred: total_sent,
                    total_bytes: file_size,
                    direction: TransferDirection::Send,
                });

                block_num = block_num.wrapping_add(1);
            }

            // 发送 EOT
            Self::send_eot(port, cancel)?;
        }

        // 阶段 3：发送批次结束（空块 0）
        // 某些嵌入式 YModem 实现在 EOT+ACK 后立即退出协议模式，
        // 不再响应空块 0。此处将空块 0 失败降级为警告，因为文件数据已完整传输。
        let empty_block0 = [0u8; BLOCK0_SIZE];
        if let Err(e) = Self::send_block(port, 0, &empty_block0, BLOCK0_SIZE, cancel) {
            log::warn!("YModem 批次结束空块 0 发送失败（文件数据已传输）: {}", e);
        }

        Ok(())
    }

    /// 发送单个块
    fn send_block(
        port: &mut Box<dyn serialport::SerialPort>,
        block_num: u8,
        data: &[u8],
        block_size: usize,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let header_byte = if block_size == DATA_BLOCK_SIZE { STX } else { SOH };
        let data_slice = &data[..block_size];

        // 构建块：header + block_num + ~block_num + data + CRC
        let mut packet = Vec::with_capacity(3 + block_size + 2);
        packet.push(header_byte);
        packet.push(block_num);
        packet.push(!block_num);
        packet.extend_from_slice(data_slice);

        let crc = crc16_ccitt(data_slice);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);

        for retry in 0..MAX_RETRIES {
            if cancel() { return Err("传输已取消".into()); }

            port.write_all(&packet)?;

            // 等待 ACK
            match read_byte_with_timeout(port, 3000)? {
                Some(ACK) => return Ok(()),
                Some(CAN) => return Err("接收方取消了传输".into()),
                Some(NAK) | None => {
                    if retry == MAX_RETRIES - 1 {
                        return Err(format!("块 {} 重试次数耗尽", block_num).into());
                    }
                }
                _ => {
                    if retry == MAX_RETRIES - 1 {
                        return Err(format!("块 {} 收到意外响应", block_num).into());
                    }
                }
            }
        }

        Err(format!("块 {} 发送失败", block_num).into())
    }

    /// 发送 EOT
    fn send_eot(
        port: &mut Box<dyn serialport::SerialPort>,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for retry in 0..MAX_RETRIES {
            if cancel() { return Err("传输已取消".into()); }
            port.write_all(&[EOT])?;
            match read_byte_with_timeout(port, 3000)? {
                Some(ACK) => return Ok(()),
                Some(CAN) => return Err("接收方取消了传输".into()),
                Some(NAK) | None => {
                    if retry == MAX_RETRIES - 1 {
                        return Err("EOT 确认超时".into());
                    }
                }
                _ => continue,
            }
        }
        Err("EOT 发送失败".into())
    }
}

/// 读取一个字节（带超时）— 共享工具函数
fn read_byte_with_timeout(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
) -> Result<Option<u8>, Box<dyn std::error::Error>> {
    let mut buf = [0u8; 1];
    let start = std::time::Instant::now();
    loop {
        match port.read(&mut buf) {
            Ok(1) => return Ok(Some(buf[0])),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(Box::new(e)),
        }
        if start.elapsed() > Duration::from_millis(timeout_ms) {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// YModem 接收器 — 直接操作串口
pub struct YModemReceiver;

impl YModemReceiver {
    /// 通过串口接收文件（YModem 协议）
    pub fn receive(
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: impl Fn(TransferProgress),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(download_dir)?;

        let mut current_file: Option<(String, fs::File, u64)> = None; // (name, handle, total_size)

        // 阶段 1：发送 'C' 启动 CRC 模式传输
        for retry in 0..MAX_RETRIES {
            if cancel() { return Err("传输已取消".into()); }
            port.write_all(&[C])?;
            match read_byte_with_timeout(port, 3000)? {
                Some(SOH) | Some(STX) => break,
                Some(CAN) => return Err("发送方取消了传输".into()),
                _ => {
                    if retry == MAX_RETRIES - 1 {
                        return Err("启动传输超时".into());
                    }
                }
            }
        }

        loop {
            if cancel() { return Err("传输已取消".into()); }

            let header = match read_byte_with_timeout(port, 5000)? {
                Some(SOH) => SOH,
                Some(STX) => STX,
                Some(EOT) => {
                    // 文件结束 — 关闭当前文件
                    if let Some((name, file, total)) = current_file.take() {
                        drop(file);
                        on_progress(TransferProgress {
                            file_name: name,
                            bytes_transferred: total,
                            total_bytes: total,
                            direction: TransferDirection::Receive,
                        });
                    }
                    port.write_all(&[ACK])?;
                    continue; // 可能是批次中的下一个文件
                }
                Some(CAN) => return Err("发送方取消了传输".into()),
                _ => return Err("等待块超时".into()),
            };

            let block_size = if header == STX { DATA_BLOCK_SIZE } else { BLOCK0_SIZE };

            // 读取块序号和反码
            let block_num = match read_byte_with_timeout(port, 1000)? {
                Some(b) => b,
                None => return Err("读取块序号超时".into()),
            };
            let block_num_neg = match read_byte_with_timeout(port, 1000)? {
                Some(b) => b,
                None => return Err("读取块序号反码超时".into()),
            };

            if block_num != !block_num_neg {
                port.write_all(&[NAK])?;
                continue;
            }

            // 读取块数据
            let mut data = vec![0u8; block_size];
            for b in data.iter_mut() {
                *b = match read_byte_with_timeout(port, 1000)? {
                    Some(byte) => byte,
                    None => return Err("读取块数据超时".into()),
                };
            }

            // 读取 CRC
            let crc_hi = match read_byte_with_timeout(port, 1000)? {
                Some(b) => b,
                None => return Err("读取 CRC 超时".into()),
            };
            let crc_lo = match read_byte_with_timeout(port, 1000)? {
                Some(b) => b,
                None => return Err("读取 CRC 超时".into()),
            };
            let received_crc = ((crc_hi as u16) << 8) | (crc_lo as u16);

            // 验证 CRC
            let computed_crc = crc16_ccitt(&data);
            if computed_crc != received_crc {
                port.write_all(&[NAK])?;
                continue;
            }

            // 块 0 处理
            if block_num == 0 {
                // 空块 0 → 批次结束
                if data[0] == 0 {
                    if let Some((_, file, _)) = current_file.take() {
                        drop(file);
                    }
                    port.write_all(&[ACK])?;
                    break;
                }

                // 关闭上一个文件（如果存在）
                if let Some((_, file, _)) = current_file.take() {
                    drop(file);
                }

                // 解析文件元数据：name\0size\0...
                let null_pos = data.iter().position(|&b| b == 0);
                if let Some(pos) = null_pos {
                    let file_name = String::from_utf8_lossy(&data[..pos]).to_string();
                    let file_path = std::path::Path::new(download_dir).join(&file_name);

                    // 解析文件大小（在 name\0 之后，下一个 \0 之前）
                    let rest = &data[pos + 1..];
                    let size_str = rest.iter()
                        .take_while(|&&b| b != 0)
                        .map(|&b| b as char)
                        .collect::<String>();
                    let total_size: u64 = size_str.parse().unwrap_or(0);

                    match fs::File::create(&file_path) {
                        Ok(file) => {
                            current_file = Some((file_name.clone(), file, total_size));
                            on_progress(TransferProgress {
                                file_name,
                                bytes_transferred: 0,
                                total_bytes: total_size,
                                direction: TransferDirection::Receive,
                            });
                        }
                        Err(e) => return Err(format!("无法创建文件 {:?}: {}", file_path, e).into()),
                    }
                }
                port.write_all(&[ACK])?;
            } else {
                // 数据块 — 写入当前文件
                if let Some((ref file_name, ref mut file, total_size)) = current_file {
                    // 截断到实际大小（最后一块可能不满）
                    let write_len = if block_size == DATA_BLOCK_SIZE {
                        data.len()
                    } else {
                        data.iter().rposition(|&b| b != 0x1A).map_or(0, |p| p + 1)
                    };
                    file.write_all(&data[..write_len])?;
                    let pos = file.stream_position().unwrap_or(0);
                    on_progress(TransferProgress {
                        file_name: file_name.clone(),
                        bytes_transferred: pos,
                        total_bytes: total_size,
                        direction: TransferDirection::Receive,
                    });
                }
                port.write_all(&[ACK])?;
            }
        }

        Ok(())
    }
}
