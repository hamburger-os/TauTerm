//! iperf2 协议共享类型

use super::super::IperfProtocol;

/// 测试方向模式（-d/-r 语义对齐 2.2.1 Settings.cpp/Listener.cpp）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TestDirection {
    /// 普通单向测试（客户端 → 服务端）
    #[default]
    Normal,
    /// -r tradeoff：同一连接顺序反向（客户端发完 shutdown(WR)，服务端
    /// 读 EOF 后在同一 socket 回发）
    TradeOff,
    /// -d dualtest：客户端起本地监听（头部 RUN_NOW），服务端反向 connect
    /// 回客户端端口，两条连接同时双向测吞吐
    DualTest,
}

impl TestDirection {
    /// 是否为双向测试（-d/-r）
    pub fn is_bidirectional(&self) -> bool {
        !matches!(self, TestDirection::Normal)
    }

    /// 客户端在方向模式下的 flags 附加位（VERSION1 恒设；DualTest 另加 RUN_NOW）
    pub fn header_flags(&self) -> u32 {
        match self {
            TestDirection::Normal => 0,
            TestDirection::TradeOff => super::test_hdr::HEADER_VERSION1,
            TestDirection::DualTest => {
                super::test_hdr::HEADER_VERSION1 | super::test_hdr::HEADER_RUN_NOW
            }
        }
    }
}

/// 服务端依据客户端测试头判定的接待模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTestMode {
    /// 普通单向接收
    Normal,
    /// -r：接收完后同一 socket 回发
    TradeOff,
    /// -d：反向 connect 客户端监听端口回发（RUN_NOW）
    DualTest,
}

/// 测速参数（由 `IperfDynamicParams` 转换而来，供 iperf2 引擎使用）
#[derive(Debug, Clone)]
pub struct Iperf2TestParams {
    /// 传输模式（TCP / UDP）
    pub mode: TestMode,
    /// 测试时长秒数（-t）
    pub duration_secs: u32,
    /// 并行流数（-P）
    pub parallel_streams: u32,
    /// 目标带宽 bps（-b；UDP 限速发送速率，TCP 可选限速）
    pub bandwidth_bps: Option<u64>,
    /// TCP/UDP socket 缓冲大小（-w，字节；connect/listen/bind 前设置）
    pub window_size: Option<u32>,
    /// 报告间隔秒数（-i）
    pub report_interval_secs: u32,
    /// 目标端口（-p）
    pub port: u16,
    /// 方向模式（Normal / -r TradeOff / -d DualTest）
    pub direction: TestDirection,
}

/// 传输模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    Tcp,
    Udp,
}

impl TestMode {
    pub fn is_udp(&self) -> bool {
        matches!(self, TestMode::Udp)
    }

    pub fn to_iperf_protocol(self) -> IperfProtocol {
        match self {
            TestMode::Tcp => IperfProtocol::Tcp,
            TestMode::Udp => IperfProtocol::Udp,
        }
    }
}

/// 单区间报告
#[derive(Debug, Clone)]
pub struct Iperf2Interval {
    pub start_secs: f64,
    pub end_secs: f64,
    /// 本方向传输字节（发送方向 = 发送字节，接收方向 = 接收字节）
    pub transferred_bytes: u64,
    pub bandwidth_bps: f64,
    /// UDP 抖动（ms）
    pub jitter_ms: Option<f64>,
    pub lost_packets: Option<u64>,
    pub total_packets: Option<u64>,
    pub lost_percent: Option<f64>,
}
