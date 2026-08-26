//! UDP 抖动 / 丢包统计累加器
//!
//! 抖动采用 RFC 3550 6.4.1 的 inter-arrival jitter 递推公式：
//!   D(i-1,i) = (R_i - S_i) - (R_(i-1) - S_(i-1))
//!   J = J + (|D| - J) / 16
//! 其中 S = 发送方时间戳（数据包头 secs/usecs 换算毫秒），R = 接收时刻。
//! 收发两端无需时钟同步（与标准 iperf2 一致）。

/// 区间累加器
///
/// 抖动/丢包统计（RFC 3550 递推 + 序号间隙）；区间边界时 `reset()` 归零。
/// 带宽区间数据由调用方用 `SharedByteCounter` 差值计算（见 data_udp.rs），
/// 本累加器不再承担快照职责。
pub struct IntervalAccumulator {
    /// 收到包数（UDP）
    packets: u64,
    /// 丢失包数（UDP，按序号间隙累计）
    lost: u64,
    /// 最高已见序号（UDP）
    highest_id: i64,
    /// 当前抖动值（ms）
    jitter_ms: f64,
    /// 上一包传输时间差 D 计算缓存：上一包的 (接收时刻 - 发送时间戳)（ms）
    last_transit_ms: Option<f64>,
}

impl Default for IntervalAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl IntervalAccumulator {
    pub fn new() -> Self {
        Self {
            packets: 0,
            lost: 0,
            highest_id: -1,
            jitter_ms: 0.0,
            last_transit_ms: None,
        }
    }

    /// 记录 UDP 数据包（发送侧：仅计数；接收侧：完整抖动/丢包计算）
    ///
    /// - `sender`: 是否为发送侧（发送侧不计算抖动/丢包）
    /// - `packet_id`: 包序号
    /// - `send_ms`: 发送方时间戳（毫秒，由包头 secs/usecs 换算）
    pub fn record_udp(&mut self, sender: bool, packet_id: i64, send_ms: f64) {
        self.packets += 1;
        if sender {
            return;
        }
        // 丢包：序号间隙（首个包不计数）
        if self.highest_id >= 0 && packet_id > self.highest_id {
            let gap = packet_id - self.highest_id - 1;
            if gap > 0 {
                self.lost += gap as u64;
            }
        }
        if packet_id > self.highest_id {
            self.highest_id = packet_id;
        }
        // 抖动（RFC 3550）
        let transit = now_ms() - send_ms;
        if let Some(last) = self.last_transit_ms {
            let d = (transit - last).abs();
            self.jitter_ms += (d - self.jitter_ms) / 16.0;
        }
        self.last_transit_ms = Some(transit);
    }

    /// 清空并重新计时（区间边界）
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 已接收包数（UDP idle 判断用）
    pub fn packets(&self) -> u64 {
        self.packets
    }

    /// 累计丢失包数
    pub fn lost(&self) -> u64 {
        self.lost
    }

    /// 当前抖动估计（ms）
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_ms
    }
}

/// 共享字节计数器
///
/// 发送/接收线程原子累加，主线程在 -i 区间边界快照差值。
/// 避免高频加锁累加器的开销。
#[derive(Debug, Default)]
pub struct SharedByteCounter {
    bytes: std::sync::atomic::AtomicU64,
}

impl SharedByteCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, n: u64) {
        self.bytes
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.bytes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 清零（服务端会话在下一轮测试开始时复用）
    pub fn reset(&self) {
        self.bytes.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 当前时间戳（毫秒）
pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}
