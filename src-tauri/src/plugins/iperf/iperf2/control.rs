//! iperf2 连接握手（对齐 2.2.1 源码，单连接模型）
//!
//! TCP（2.2.1）：客户端连接服务器端口后，**首个载荷即为 64B 测试头**
//! （`client_hdr_v1` + `client_hdrext`），之后数据在同一连接上传输——
//! 不存在独立的数据端口（无"控制端口 + 1"）。
//! 服务器读完测试头后回 `client_hdr_ack`（28B，flags 含 EXTEND 且非 VERSION2 时）。
//!
//! 所有读写使用阻塞 I/O + 超时，支持 `abort_flag` 中止。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};

use super::test_hdr::{
    client_test_hdr_len, tcp_first_payload, ClientHdrAck, ClientHdrExt, ClientHdrV1,
    HEADER_EXTEND, HEADER_VERSION2, MAX_HEADER_LEN,
};
use super::types::ServerTestMode;

/// 连接/读写超时
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
/// 服务器 ack 读取超时（2.1+ 服务器必回；2.0.x 不回，容忍缺失）
const ACK_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// 无进展连续超时次数上限（约 30s）后放弃
const MAX_STALLED_TIMEOUTS: usize = 3;

/// 服务端握手结果（测试头 + 接待模式 + 对端地址）
pub struct HandshakeResult {
    pub hdr: ClientHdrV1,
    /// 依据 flags 判定的接待模式（-d/-r/普通）
    pub mode: ServerTestMode,
    /// 对端地址（-d 反向 connect 目标）
    pub peer_addr: SocketAddr,
}

/// 客户端连接握手：连接服务器端口、发送 64B 测试头、读取 28B ack（容忍缺失）。
///
/// `listen_port`：-d 模式下客户端反向监听端口（写入头部 `mPort`，对齐 2.2.1
/// `Settings_GenerateClientHdrV1`：mListenPort 优先、否则服务器端口）。
/// 返回已就绪的数据流——该连接即为数据连接。
pub fn client_handshake(
    server_addr: &str,
    params: &super::types::Iperf2TestParams,
    listen_port: Option<u16>,
    abort: &Arc<AtomicBool>,
) -> Result<TcpStream, String> {
    check_abort(abort)?;
    let addrs: Vec<_> = server_addr
        .to_socket_addrs()
        .map_err(|e| format!("无法解析服务器地址 {}: {}", server_addr, e))?
        .collect();

    // 64B 测试头（v1 + extend；TCP flags = EXTEND|LEN_BIT|(64<<1) = 0x4001_0080；
    // 方向模式附加 VERSION1/RUN_NOW 位）
    let mut base = ClientHdrV1::new_client(
        false,
        params.parallel_streams,
        params.port,
        params.duration_secs,
    );
    if let Some(lp) = listen_port {
        base.m_port = lp as i32;
    }
    let base = base.with_direction(params.direction);
    let ext = ClientHdrExt::new_client(params.bandwidth_bps);
    let header_bytes = tcp_first_payload(&base, &ext);

    let mut last_err = String::from("无可用地址");
    for addr in addrs {
        check_abort(abort)?;
        let domain = if addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };
        let sock = match Socket::new(domain, Type::STREAM, Some(Protocol::TCP)) {
            Ok(s) => s,
            Err(e) => {
                last_err = format!("{}: {}", addr, e);
                continue;
            }
        };
        // -w：connect 之前设 SO_SNDBUF（对齐 tcp_window_size.c：Windows 上
        // >64KB 仅在 connect() 前设置生效；设置失败静默降级为系统默认）
        if let Some(w) = params.window_size {
            if let Err(e) = sock.set_send_buffer_size(w as usize) {
                log::debug!("[iperf2] SO_SNDBUF({}) 设置失败（忽略）: {}", w, e);
            } else if let Ok(effective) = sock.send_buffer_size() {
                log::debug!("[iperf2] SO_SNDBUF 请求 {} 实际 {}（OS 可能钳制）", w, effective);
            }
        }
        match sock.connect_timeout(&addr.into(), CONTROL_TIMEOUT) {
            Ok(()) => {
                let stream: TcpStream = sock.into();
                return finish_handshake(stream, &header_bytes, &base, abort);
            }
            Err(e) => last_err = format!("{}: {}", addr, e),
        }
    }
    Err(format!("无法连接 iperf2 服务端 {}: {}", server_addr, last_err))
}

/// 发送测试头 + 读 ack（连接成功后）
fn finish_handshake(
    mut stream: TcpStream,
    header_bytes: &[u8],
    base: &ClientHdrV1,
    abort: &Arc<AtomicBool>,
) -> Result<TcpStream, String> {
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    stream
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|e| format!("设置写超时失败: {}", e))?;
    check_abort(abort)?;

    stream
        .write_all(header_bytes)
        .map_err(|e| format!("发送测试头失败: {}", e))?;
    log::info!(
        "[iperf2] 客户端已发送测试头 ({}B, flags=0x{:08X}, port={}, threads={}, time={}s)",
        header_bytes.len(),
        base.flags,
        base.m_port,
        base.num_threads,
        base.time_secs()
    );

    // 读取服务器 ack（避免关闭时因未读数据触发 Windows RST）
    read_server_ack(&mut stream)?;
    Ok(stream)
}

/// 读取服务器 `client_hdr_ack`（28B）。EOF/超时/非 ack 一律容忍。
fn read_server_ack(stream: &mut TcpStream) -> Result<(), String> {
    let saved = stream
        .read_timeout()
        .map_err(|e| format!("读取超时配置失败: {}", e))?;
    stream
        .set_read_timeout(Some(ACK_READ_TIMEOUT))
        .map_err(|e| format!("设置读超时失败: {}", e))?;

    let mut buf = [0u8; ClientHdrAck::SIZE];
    let mut got = 0usize;
    while got < ClientHdrAck::SIZE {
        match stream.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    let _ = stream.set_read_timeout(saved);

    if got == ClientHdrAck::SIZE {
        if let Ok(ack) = ClientHdrAck::deserialize(&buf) {
            if ack.is_ack() {
                log::debug!(
                    "[iperf2] 已读服务器 ack (version_u=0x{:08X}, version_l=0x{:08X})",
                    ack.version_u,
                    ack.version_l
                );
            }
        }
    } else {
        log::debug!("[iperf2] 服务器 ack 未完整收到（{}B，容忍）", got);
    }
    Ok(())
}

/// 服务端握手：读取客户端测试头（长度按 flags 推导）并回 `client_hdr_ack`。
///
/// 返回测试头、接待模式与对端地址。端口探测/非法连接返回 Err（调用方静默退出）。
pub fn server_handshake(
    stream: &mut TcpStream,
    abort: &Arc<AtomicBool>,
) -> Result<HandshakeResult, String> {
    let peer_addr = stream
        .peer_addr()
        .map_err(|e| format!("获取对端地址失败: {}", e))?;
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|e| format!("设置读超时失败: {}", e))?;
    stream
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|e| format!("设置写超时失败: {}", e))?;

    // 1. 读 4B flags（不足 4B 视为探测/提前关闭）
    let mut buf = [0u8; MAX_HEADER_LEN];
    let mut received = 0usize;
    read_with_stall(stream, abort, &mut buf[..4], &mut received, 4)
        .map_err(|e| format!("读取测试头 flags 失败: {}", e))?;

    let flags = u32::from_be_bytes(buf[0..4].try_into().unwrap());

    // 2. 按 flags 推导测试头总长度（Settings_ClientTestHdrLen）
    let Some(hdr_len) = client_test_hdr_len(flags) else {
        return Err(format!("非 v1/v2 测试头 flags=0x{:08X}（可能为端口探测）", flags));
    };

    // 3. 读完剩余部分（received 为绝对偏移，须传入完整切片）
    read_with_stall(stream, abort, &mut buf[..hdr_len], &mut received, hdr_len)
        .map_err(|e| format!("读取测试头失败: {}", e))?;

    let hdr = ClientHdrV1::deserialize(&buf[..24])?;
    let mode = hdr.server_mode();
    log::info!(
        "[iperf2] 服务端收到 v1 头: flags=0x{:08X}, port={}, threads={}, udp={}, amount={}, mode={:?}, len={}B",
        hdr.flags,
        hdr.m_port,
        hdr.num_threads,
        hdr.is_udp(),
        hdr.m_amount,
        mode,
        hdr_len
    );

    // 4. 回 ack（对齐 Listener.cpp：TCP + EXTEND 且非 VERSION2）
    if hdr.flags & HEADER_EXTEND != 0 && hdr.flags & HEADER_VERSION2 == 0 {
        let ack = ClientHdrAck::new_server();
        stream
            .write_all(&ack.serialize())
            .map_err(|e| format!("回发 client_hdr_ack 失败: {}", e))?;
        log::debug!("[iperf2] 已回 client_hdr_ack ({}B)", ClientHdrAck::SIZE);
    }

    Ok(HandshakeResult {
        hdr,
        mode,
        peer_addr,
    })
}

/// 阻塞读取直到 `target` 字节；超时无进展达上限即失败（防止卡死连接）。
fn read_with_stall(
    stream: &mut TcpStream,
    abort: &Arc<AtomicBool>,
    buf: &mut [u8],
    received: &mut usize,
    target: usize,
) -> Result<(), String> {
    let mut stalled = 0usize;
    while *received < target {
        check_abort(abort)?;
        match stream.read(&mut buf[*received..target]) {
            Ok(0) => return Err(format!("客户端提前关闭（仅收到 {}/{}B）", *received, target)),
            Ok(n) => {
                *received += n;
                stalled = 0;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                stalled += 1;
                if stalled >= MAX_STALLED_TIMEOUTS {
                    return Err(format!("读取超时无进展（{}/{}B）", *received, target));
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

fn check_abort(abort: &Arc<AtomicBool>) -> Result<(), String> {
    if abort.load(Ordering::Relaxed) {
        Err("已中止".into())
    } else {
        Ok(())
    }
}
