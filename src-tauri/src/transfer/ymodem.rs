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
    pub file_index: u32,
    pub total_files: u32,
    pub aggregate_bytes_transferred: u64,
    pub aggregate_total_bytes: u64,
    /// 传输方向（字段不直接在 Rust 侧读取，但通过 JSON 序列化发送到前端
    /// transfer-progress 事件，前端据此显示方向指示器）
    #[allow(dead_code)]
    pub direction: TransferDirection,
}

/// 文件级别事件（非逐块进度）
#[derive(Debug, Clone)]
pub enum YModemFileEvent {
    /// 文件开始传输
    FileStart {
        file_name: String,
        file_index: u32,
        total_files: u32,
        file_size: u64,
    },
    /// 文件传输完成（成功或失败）
    FileComplete {
        file_name: String,
        file_index: u32,
        total_files: u32,
        bytes_transferred: u64,
        success: bool,
        error: Option<String>,
    },
}

/// 批次传输结果，用于 transfer-complete 事件
#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchFileResult {
    pub file_name: String,
    pub status: String,   // "completed" | "failed"
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    ///
    /// 返回 `Ok(batch_results)` 其中包含每个文件的传输结果。
    /// 部分文件失败时仍返回 Ok — 调用方通过 BatchFileResult.status 判断。
    pub fn send(
        port: &mut Box<dyn serialport::SerialPort>,
        file_paths: &[String],
        on_progress: impl Fn(TransferProgress),
        on_file_event: impl Fn(YModemFileEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        let total_files = file_paths.len() as u32;

        // 阶段 1：等待接收方发送 'C'（CRC 模式请求）
        // 标准 YModem 接收方会持续发送 'C'（每秒一次），直到发送方响应。
        // 发送前已清空缓冲区，此处严格匹配 'C' 字符。
        // 收到非 'C' 字节说明设备未进入 YModem 接收模式，需用户先在设备端执行接收命令。
        let mut c_count = 0u32;
        for retry in 0..MAX_RETRIES * 3 {
            if cancel() { Self::send_cancel(port); return Err("传输已取消".into()); }
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

        // 计算批次总大小（用于聚合进度）
        let mut aggregate_total: u64 = 0;
        let mut file_sizes: Vec<u64> = Vec::with_capacity(file_paths.len());
        for file_path in file_paths {
            match fs::metadata(file_path) {
                Ok(m) => {
                    file_sizes.push(m.len());
                    aggregate_total += m.len();
                }
                Err(_) => {
                    file_sizes.push(0);
                }
            }
        }

        // 阶段 2：发送文件
        let mut batch_results: Vec<BatchFileResult> = Vec::with_capacity(file_paths.len());
        let mut aggregate_completed: u64 = 0; // 已完成文件的总字节数

        for (file_idx, file_path) in file_paths.iter().enumerate() {
            if cancel() { Self::send_cancel(port); return Err("传输已取消".into()); }

            // 第一个文件之后，清空串口缓冲区中可能残留的字节
            // （设备在 send_eot 阶段可能输出调试信息等），直接继续下一文件。
            // send_eot() 已完成 EOT→NAK→EOT→ACK(+'C') 握手，无需额外等待。
            if file_idx > 0 {
                Self::flush_port_buffer(port);
            }

            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let file_size = file_sizes[file_idx];
            let fi = file_idx as u32;

            // 发送文件开始事件
            on_file_event(YModemFileEvent::FileStart {
                file_name: file_name.clone(),
                file_index: fi,
                total_files,
                file_size,
            });

            // 尝试打开文件
            let file = match fs::File::open(file_path) {
                Ok(f) => f,
                Err(e) => {
                    let err_msg = format!("无法打开文件: {}", e);
                    on_file_event(YModemFileEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: fi,
                        total_files,
                        bytes_transferred: 0,
                        success: false,
                        error: Some(err_msg.clone()),
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "failed".into(),
                        size: 0,
                        error: Some(err_msg.clone()),
                    });
                    // 将失败文件的大小从聚合中剔除
                    aggregate_total -= file_size;
                    continue;
                }
            };

            // 发送块 0（文件元数据）
            let mut block0 = [0u8; BLOCK0_SIZE];
            let meta_str = format!("{}\0{}", file_name, file_size);
            let meta_bytes = meta_str.as_bytes();
            let copy_len = meta_bytes.len().min(BLOCK0_SIZE);
            block0[..copy_len].copy_from_slice(&meta_bytes[..copy_len]);

            if let Err(e) = Self::send_block(port, 0, &block0, BLOCK0_SIZE, cancel) {
                let err_msg = e.to_string();
                on_file_event(YModemFileEvent::FileComplete {
                    file_name: file_name.clone(),
                    file_index: fi,
                    total_files,
                    bytes_transferred: 0,
                    success: false,
                    error: Some(err_msg.clone()),
                });
                batch_results.push(BatchFileResult {
                    file_name: file_name.clone(),
                    status: "failed".into(),
                    size: file_size,
                    error: Some(err_msg),
                });
                aggregate_total -= file_size;
                continue;
            }

            // 发送数据块
            let mut block_num: u8 = 1;
            let mut buf = [0u8; DATA_BLOCK_SIZE];
            let mut total_sent: u64 = 0;
            let mut file = std::io::BufReader::new(file);

            loop {
                if cancel() {
                    Self::send_cancel(port);
                    return Err("传输已取消".into());
                }

                let n = match file.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) => {
                        let err_msg = format!("读取文件错误: {}", e);
                        on_file_event(YModemFileEvent::FileComplete {
                            file_name: file_name.clone(),
                            file_index: fi,
                            total_files,
                            bytes_transferred: total_sent,
                            success: false,
                            error: Some(err_msg.clone()),
                        });
                        batch_results.push(BatchFileResult {
                            file_name: file_name.clone(),
                            status: "failed".into(),
                            size: file_size,
                            error: Some(err_msg),
                        });
                        aggregate_total -= file_size;
                        break;
                    }
                };
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

                if let Err(e) = Self::send_block(port, block_num, &block_data, DATA_BLOCK_SIZE, cancel) {
                    let err_msg = e.to_string();
                    on_file_event(YModemFileEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: fi,
                        total_files,
                        bytes_transferred: total_sent,
                        success: false,
                        error: Some(err_msg.clone()),
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "failed".into(),
                        size: file_size,
                        error: Some(err_msg),
                    });
                    aggregate_total -= file_size;
                    break;
                }

                total_sent += n as u64;
                on_progress(TransferProgress {
                    file_name: file_name.clone(),
                    bytes_transferred: total_sent,
                    total_bytes: file_size,
                    file_index: fi,
                    total_files,
                    aggregate_bytes_transferred: aggregate_completed + total_sent,
                    aggregate_total_bytes: aggregate_total,
                    direction: TransferDirection::Send,
                });

                block_num = block_num.wrapping_add(1);
            }

            // 如果上面是读错误跳出的，continue 到下一个文件
            if total_sent == 0 && file_size > 0 {
                continue;
            }

            // 发送 EOT
            let eot_result = Self::send_eot(port, cancel);
            match eot_result {
                Ok(()) => {
                    aggregate_completed += file_size;
                    on_file_event(YModemFileEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: fi,
                        total_files,
                        bytes_transferred: total_sent,
                        success: true,
                        error: None,
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "completed".into(),
                        size: file_size,
                        error: None,
                    });
                    // 发送最终进度（100%）
                    on_progress(TransferProgress {
                        file_name: file_name.clone(),
                        bytes_transferred: total_sent,
                        total_bytes: file_size,
                        file_index: fi,
                        total_files,
                        aggregate_bytes_transferred: aggregate_completed,
                        aggregate_total_bytes: aggregate_total,
                        direction: TransferDirection::Send,
                    });
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    on_file_event(YModemFileEvent::FileComplete {
                        file_name: file_name.clone(),
                        file_index: fi,
                        total_files,
                        bytes_transferred: total_sent,
                        success: false,
                        error: Some(err_msg.clone()),
                    });
                    batch_results.push(BatchFileResult {
                        file_name: file_name.clone(),
                        status: "failed".into(),
                        size: file_size,
                        error: Some(err_msg),
                    });
                    aggregate_total -= file_size;
                }
            }
        }

        // 阶段 3：发送批次结束（空块 0）
        // send_eot() 已经完成了与设备的 EOT→NAK→EOT→ACK(+'C') 握手。
        // 最后一个文件后，设备端已发送 'C' 等待下一个 Block 0。
        // 直接发送空 Block 0 作为"没有更多文件"的响应（YMODEM 标准方式）。
        let empty_block0 = [0u8; BLOCK0_SIZE];
        if let Err(e) = Self::send_block(port, 0, &empty_block0, BLOCK0_SIZE, cancel) {
            log::warn!("YModem 批次结束空块 0 发送失败（文件数据已传输）: {}", e);
        }

        Ok(batch_results)
    }

    /// 发送 CAN 序列通知远端取消（尽力而为）
    fn send_cancel(port: &mut Box<dyn serialport::SerialPort>) {
        // 发送两个连续的 CAN 字节（YModem 规范要求）
        if let Err(e) = port.write_all(&[CAN, CAN]) {
            log::warn!("发送 CAN 序列失败: {}", e);
        }
        // 短暂排空，保证字节发出
        if let Err(e) = port.flush() {
            log::warn!("CAN 后刷新端口失败: {}", e);
        }
        // 等待一小段时间让远端处理
        std::thread::sleep(Duration::from_millis(100));
    }

    /// 清空端口接收缓冲区
    ///
    /// 丢弃设备残留输出，避免干扰 YModem 握手协议。
    /// 连续读取直到连续 3 次超时（每次 50ms），确保缓冲区清空。
    fn flush_port_buffer(port: &mut Box<dyn serialport::SerialPort>) {
        let mut buf = [0u8; 256];
        let mut empty_count = 0u32;
        for _ in 0..20 {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => { empty_count = 0; }
                _ => {
                    empty_count += 1;
                    if empty_count >= 3 { break; }
                }
            }
        }
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
            port.flush()?;

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

    /// 发送 EOT（文件结束）
    ///
    /// RT-Thread YMODEM 接收端（及多数嵌入式实现）的 EOT 握手序列：
    ///   1. 发送方 → EOT
    ///   2. 设备端 → NAK（要求重传 EOT，同时 on_end 回调关闭文件）
    ///   3. 发送方 → EOT（重传）
    ///   4. 设备端 → ACK（确认 EOT），紧接着 → 'C'（请求下一文件）
    ///
    /// 设备端 on_end 回调涉及 Flash 写入，可能耗时数秒。
    /// 第一轮 EOT 的响应等待因此使用较长的 5 秒超时。
    /// ACK 后的 'C' 探测窗口放宽到 2 秒以防设备处理延迟。
    fn send_eot(
        port: &mut Box<dyn serialport::SerialPort>,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for retry in 0..MAX_RETRIES {
            if cancel() { return Err("传输已取消".into()); }
            port.write_all(&[EOT])?;
            port.flush()?;

            // 等待 ACK / NAK / 'C'，使用 5 秒超时以适应设备端 on_end 回调的 Flash 写入延迟
            match read_byte_with_timeout(port, 5000)? {
                Some(ACK) => {
                    // 设备 ACK 了 EOT。探测是否紧跟着 'C'（请求下一文件）。
                    // RT-Thread 的 _rym_do_fin 在 ACK 后立即发送 'C'，无延迟。
                    // 放宽探测窗口至 2 秒以防处理延迟或串口缓冲。
                    match read_byte_with_timeout(port, 2000)? {
                        Some(C) => {
                            log::info!("EOT: ACK 后收到 'C'，接收方已请求下一文件");
                            return Ok(());
                        }
                        Some(NAK) => {
                            // ACK 后又收到 NAK——设备要求重传（罕见但不排除）
                            log::info!("EOT: ACK 后收到 NAK，重传 EOT");
                            continue;
                        }
                        Some(CAN) => {
                            return Err("接收方取消了传输".into());
                        }
                        _ => {
                            // 超时或收到其他字节，视为 EOT 握手完成
                            log::info!("EOT: 收到 ACK（未探测到 'C'）");
                            return Ok(());
                        }
                    }
                }
                Some(C) => {
                    // 设备直接回复 'C'（部分实现省略 ACK，直接请求下一文件）
                    log::info!("EOT: 收到 'C'（替代 ACK），接收方已请求下一文件");
                    return Ok(());
                }
                Some(NAK) => {
                    // 设备要求重传 EOT——标准 YMODEM 行为！
                    // RT-Thread 实现在 on_end() 之后发送 NAK 要求第二次 EOT
                    log::info!("EOT: 收到 NAK，重传 EOT（第 {} 次）", retry + 1);
                    continue;
                }
                Some(CAN) => {
                    return Err("接收方取消了传输".into());
                }
                None => {
                    // 超时——设备端 on_end 回调（Flash 写入）可能耗时较长
                    if retry == MAX_RETRIES - 1 {
                        return Err("EOT 确认超时：设备可能正在处理文件（Flash 写入）".into());
                    }
                    log::info!("EOT: 等待响应超时，重试（第 {} 次）", retry + 1);
                    continue;
                }
                _ => {
                    continue;
                }
            }
        }
        Err("EOT 发送失败：超过最大重试次数".into())
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
    ///
    /// 返回 `Ok(batch_results)` 包含每个接收到的文件的结果。
    pub fn receive(
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: impl Fn(TransferProgress),
        on_file_event: impl Fn(YModemFileEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>> {
        fs::create_dir_all(download_dir)?;

        let mut current_file: Option<(String, fs::File, u64)> = None; // (name, handle, total_size)
        let mut file_index: u32 = 0;
        let mut aggregate_bytes: u64 = 0; // 已完成的文件总大小
        let mut aggregate_total: u64 = 0; // 所有已知文件的总大小（动态更新）
        let mut batch_results: Vec<BatchFileResult> = Vec::new();

        // 阶段 1：发送 'C' 启动 CRC 模式传输
        for retry in 0..MAX_RETRIES {
            if cancel() { Self::send_cancel(port); return Err("传输已取消".into()); }
            port.write_all(&[C])?;
            port.flush()?;
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
            if cancel() {
                Self::send_cancel(port);
                return Err("传输已取消".into());
            }

            let header = match read_byte_with_timeout(port, 5000)? {
                Some(SOH) => SOH,
                Some(STX) => STX,
                Some(EOT) => {
                    // 文件结束 — 关闭当前文件
                    if let Some((name, _, total)) = current_file.take() {
                        let fsize = total;
                        aggregate_bytes += fsize;
                        on_file_event(YModemFileEvent::FileComplete {
                            file_name: name.clone(),
                            file_index,
                            total_files: 0, // 接收方不知道总文件数
                            bytes_transferred: fsize,
                            success: true,
                            error: None,
                        });
                        on_progress(TransferProgress {
                            file_name: name.clone(),
                            bytes_transferred: fsize,
                            total_bytes: fsize,
                            file_index,
                            total_files: 0,
                            aggregate_bytes_transferred: aggregate_bytes,
                            aggregate_total_bytes: aggregate_total,
                            direction: TransferDirection::Receive,
                        });
                        batch_results.push(BatchFileResult {
                            file_name: name,
                            status: "completed".into(),
                            size: fsize,
                            error: None,
                        });
                        file_index += 1;
                    }
                    port.write_all(&[ACK])?;
                    port.flush()?;
                    // YModem 批量模式：ACK EOT 后发送 'C' 请求发送方传输下一个文件。
                    // 最多尝试数次，若发送方不响应则超时退出。
                    port.write_all(&[C])?;
                    port.flush()?;
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
                port.flush()?;
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
                port.flush()?;
                continue;
            }

            // 块 0 处理
            if block_num == 0 {
                // 空块 0 → 批次结束
                if data[0] == 0 {
                    if let Some((name, _, total)) = current_file.take() {
                        on_file_event(YModemFileEvent::FileComplete {
                            file_name: name.clone(),
                            file_index,
                            total_files: 0,
                            bytes_transferred: total,
                            success: true,
                            error: None,
                        });
                        batch_results.push(BatchFileResult {
                            file_name: name,
                            status: "completed".into(),
                            size: total,
                            error: None,
                        });
                    }
                    port.write_all(&[ACK])?;
                    port.flush()?;
                    break;
                }

                // 关闭上一个文件（如果存在）并记录结果
                if let Some((prev_name, _, prev_total)) = current_file.take() {
                    aggregate_bytes += prev_total;
                    on_file_event(YModemFileEvent::FileComplete {
                        file_name: prev_name.clone(),
                        file_index: file_index - 1,
                        total_files: 0,
                        bytes_transferred: prev_total,
                        success: true,
                        error: None,
                    });
                    batch_results.push(BatchFileResult {
                        file_name: prev_name,
                        status: "completed".into(),
                        size: prev_total,
                        error: None,
                    });
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

                    aggregate_total += total_size;

                    match fs::File::create(&file_path) {
                        Ok(file) => {
                            current_file = Some((file_name.clone(), file, total_size));
                            on_file_event(YModemFileEvent::FileStart {
                                file_name: file_name.clone(),
                                file_index,
                                total_files: 0, // 接收方未知
                                file_size: total_size,
                            });
                            on_progress(TransferProgress {
                                file_name,
                                bytes_transferred: 0,
                                total_bytes: total_size,
                                file_index,
                                total_files: 0,
                                aggregate_bytes_transferred: aggregate_bytes,
                                aggregate_total_bytes: aggregate_total,
                                direction: TransferDirection::Receive,
                            });
                        }
                        Err(e) => return Err(format!("无法创建文件 {:?}: {}", file_path, e).into()),
                    }
                }
                port.write_all(&[ACK])?;
                port.flush()?;
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
                        file_index,
                        total_files: 0,
                        aggregate_bytes_transferred: aggregate_bytes + pos,
                        aggregate_total_bytes: aggregate_total,
                        direction: TransferDirection::Receive,
                    });
                }
                port.write_all(&[ACK])?;
                port.flush()?;
            }
        }

        Ok(batch_results)
    }

    /// 发送 CAN 序列通知远端取消（尽力而为）
    fn send_cancel(port: &mut Box<dyn serialport::SerialPort>) {
        if let Err(e) = port.write_all(&[CAN, CAN]) {
            log::warn!("接收方发送 CAN 序列失败: {}", e);
        }
        if let Err(e) = port.flush() {
            log::warn!("接收方 CAN 后刷新端口失败: {}", e);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
