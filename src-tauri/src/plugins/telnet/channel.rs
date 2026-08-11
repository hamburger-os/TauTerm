//! Telnet 通道实现
//!
//! 包装 `telnet` crate 的 `Telnet`（RFC 854 协议层：IAC 解析、0xFF 转义、
//! 协商/子协商收发）实现内核 `Channel` trait。
//! 本模块只负责**协商策略**与**回显状态跟踪**，IAC 状态机由 `telnet` crate 处理。

use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use telnet::{Action, Event, Telnet, TelnetOption};

use crate::channel::{error::ChannelError, Channel};

/// 通道读超时：驱动 read() 排空循环终止（与串口 50ms 读超时策略一致）
pub(crate) const READ_TIMEOUT: Duration = Duration::from_millis(50);

/// Telnet 通道
///
/// 包装 `telnet::Telnet`。`probe` 为 `try_clone` 的 socket 句柄，
/// 用于 EOF 检测与超时管理（克隆共享底层 socket 与超时配置）。
pub struct TelnetChannel {
    telnet: Telnet,
    probe: std::net::TcpStream,
    /// 回显状态变化回调（由适配器注入；内部捕获 AppHandle 与 session_id 槽）
    on_echo_change: Box<dyn Fn(bool) + Send>,
    /// session_id 槽：I/O 循环启动时经 `on_session_started` 注入，
    /// 回调据此携带正确的会话标识（槽未注入时仅记录日志，不丢关键状态）。
    session_id_slot: Arc<Mutex<Option<String>>>,
    /// 剥离 IAC 后的净载荷缓冲（残留数据跨 read() 调用保留，绝不丢弃）
    clean_buffer: Vec<u8>,
    /// 当前回显协商状态：false = 服务器回显，true = 客户端本地回显
    local_echo: bool,
    connected: bool,
}

impl TelnetChannel {
    pub fn new(
        telnet: Telnet,
        probe: std::net::TcpStream,
        on_echo_change: Box<dyn Fn(bool) + Send>,
        session_id_slot: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            telnet,
            probe,
            on_echo_change,
            session_id_slot,
            clean_buffer: Vec::with_capacity(16384),
            local_echo: false,
            connected: true,
        }
    }

    /// 协商策略表（RFC 854/856/857/858/1073）
    ///
    /// `telnet` crate 不自动响应协商，这里定义客户端策略：
    /// - ECHO：跟随服务器（WILL → 服务器回显；WONT → 客户端本地回显）
    /// - SGA / BINARY：接受双向协商
    /// - NAWS：我们主动 WILL NAWS，接受服务器 DO 应答
    /// - 未知选项：一律拒绝（RFC 854 要求）
    fn handle_negotiation(&mut self, action: Action, opt: TelnetOption) {
        let prev_echo = self.local_echo;
        match (action, opt) {
            (Action::Will, TelnetOption::Echo) => {
                self.local_echo = false;
                let _ = self.telnet.negotiate(&Action::Do, TelnetOption::Echo);
            }
            (Action::Wont, TelnetOption::Echo) => {
                self.local_echo = true;
                let _ = self.telnet.negotiate(&Action::Dont, TelnetOption::Echo);
            }
            // 服务器要求我们回显：拒绝（客户端不做服务器角色）
            (Action::Do, TelnetOption::Echo) => {
                let _ = self.telnet.negotiate(&Action::Wont, TelnetOption::Echo);
            }
            (Action::Will, TelnetOption::SuppressGoAhead) => {
                let _ = self.telnet.negotiate(&Action::Do, TelnetOption::SuppressGoAhead);
            }
            (Action::Do, TelnetOption::SuppressGoAhead) => {
                let _ = self.telnet.negotiate(&Action::Will, TelnetOption::SuppressGoAhead);
            }
            (Action::Will, TelnetOption::TransmitBinary) => {
                let _ = self.telnet.negotiate(&Action::Do, TelnetOption::TransmitBinary);
            }
            (Action::Do, TelnetOption::TransmitBinary) => {
                let _ = self.telnet.negotiate(&Action::Will, TelnetOption::TransmitBinary);
            }
            // NAWS：服务器 DO NAWS 是对我们 WILL NAWS 的应答 → 确认；
            // 服务器 WILL NAWS（倒置用法）→ 拒绝
            (Action::Do, TelnetOption::NAWS) => {
                let _ = self.telnet.negotiate(&Action::Will, TelnetOption::NAWS);
            }
            (Action::Will, TelnetOption::NAWS) => {
                let _ = self.telnet.negotiate(&Action::Dont, TelnetOption::NAWS);
            }
            // 未知/不支持选项：一律拒绝
            (Action::Will, _) => {
                let _ = self.telnet.negotiate(&Action::Dont, opt);
            }
            (Action::Do, _) => {
                let _ = self.telnet.negotiate(&Action::Wont, opt);
            }
            // WONT/DONT 是应答，无需再应答
            (Action::Wont, _) | (Action::Dont, _) => {}
        }
        // 回显状态只在变化时通知前端（避免冗余事件）
        if self.local_echo != prev_echo {
            (self.on_echo_change)(self.local_echo);
        }
    }
}

impl Read for TelnetChannel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // 残留数据优先交付：上次 read() 未消费完的净载荷先返回（不排空），
        // 由 I/O 循环连续 read 追平。绝不因清除缓冲而丢弃数据。
        if !self.clean_buffer.is_empty() {
            return self.serve_clean(buf);
        }

        // EOF 检测必须先行：telnet crate 在 socket EOF（read 返回 Ok(0)）时
        // 会无限空转（read 循环永不产出事件），故每次排空前用 peek 探针。
        match self.probe.peek(&mut [0u8; 1]) {
            Ok(0) => {
                // EOF：服务器关闭连接
                self.connected = false;
                return Err(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "Telnet 服务器已断开连接",
                ));
            }
            // 无待读数据：空闲，直接返回（io loop 进入空闲等待）
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                return Ok(0);
            }
            Err(e) => {
                self.connected = false;
                return Err(e);
            }
            Ok(_) => {}
        }

        // 有数据待读：排空事件队列（read_timeout 终止于 TimedOut，队列必为空）。
        // 首个事件后切换为 1ms 轮询超时：空 socket 立即 WouldBlock/TimedOut 退出，
        // 把每个数据批次尾部的固定停滞从 50ms 降到 1ms（Windows SO_RCVTIMEO
        // 最小粒度为 1ms，Duration::ZERO 会被 set_read_timeout 拒绝）。
        // 超时在下一次 read() 调用恢复为 READ_TIMEOUT。
        let mut timeout = READ_TIMEOUT;
        loop {
            match self.telnet.read_timeout(timeout) {
                Ok(Event::Data(d)) => {
                    self.clean_buffer.extend_from_slice(&d);
                    timeout = Duration::from_millis(1);
                }
                Ok(Event::Negotiation(action, opt)) => {
                    self.handle_negotiation(action, opt);
                    timeout = Duration::from_millis(1);
                }
                // Subnegotiation（如服务器 NAWS 请求）与未知 IAC：RFC 854 容错吸收
                Ok(Event::Subnegotiation(_, _)) | Ok(Event::UnknownIAC(_)) => {
                    timeout = Duration::from_millis(1);
                }
                Ok(Event::TimedOut) | Ok(Event::NoData) => break,
                Ok(Event::Error(e)) => {
                    self.connected = false;
                    // 已积累数据先交付，错误顺延至下一次 read()（probe 会再次捕获）
                    if !self.clean_buffer.is_empty() {
                        return self.serve_clean(buf);
                    }
                    return Err(std::io::Error::other(format!("Telnet 协议错误: {e}")));
                }
                Err(e)
                    if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock =>
                {
                    break;
                }
                Err(e) => {
                    self.connected = false;
                    if !self.clean_buffer.is_empty() {
                        return self.serve_clean(buf);
                    }
                    return Err(e);
                }
            }
        }

        self.serve_clean(buf)
    }
}

impl TelnetChannel {
    /// 从 clean_buffer 头部拷贝最多 `buf.len()` 字节并原地移除已消费前缀，
    /// 余量保留供下一次 read() 继续消费（顺序正确：残留在前，新数据在后）。
    fn serve_clean(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.clean_buffer.len().min(buf.len());
        if n > 0 {
            buf[..n].copy_from_slice(&self.clean_buffer[..n]);
            self.clean_buffer.drain(..n);
        }
        Ok(n)
    }
}

impl Write for TelnetChannel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // crate 的 write() 已处理 0xFF → IAC IAC 转义（BINARY 模式安全）
        self.telnet.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.probe.flush()
    }
}

impl Channel for TelnetChannel {
    /// I/O 循环启动回调：注入 session_id，保证回显事件 emit 携带正确的会话标识。
    /// I/O 循环在读循环前调用此钩子，故协商事件触发回调时槽必已注入。
    fn on_session_started(&mut self, session_id: &str) {
        *self.session_id_slot.lock().unwrap() = Some(session_id.to_string());
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError> {
        self.probe
            .set_read_timeout(Some(dur))
            .map_err(|e| ChannelError::Io(e.into()))
    }

    /// 发送 NAWS 窗口尺寸（RFC 1073）：
    /// `IAC SB NAWS w_hi w_lo h_hi h_lo IAC SE`（如 132×43 → FF FA 1F 00 84 00 2B FF F0）
    fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<(), ChannelError> {
        let w = cols.clamp(1, u16::MAX as u32) as u16;
        let h = rows.clamp(1, u16::MAX as u32) as u16;
        let payload = [
            (w >> 8) as u8,
            (w & 0xFF) as u8,
            (h >> 8) as u8,
            (h & 0xFF) as u8,
        ];
        self.telnet
            .subnegotiate(TelnetOption::NAWS, &payload)
            .map_err(|e| ChannelError::Io(std::io::Error::other(format!("NAWS 发送失败: {e}"))))
    }
}
