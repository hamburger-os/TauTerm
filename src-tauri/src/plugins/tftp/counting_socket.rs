//! 计数 Socket — 实现 `tftpd::Socket` trait，统计收发字节数用于进度上报。
//!
//! 在每次 `send`/`recv` 时累加字节和块计数，通过回调通知上层进度变化。

use std::error::Error;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tftpd::{Packet, Socket};

/// 传输进度快照
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// 已传输字节数（发送 + 接收）
    pub bytes_transferred: u64,
    /// 总字节数（0 = 未知大小）
    pub total_bytes: u64,
    /// 已传输块数
    pub blocks_transferred: u64,
    /// 传输速度（bytes/s），EMA 平滑后的值
    pub bytes_per_second: u64,
}

/// 进度回调：每次 I/O 操作后调用（受节流控制）
pub type ProgressCallback = Box<dyn Fn(TransferProgress) + Send + Sync>;

/// 节流参数：进度回调最小发射间隔
const THROTTLE_MIN_INTERVAL_MS: u64 = 100;
/// 节流参数：距离上次发射的最小字节增量（64 KB）
const THROTTLE_MIN_BYTES: u64 = 65536;

/// 计数 Socket
///
/// 包装 `std::net::UdpSocket`，在每次 `send`/`recv` 时：
/// 1. 累加 `bytes_sent` / `bytes_received` 原子计数器
/// 2. 调用 `progress_cb` 回调（如果设置）
///
/// 典型用法：
/// ```ignore
/// let sock = CountingSocket::new(udp, peer);
/// sock.set_total_size(file_size);
/// sock.set_progress_callback(Box::new(move |p| { emit_event(p); }));
/// transfer::send_file(&mut sock, path, peer, params, abort)?;
/// ```
pub struct CountingSocket {
    inner: UdpSocket,
    remote: SocketAddr,
    /// 协商后的数据块大小（字节），用于 `recv()` 的接收缓冲区。
    blksize: usize,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    file_bytes: AtomicU64,
    total_size: AtomicU64,
    blocks_transferred: AtomicU64,
    progress_cb: std::sync::Mutex<Option<ProgressCallback>>,
    recv_buf: Mutex<Vec<u8>>,
    // 速度计算（EMA）+ 节流状态
    speed_state: Mutex<SpeedState>,
}

struct SpeedState {
    last_bytes: u64,
    last_time: Instant,
    ema: f64,
    /// 上次触发进度回调的时刻
    last_emit_time: Instant,
    /// 上次触发进度回调时的文件字节数
    file_bytes_at_last_emit: u64,
}

impl CountingSocket {
    pub fn new(socket: UdpSocket, remote: SocketAddr, blksize: usize) -> Self {
        let now = Instant::now();
        Self {
            inner: socket,
            remote,
            blksize,
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            file_bytes: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
            blocks_transferred: AtomicU64::new(0),
            progress_cb: std::sync::Mutex::new(None),
            recv_buf: Mutex::new(Vec::new()),
            speed_state: Mutex::new(SpeedState {
                last_bytes: 0,
                last_time: now,
                ema: 0.0,
                last_emit_time: now,
                file_bytes_at_last_emit: 0,
            }),
        }
    }

    /// 返回协商后的数据块大小（字节），供 `send_file`/`receive_file` 使用。
    /// 该值与实际收发缓冲区大小精确一致，避免使用 `params.blksize` 时
    /// 因参数更新竞态导致协商值 ≠ 本地参数值。
    pub fn blksize(&self) -> usize {
        self.blksize
    }

    /// 设置总传输大小（用于进度百分比计算）。
    pub fn set_total_size(&self, size: u64) {
        self.total_size.store(size, Ordering::Relaxed);
    }

    /// 补录已接收字节（用于在 CountingSocket 创建之前已接收的数据）。
    /// 会触发节流后的进度回调，确保首块数据体现在进度百分比中。
    ///
    /// 当前由 `record_data_bytes` 覆盖了使用场景，保留此方法作为公共 API
    /// 供未来扩展（如协议层字节计数与文件字节计数分离的场景）。
    #[allow(dead_code)]
    pub fn record_received_bytes(&self, n: u64) {
        self.bytes_received.fetch_add(n, Ordering::Relaxed);
        self.emit_progress();
    }

    /// 记录文件数据字节（区别于线字节，不含 TFTP 协议头开销）。
    /// 每块数据写入后由上层调用，触发节流后的进度回调。
    pub fn record_data_bytes(&self, n: u64) {
        self.file_bytes.fetch_add(n, Ordering::Relaxed);
        self.emit_progress();
    }

    /// 设置进度回调。
    pub fn set_progress_callback(&self, cb: ProgressCallback) {
        *self.progress_cb.lock().unwrap_or_else(|e| e.into_inner()) = Some(cb);
    }

    /// 触发进度回调（含 EMA 平滑速度计算 + 节流）
    fn emit_progress(&self) {
        let cb_guard = self.progress_cb.lock().unwrap_or_else(|e| e.into_inner());
        if cb_guard.is_none() {
            return;
        }
        let file_bytes = self.file_bytes.load(Ordering::Relaxed);
        let blocks = self.blocks_transferred.load(Ordering::Relaxed);
        let total_size = self.total_size.load(Ordering::Relaxed);

        // EMA 速度计算（始终执行，保证速度统计准确）
        let mut speed = self.speed_state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let dt = (now - speed.last_time).as_secs_f64();
        let bps = if dt > 0.05 && file_bytes > speed.last_bytes {
            let instant = (file_bytes - speed.last_bytes) as f64 / dt;
            // EMA: 70% 瞬时 + 30% 历史，快速响应且平滑
            speed.ema = 0.7 * instant + 0.3 * speed.ema;
            speed.last_bytes = file_bytes;
            speed.last_time = now;
            speed.ema as u64
        } else if speed.ema > 0.0 {
            speed.ema as u64
        } else {
            0
        };

        // ── 节流：100ms 间隔 + 64 KB 增量，满足其一即发射 ──
        let is_first_data = speed.file_bytes_at_last_emit == 0 && file_bytes > 0;
        let since_emit = now.duration_since(speed.last_emit_time);
        let bytes_since_emit = file_bytes.saturating_sub(speed.file_bytes_at_last_emit);

        let should_emit = is_first_data
            || since_emit.as_millis() >= THROTTLE_MIN_INTERVAL_MS as u128
            || bytes_since_emit >= THROTTLE_MIN_BYTES;

        if !should_emit {
            return;
        }

        speed.last_emit_time = now;
        speed.file_bytes_at_last_emit = file_bytes;
        drop(speed);

        if let Some(ref cb) = *cb_guard {
            cb(TransferProgress {
                bytes_transferred: file_bytes,
                total_bytes: total_size,
                blocks_transferred: blocks,
                bytes_per_second: bps,
            });
        }
    }
}

impl Socket for CountingSocket {
    /// 重写默认 `recv()`，使用协商后的 `blksize` 作为接收缓冲区大小。
    ///
    /// trait 默认实现使用 `MAX_REQUEST_PACKET_SIZE = 512`，在 blksize > 512
    /// 时会导致 Windows `WSAEMSGSIZE` (10040) 错误。
    fn recv(&self) -> Result<Packet, Box<dyn Error>> {
        self.recv_with_size(self.blksize)
    }

    /// 向已连接的远端发送数据包。
    ///
    /// 在 Windows 上，已 connected 的 UDP socket 使用 `send`（而非 `send_to`）
    /// 可避免大数据包投递失败（`send_to` 到已连接地址在部分 Windows 版本上有已知问题）。
    fn send(&self, packet: &Packet) -> Result<(), Box<dyn Error>> {
        let data = packet.serialize()?;
        let n = self.inner.send(&data)?;
        self.bytes_sent.fetch_add(n as u64, Ordering::Relaxed);
        if matches!(packet, Packet::Data { .. }) {
            self.blocks_transferred.fetch_add(1, Ordering::Relaxed);
        }
        self.emit_progress();
        Ok(())
    }

    fn send_to(&self, packet: &Packet, to: &SocketAddr) -> Result<(), Box<dyn Error>> {
        let data = packet.serialize()?;
        // 当目标地址与连接地址一致时，优先使用 send() 以避免
        // Windows connected UDP socket 使用 sendto 时的投递问题
        let n = if *to == self.remote {
            self.inner.send(&data)?
        } else {
            self.inner.send_to(&data, to)?
        };
        self.bytes_sent.fetch_add(n as u64, Ordering::Relaxed);
        if matches!(packet, Packet::Data { .. }) {
            self.blocks_transferred.fetch_add(1, Ordering::Relaxed);
        }
        self.emit_progress();
        Ok(())
    }

    fn recv_with_size(&self, size: usize) -> Result<Packet, Box<dyn Error>> {
        let mut buf_guard = self.recv_buf.lock().unwrap();
        buf_guard.resize(size + 4, 0);
        let buf = &mut buf_guard[..];
        let amt = self.inner.recv(buf)?;
        self.bytes_received.fetch_add(amt as u64, Ordering::Relaxed);
        let packet = Packet::deserialize(&buf[..amt])?;
        self.emit_progress();
        Ok(packet)
    }

    fn recv_from_with_size(&self, size: usize) -> Result<(Packet, SocketAddr), Box<dyn Error>> {
        let mut buf_guard = self.recv_buf.lock().unwrap();
        buf_guard.resize(size + 4, 0);
        let buf = &mut buf_guard[..];
        let (amt, addr) = self.inner.recv_from(buf)?;
        self.bytes_received.fetch_add(amt as u64, Ordering::Relaxed);
        let packet = Packet::deserialize(&buf[..amt])?;
        self.emit_progress();
        Ok((packet, addr))
    }

    fn remote_addr(&self) -> Result<SocketAddr, Box<dyn Error>> {
        Ok(self.remote)
    }

    fn set_read_timeout(&mut self, dur: Duration) -> Result<(), Box<dyn Error>> {
        self.inner.set_read_timeout(Some(dur))?;
        Ok(())
    }

    fn set_write_timeout(&mut self, dur: Duration) -> Result<(), Box<dyn Error>> {
        self.inner.set_write_timeout(Some(dur))?;
        Ok(())
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<(), Box<dyn Error>> {
        self.inner.set_nonblocking(nonblocking)?;
        Ok(())
    }
}

impl std::fmt::Debug for CountingSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CountingSocket")
            .field("remote", &self.remote)
            .finish()
    }
}
