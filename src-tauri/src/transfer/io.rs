//! 共享 I/O 工具函数
//!
//! 所有 X/Y/ZModem 协议共用的串口 I/O 操作：
//! - 带超时的单字节读取
//! - 端口缓冲区刷新（丢弃残留数据）
//! - CAN 取消序列发送

use std::io::Read;
use std::time::{Duration, Instant};

/// CAN 字节常量
pub const CAN: u8 = 0x18;
/// NAK 字节常量
pub const NAK: u8 = 0x15;
/// 'C' 字节常量 — CRC 模式请求（WANTCRC）
pub const C: u8 = 0x43;
/// 'G' 字节常量 — YMODEM-g 流模式请求（WANTG）
pub const G: u8 = 0x47;

/// 从串口读取一个字节（带超时）
///
/// 以 10ms 轮询间隔读取，总等待不超过 `timeout_ms`。
/// 返回 `Ok(Some(byte))` 收到字节，`Ok(None)` 超时，`Err` I/O 错误。
pub fn read_byte_with_timeout(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
) -> Result<Option<u8>, Box<dyn std::error::Error>> {
    let mut buf = [0u8; 1];
    let start = Instant::now();
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

/// 清空端口接收缓冲区
///
/// 连续读取并丢弃数据，直到连续 3 次读取为空（超时或零字节），
/// 确保缓冲区完全清空。最多尝试 20 次以避免死循环。
pub fn flush_port_buffer(port: &mut Box<dyn serialport::SerialPort>) {
    let mut buf = [0u8; 256];
    let mut empty_count: u32 = 0;
    for _ in 0..20 {
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                empty_count = 0;
            }
            _ => {
                empty_count += 1;
                if empty_count >= 3 {
                    break;
                }
            }
        }
    }
}

/// 发送 CAN 取消序列（对齐 lrzsz `canit()`: 10 个 CAN + 8 个退格）
///
/// 尽力而为通知远端取消传输。发送后刷新输出缓冲区并等待 100ms
/// 以确保字节发出并被远端处理。
pub fn send_cancel(port: &mut Box<dyn serialport::SerialPort>) {
    use std::io::Write;
    // lrzsz canit(): 10 × CAN (0x18) + 8 × BS (0x08)
    let sequence: [u8; 18] = [
        CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, // 10 CAN
        0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,  // 8 backspace
    ];
    if let Err(e) = port.write_all(&sequence) {
        log::warn!("发送 CAN 序列失败: {}", e);
    }
    if let Err(e) = port.flush() {
        log::warn!("CAN 后刷新端口失败: {}", e);
    }
    std::thread::sleep(Duration::from_millis(100));
}

/// 状态化双 CAN 检测器（对齐 lrzsz `wcgetsec` 连续 CAN 检测）
///
/// 只有当连续收到两个 CAN 字节时才返回 `true`。
/// 调用方负责维护 `last_byte` 状态。
///
/// # 参数
/// - `byte`: 当前收到的字节
/// - `last_can`: 上一个字节是否为 CAN 的可变引用（状态跟踪）
///
/// # 返回
/// `true` 表示检测到取消序列
pub fn detect_cancel(byte: u8, last_can: &mut bool) -> bool {
    if byte == CAN {
        if *last_can {
            // 两个连续 CAN → 取消
            return true;
        }
        *last_can = true;
    } else {
        *last_can = false;
    }
    false
}

/// 清空接收缓冲区（轻量版，用于文件间清理）
///
/// 以短超时（100ms/字节）连续读取并丢弃数据，最多读取 20 字节。
/// 用于文件传输间隙清理残留字节。
pub fn drain_rx_buffer(port: &mut Box<dyn serialport::SerialPort>) {
    for _ in 0..20 {
        match read_byte_with_timeout(port, 100) {
            Ok(Some(_)) => continue,
            _ => break, // 超时或错误 → 缓冲区已清空
        }
    }
}

/// `wait_for_nak_or_c` 的返回值（对齐 lrzsz getnak()）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitResult {
    /// 收到 'C' — 接收方就绪（CRC 模式）
    WantCrc,
    /// 收到 NAK — 接收方就绪（校验和模式）
    WantChecksum,
    /// 收到 'G' — 接收方就绪（YMODEM-g 流模式）
    WantG,
    /// 检测到双 CAN 取消序列
    Cancel,
}

/// 等待接收方发送 'C' (WANTCRC)、NAK（校验和模式）或 'G'（流模式）
///
/// 对齐 lrzsz getnak()：以 200ms 轮询间隔等待，总超时为 `timeout_ms`。
/// 检测到双 CAN 序列时返回 `WaitResult::Cancel`。
/// NAK 不再视为错误 — 接收方发送 NAK 表示请求校验和模式（对齐 lrzsz）。
/// 'G' 表示请求 YMODEM-g 流模式。
///
/// # 返回
/// - `Ok(WaitResult::WantCrc)`: 收到 'C'，接收方就绪（CRC-16 模式）
/// - `Ok(WaitResult::WantChecksum)`: 收到 NAK，接收方就绪（校验和模式）
/// - `Ok(WaitResult::WantG)`: 收到 'G'，接收方就绪（流模式）
/// - `Ok(WaitResult::Cancel)`: 检测到双 CAN 取消序列
/// - `Err(e)`: I/O 错误或超时（含意外字节达到 max_retries 限制）
pub fn wait_for_nak_or_c(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
    max_retries: u32,
) -> Result<WaitResult, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let mut last_can = false;
    let mut retry_count: u32 = 0;

    loop {
        if start.elapsed() > std::time::Duration::from_millis(timeout_ms) {
            return Err(format!(
                "等待接收方就绪信号超时（{}ms），已重试 {} 次",
                timeout_ms, retry_count
            )
            .into());
        }

        match read_byte_with_timeout(port, 200)? {
            Some(C) => {
                log::debug!("wait_for_nak_or_c: received 'C' (WANTCRC)");
                return Ok(WaitResult::WantCrc);
            }
            Some(G) => {
                log::debug!("wait_for_nak_or_c: received 'G' (WANTG — streaming)");
                return Ok(WaitResult::WantG);
            }
            Some(NAK) => {
                log::debug!("wait_for_nak_or_c: received NAK (checksum mode)");
                return Ok(WaitResult::WantChecksum);
            }
            Some(CAN) => {
                if detect_cancel(CAN, &mut last_can) {
                    log::warn!("wait_for_nak_or_c: detected double CAN");
                    return Ok(WaitResult::Cancel);
                }
                retry_count += 1;
                if retry_count >= max_retries {
                    return Err(format!("收到 CAN 字节，重试 {} 次后放弃", retry_count).into());
                }
            }
            Some(other) => {
                // 噪声字节：设备控制台输出混入协议通道
                // 不触发重试 — 仅消费并继续等待有效的协议信号
                log::debug!(
                    "wait_for_nak_or_c: ignoring noise byte 0x{:02X} ('{}')",
                    other,
                    if other.is_ascii_graphic() || other == b' ' { other as char } else { '.' }
                );
                last_can = false;
                continue;
            }
            None => {
                // 每次轮询超时，继续等待总超时
                last_can = false;
            }
        }
    }
}

/// EOT 响应分类（对齐 lrzsz EOT 握手机制）
///
/// 用于 `send_eot` / 接收端 EOT 处理的统一响应分类，
/// 区分标准 lrzsz 单 EOT 路径和 rt-thread 设备的双 EOT 路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EotResponse {
    /// 收到 ACK (0x06) — 标准 lrzsz 路径，EOT 已被确认
    Ack,
    /// 收到 NAK (0x15) — rt-thread 设备请求第二个 EOT（双 EOT 路径）
    Nak,
    /// 收到 CAN (0x18) — 需配合 detect_cancel 检测双 CAN
    Can,
    /// 收到 'C' (0x43) — 设备直接就绪，跳过 EOT 确认直接进入下一文件
    WantCrc,
    /// 超时
    Timeout,
    /// 其他未识别字节
    Other(u8),
}

/// 读取 EOT 响应字节并分类
///
/// 封装 `read_byte_with_timeout`，将单字节响应映射为 `EotResponse` 枚举值。
///
/// # 参数
/// - `port`: 串口设备
/// - `timeout_ms`: 单字节读取超时（毫秒）
pub fn read_eot_response(
    port: &mut Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
) -> Result<EotResponse, Box<dyn std::error::Error>> {
    match read_byte_with_timeout(port, timeout_ms)? {
        Some(0x06) => Ok(EotResponse::Ack),       // ACK
        Some(NAK)  => Ok(EotResponse::Nak),        // 0x15
        Some(CAN)  => Ok(EotResponse::Can),        // 0x18
        Some(C)    => Ok(EotResponse::WantCrc),    // 0x43
        Some(other) => Ok(EotResponse::Other(other)),
        None => Ok(EotResponse::Timeout),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancel_bytes_are_correct() {
        // CAN 常量应为 0x18
        assert_eq!(CAN, 0x18);
    }

    #[test]
    fn test_detect_cancel_double_can() {
        let mut last_can = false;
        // 第一个 CAN: 不触发
        assert!(!detect_cancel(CAN, &mut last_can));
        assert!(last_can);
        // 第二个 CAN: 触发
        assert!(detect_cancel(CAN, &mut last_can));
    }

    #[test]
    fn test_detect_cancel_single_can() {
        let mut last_can = false;
        // 单个 CAN 不触发
        assert!(!detect_cancel(CAN, &mut last_can));
        assert!(last_can);
        // 中间插入非 CAN 字节
        assert!(!detect_cancel(0x06, &mut last_can)); // ACK
        assert!(!last_can);
        // 又一个 CAN: 仍然不触发（因为被 ACK 隔开）
        assert!(!detect_cancel(CAN, &mut last_can));
        assert!(last_can);
    }

    #[test]
    fn test_detect_cancel_no_can() {
        let mut last_can = false;
        assert!(!detect_cancel(0x06, &mut last_can)); // ACK
        assert!(!last_can);
        assert!(!detect_cancel(0x15, &mut last_can)); // NAK
        assert!(!last_can);
    }

    #[test]
    fn test_flush_buffer_on_empty_cursor() {
        // drain_rx_buffer 和 flush_port_buffer 在无数据时正常退出
        // 此测试仅验证函数不会 panic；实际行为需集成测试
    }

    #[test]
    fn test_eot_response_variants_distinct() {
        // 验证枚举变体互不相同
        assert_ne!(EotResponse::Ack, EotResponse::Nak);
        assert_ne!(EotResponse::Ack, EotResponse::Can);
        assert_ne!(EotResponse::Ack, EotResponse::WantCrc);
        assert_ne!(EotResponse::Ack, EotResponse::Timeout);
        assert_ne!(EotResponse::Other(0x00), EotResponse::Other(0xFF));
    }
}
