//! Telnet 协议插件
//!
//! 实现 `ProtocolAdapter` trait，提供 RFC 854 Telnet 终端会话。
//! 协议状态机由 `telnet` crate 处理，本插件负责协商策略与连接管理。

pub mod channel;

use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use telnet::{Action, Telnet, TelnetOption};

use crate::channel::error::SessionError;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    ChannelKind, EndpointInfo, PluginManifest, ProtocolAdapter, ProtocolConnection,
    TransferProtocolType,
};
use channel::{TelnetChannel, READ_TIMEOUT};

// ── Telnet 配置 ──────────────────────────────────────

/// Telnet 连接参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelnetConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 { 23 }

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 23,
        }
    }
}

// ── Telnet 适配器 ────────────────────────────────────

/// Telnet 协议适配器
///
/// 持有 `AppHandle`（setup 注入）：回显状态变化事件由通道内回调直接 emit，
/// 无需额外的 relay 线程/事件通道。
pub struct TelnetAdapter {
    app: Mutex<Option<AppHandle>>,
}

impl TelnetAdapter {
    pub fn new() -> Self { Self { app: Mutex::new(None) } }

    /// 注入全局 AppHandle（lib.rs setup 调用）。
    /// setup 在所有命令处理器就绪前运行，`connect()` 必然在注入之后执行。
    pub fn inject_app_handle(&self, app: AppHandle) {
        *self.app.lock().expect("TelnetAdapter app 锁") = Some(app);
    }

    /// 创建 Telnet 插件清单
    pub fn manifest() -> PluginManifest {
        PluginManifest {
            id: "telnet".into(),
            name: "Telnet".into(),
            version: "1.0.0".into(),
            category: "terminal".into(),
            description: "Telnet 终端".into(),
            icon: "plug".into(),
            content_type: "terminal".into(),
            capabilities: vec!["connection".into()],
            // 无文件传输：transfer_protocols 为空 → 前端 Transmission 面板不显示
            transfer_protocols: vec![],
        }
    }

    /// 从 JSON Value 解析 Telnet 参数
    ///
    /// 字段缺失/类型错误时返回错误而非静默采用默认值（避免参数被吞）。
    /// 注：host 为空时 `open_connection` 回退 127.0.0.1，仅为本机开发便利
    /// （前端连接表单已强制 host 非空）。
    fn parse_params(params: &serde_json::Value) -> Result<TelnetConfig, SessionError> {
        serde_json::from_value(params.clone()).map_err(|e| SessionError::ConnectionFailed {
            reason: format!("Telnet 参数无效: {e}"),
        })
    }

    /// 建立 TCP 连接并完成初始选项协商
    fn open_connection(config: &TelnetConfig) -> Result<(Telnet, std::net::TcpStream), SessionError> {
        let host = if config.host.is_empty() { "127.0.0.1".to_string() } else { config.host.clone() };
        // connect_timeout 要求 &SocketAddr，先解析（取第一个解析结果）
        let socket_addrs = (host.as_str(), config.port)
            .to_socket_addrs()
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("解析地址 {}:{} 失败: {}", host, config.port, e),
            })?;
        let addr = socket_addrs.as_slice().first().ok_or_else(|| SessionError::ConnectionFailed {
            reason: format!("地址 {}:{} 未解析出结果", host, config.port),
        })?.clone();
        let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("无法连接 {}:{}: {}", host, config.port, e),
            })?;
        stream.set_nodelay(true).ok();
        // TCP keepalive（长连设备场景保活；std 暂无 set_keepalive，用 socket2）
        socket2::SockRef::from(&stream)
            .set_keepalive(true)
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("设置 TCP keepalive 失败: {}", e),
            })?;
        // 读超时驱动通道 read() 空闲检测（与串口 50ms 策略一致）
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("设置读超时失败: {}", e),
            })?;
        let probe = stream
            .try_clone()
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("克隆 socket 失败: {}", e),
            })?;

        let mut telnet = Telnet::from_stream(Box::new(stream), 16384);

        // 初始协商：宣布窗口尺寸能力 + 请求 SGA/BINARY
        let _ = telnet.negotiate(&Action::Will, TelnetOption::NAWS);
        let _ = telnet.negotiate(&Action::Do, TelnetOption::SuppressGoAhead);
        let _ = telnet.negotiate(&Action::Will, TelnetOption::TransmitBinary);
        let _ = telnet.negotiate(&Action::Do, TelnetOption::TransmitBinary);

        Ok((telnet, probe))
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for TelnetAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let config = Self::parse_params(params)?;
        let (telnet, probe) = Self::open_connection(&config)?;

        // 回显状态 → 前端事件。session_id 由 I/O 循环启动时经
        // `Channel::on_session_started` 注入槽中；I/O 循环先于任何协商
        // 事件调用该钩子，故正常流程下事件必带正确标识、永不丢失。
        // 槽为空（仅测试等非 I/O 循环路径）时记录日志而不 emit。
        let session_id_slot = Arc::new(Mutex::new(None));
        // setup 在命令处理器就绪前注入 AppHandle，此处缺失仅可能是
        // 启动顺序异常（或测试直连路径），优雅返回错误而非 panic。
        let app = self
            .app
            .lock()
            .map_err(|_| SessionError::ConnectionFailed {
                reason: "TelnetAdapter app 锁已中毒".into(),
            })?
            .clone()
            .ok_or_else(|| SessionError::ConnectionFailed {
                reason: "TelnetAdapter 未注入 AppHandle（setup 未执行?）".into(),
            })?;
        let slot = session_id_slot.clone();
        let on_echo_change: Box<dyn Fn(bool) + Send> = Box::new(move |local_echo| {
            let sid = slot.lock().ok().and_then(|s| s.clone());
            match sid {
                Some(sid) => {
                    let _ = app.emit("telnet-echo-state", serde_json::json!({
                        "session_id": sid,
                        "local_echo": local_echo,
                    }));
                }
                None => log::trace!(
                    "Telnet 回显事件在 session_id 注入前到达，忽略: local_echo={local_echo}"
                ),
            }
        });
        let channel = TelnetChannel::new(telnet, probe, on_echo_change, session_id_slot);

        Ok(ProtocolConnection {
            channel: Some(ChannelKind::Sync(Box::new(channel))),
            comm_handle: None,
            side_channel: None,
            teardown_delay: self.teardown_delay(),
        })
    }

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        Ok(Vec::new())
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![]
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Sync
    }

    fn teardown_delay(&self) -> std::time::Duration {
        Duration::ZERO
    }
}

// ── 单元测试 ─────────────────────────────────────────
//
// 使用 std::net::TcpListener 回环对端验证协商策略与 NAWS 编码。
// 测试辅助函数通过适配器相同路径建立连接（复用 open_connection）。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::Channel;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::JoinHandle;

    /// 启动回环对端（线程 accept 一个连接），返回对端句柄与监听地址
    fn spawn_peer() -> (JoinHandle<TcpStream>, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 失败");
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (peer, _) = listener.accept().expect("accept 失败");
            peer.set_read_timeout(Some(Duration::from_millis(50))).ok();
            peer
        });
        (handle, addr)
    }

    /// 建立 TelnetChannel（与适配器 connect 相同路径），等待对端 accept，
    /// 返回 (channel, peer, echo_rx)：echo_rx 收集回显状态回调调用（bool）。
    /// 测试中回调用 std mpsc 收集，语义与原事件通道一致。
    fn connect_channel(
        peer_handle: JoinHandle<TcpStream>,
        addr: SocketAddr,
    ) -> (TelnetChannel, TcpStream, Receiver<bool>) {
        let config = TelnetConfig {
            host: "127.0.0.1".into(),
            port: addr.port(),
        };
        let (telnet, probe) = TelnetAdapter::open_connection(&config).expect("连接失败");
        let (events_tx, events_rx) = mpsc::channel();
        let on_echo_change = Box::new(move |local_echo: bool| {
            let _ = events_tx.send(local_echo);
        });
        let channel =
            TelnetChannel::new(telnet, probe, on_echo_change, Arc::new(Mutex::new(None)));
        let peer = peer_handle.join().expect("对端线程失败");
        (channel, peer, events_rx)
    }

    /// 从对端读 n 字节（带超时重试，容忍协商字节分片）
    fn read_exact(peer: &mut TcpStream, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n);
        let mut chunk = [0u8; 64];
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while out.len() < n && std::time::Instant::now() < deadline {
            match peer.read(&mut chunk) {
                Ok(0) => break,
                Ok(m) => out.extend_from_slice(&chunk[..m]),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => panic!("对端读失败: {e}"),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(out.len(), n, "对端读取字节数不足（超时）");
        out
    }

    /// 从对端读直到出现目标子串（用于协商字节流断言）
    fn read_until(peer: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 64];
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !contains(&out, needle) && std::time::Instant::now() < deadline {
            match peer.read(&mut chunk) {
                Ok(0) => break,
                Ok(m) => out.extend_from_slice(&chunk[..m]),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => panic!("对端读失败: {e}"),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(contains(&out, needle), "对端未收到目标字节 {:02X?}，实际: {:02X?}", needle, out);
        out
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() { return true; }
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// 消费初始协商字节（WILL NAWS / DO SGA / WILL BINARY / DO BINARY，共 12 字节）
    fn consume_initial_negotiation(peer: &mut TcpStream) {
        let _ = read_until(peer, &[0xFF, 0xFD, 0x00]); // 直到 DO BINARY
    }

    /// 初始协商：客户端应主动发送 WILL NAWS / DO SGA / WILL BINARY / DO BINARY
    #[test]
    fn initial_negotiation_sent() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        // 触发一次排空（无数据，仅验证连接通路）
        let _ = channel.read(&mut [0u8; 64]).unwrap();
        let received = {
            let mut out = Vec::new();
            let mut chunk = [0u8; 64];
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match peer.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(m) => out.extend_from_slice(&chunk[..m]),
                    Err(_) => {}
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            out
        };
        assert!(contains(&received, &[0xFF, 0xFB, 0x1F]), "缺少 WILL NAWS: {:02X?}", received);
        assert!(contains(&received, &[0xFF, 0xFD, 0x03]), "缺少 DO SGA: {:02X?}", received);
        assert!(contains(&received, &[0xFF, 0xFB, 0x00]), "缺少 WILL BINARY: {:02X?}", received);
        assert!(contains(&received, &[0xFF, 0xFD, 0x00]), "缺少 DO BINARY: {:02X?}", received);
    }

    /// 服务器 WILL ECHO → 客户端应答 DO ECHO，本地回显保持关闭（无事件）
    #[test]
    fn server_will_echo_reply_do() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = peer.write_all(&[0xFF, 0xFB, 0x01]); // IAC WILL ECHO
        let _ = channel.read(&mut [0u8; 64]).unwrap();
        let received = read_until(&mut peer, &[0xFF, 0xFD, 0x01]); // IAC DO ECHO
        assert!(contains(&received, &[0xFF, 0xFD, 0x01]), "缺少 DO ECHO 应答: {:02X?}", received);
        // 本地回显保持关闭：不应有事件
        assert!(rx.try_recv().is_err(), "WILL ECHO 不应触发回显事件");
    }

    /// 服务器 WONT ECHO → 客户端应答 DONT ECHO，回显事件 EchoState(true)
    #[test]
    fn server_wont_echo_local_echo_event() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = peer.write_all(&[0xFF, 0xFC, 0x01]); // IAC WONT ECHO
        let _ = channel.read(&mut [0u8; 64]).unwrap();
        let received = read_until(&mut peer, &[0xFF, 0xFE, 0x01]); // IAC DONT ECHO
        assert!(contains(&received, &[0xFF, 0xFE, 0x01]), "缺少 DONT ECHO 应答: {:02X?}", received);
        // 回显状态变化事件（回调注入 bool）
        let event = rx.try_recv().expect("应收到回显事件");
        assert!(event, "回显事件应为 true");
    }

    /// 服务器 DO ECHO → 拒绝（WONT ECHO）
    #[test]
    fn server_do_echo_rejected() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = peer.write_all(&[0xFF, 0xFD, 0x01]); // IAC DO ECHO
        let _ = channel.read(&mut [0u8; 64]).unwrap();
        let received = read_until(&mut peer, &[0xFF, 0xFC, 0x01]); // IAC WONT ECHO
        assert!(contains(&received, &[0xFF, 0xFC, 0x01]), "缺少 WONT ECHO 应答: {:02X?}", received);
    }

    /// 未知选项 → 拒绝（WONT/DONT）
    #[test]
    fn unknown_option_rejected() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = peer.write_all(&[0xFF, 0xFB, 0x2A]); // IAC WILL 42（未知选项）
        let _ = channel.read(&mut [0u8; 64]).unwrap();
        let received = read_until(&mut peer, &[0xFF, 0xFE, 0x2A]); // IAC DONT 42
        assert!(contains(&received, &[0xFF, 0xFE, 0x2A]), "缺少 DONT 应答: {:02X?}", received);
    }

    /// NAWS 编码：132×43 → FF FA 1F 00 84 00 2B FF F0
    #[test]
    fn naws_encoding() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        channel.resize_pty(132, 43).expect("NAWS 发送失败");
        let received = read_exact(&mut peer, 9);
        assert_eq!(
            received,
            vec![0xFF, 0xFA, 0x1F, 0x00, 0x84, 0x00, 0x2B, 0xFF, 0xF0],
            "NAWS 字节序列错误: {:02X?}", received
        );
    }

    /// 净载荷透传：服务器发送数据 → read() 返回；IAC IAC 还原为 0xFF
    #[test]
    fn payload_passthrough() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = peer.write_all(b"Hello\r\n");
        let mut buf = [0u8; 64];
        let n = channel.read(&mut buf).expect("read 失败");
        assert_eq!(&buf[..n], b"Hello\r\n", "净载荷透传错误");

        // 转义 IAC：FF FF → 0xFF
        let _ = peer.write_all(&[0x41, 0xFF, 0xFF, 0x42]);
        let n = channel.read(&mut buf).expect("read 失败");
        assert_eq!(&buf[..n], &[0x41, 0xFF, 0x42], "IAC 转义还原错误");
    }

    /// 数据保留：单次排空积累超过调用方缓冲区（16KB）时，
    /// 余量必须跨 read() 调用保留，绝不因清空缓冲而丢失（回归：clean_buffer.clear() 截断）。
    #[test]
    fn large_burst_retained_across_reads() {
        let payload: Vec<u8> = (0..40 * 1024).map(|i| (i % 251) as u8).collect();
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        peer.write_all(&payload).expect("对端写入失败");

        let mut buf = [0u8; 16384];
        let mut total = 0usize;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while total < payload.len() && std::time::Instant::now() < deadline {
            let n = channel.read(&mut buf).expect("read 失败");
            if n == 0 {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            assert_eq!(&buf[..n], &payload[total..total + n], "数据错位");
            total += n;
        }
        assert_eq!(total, payload.len(), "数据被截断: {total}/{}", payload.len());
    }

    /// 尾包交付：服务器发送数据后立即关闭连接 →
    /// 已积累数据先交付，EOF 顺延至下一次 read()（回归：错误路径丢弃 clean_buffer）。
    #[test]
    fn tail_packet_delivered_on_eof() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let payload = b"last message\r\n";
        peer.write_all(payload).expect("对端写入失败");
        drop(peer); // FIN

        let mut buf = [0u8; 64];
        let mut n = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while n == 0 && std::time::Instant::now() < deadline {
            n = channel.read(&mut buf).expect("read 失败");
            if n == 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(&buf[..n], payload, "尾包丢失");

        // 第二次 read：probe 捕获 EOF
        let err = channel.read(&mut buf).expect_err("应报 EOF 错误");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    /// EOF：服务器关闭 → read() 返回 UnexpectedEof，通道标记断开
    #[test]
    fn eof_detected() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        // 先消费初始协商字节：peer 带着未读数据关闭会触发 RST（而非 FIN）
        consume_initial_negotiation(&mut peer);
        drop(peer);
        std::thread::sleep(Duration::from_millis(100));
        let err = channel.read(&mut [0u8; 64]).expect_err("应返回 EOF 错误");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert!(!channel.is_connected());
    }

    /// 写入透传：0xFF 被转义为 IAC IAC
    #[test]
    fn write_escapes_iac() {
        let (peer_handle, addr) = spawn_peer();
        let (mut channel, mut peer, _rx) = connect_channel(peer_handle, addr);
        consume_initial_negotiation(&mut peer);
        let _ = channel.write(&[0x41, 0xFF, 0x42]).expect("write 失败");
        let received = read_exact(&mut peer, 4);
        assert_eq!(received, vec![0x41, 0xFF, 0xFF, 0x42], "写入转义错误: {:02X?}", received);
    }
}
