//! iperf 网络测速插件
//!
//! 同时支持 iperf2（自研协议实现）与 iperf3（riperf3 crate，wire-compatible）。
//! 采用 SideChannel 模式（对齐 TFTP），会话为容器模式（无终端 I/O 循环）。
//!
//! 一个 iperf Session 同时承担客户端和服务端角色：
//! - 客户端：用户配置目标主机，发起瞬时测速任务（配置 → 运行 → 出结果 → 结束）
//! - 服务端：监听端口，等待外部 iperf 客户端（如板子）连接测速
//!
//! 注意：iperf2 与 iperf3 协议互不互通，两端的版本必须一致。

pub mod server;
pub mod client;

mod iperf2;
mod iperf3;

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::channel::error::SessionError;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    ProtocolAdapter, ProtocolConnection, SideChannel, TransferProtocolType,
};


// ── 基础枚举 ─────────────────────────────────────────────

/// iperf 协议版本（两协议互不互通，对端必须同版本）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IperfVersion {
    /// iperf 2.x（自研协议实现）
    #[default]
    Iperf2,
    /// iperf 3.x（riperf3 crate）
    Iperf3,
}

/// 测速角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IperfRole {
    /// 客户端发起测速（瞬态任务）
    Client,
    /// 服务端常驻监听
    Server,
}

/// 测试方向（-d/-r 双向测试下区分 fwd 发送侧 / rev 接收侧；
/// 事件携带该字段供前端按方向归位记录）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IperfDirection {
    /// 正向（客户端 → 服务端）
    #[default]
    Fwd,
    /// 反向（服务端 → 客户端）
    Rev,
}

/// 传输协议
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IperfProtocol {
    #[default]
    Tcp,
    Udp,
}

// ── 配置类型 ─────────────────────────────────────────────

/// iperf Session 配置（ConnectDialog 创建时设定，不可变）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IperfConfig {
    /// 协议版本（iperf2 / iperf3）
    #[serde(default = "default_version")]
    pub version: IperfVersion,
    /// 服务端绑定 IP 地址
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,
    /// 服务端绑定端口（iperf2 默认 5001，iperf3 默认 5201）
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
}

fn default_version() -> IperfVersion {
    IperfVersion::Iperf2
}
/// 版本默认客户端目标端口（iperf2 5001 / iperf3 5201，
/// 与 ConnectDialog 版本切换的端口联动规则一致）
pub fn default_client_port(version: IperfVersion) -> u16 {
    match version {
        IperfVersion::Iperf2 => 5001,
        IperfVersion::Iperf3 => 5201,
    }
}
fn default_listen_ip() -> String {
    "0.0.0.0".into()
}
fn default_listen_port() -> u16 {
    5001
}

/// iperf 动态参数（Session 内实时可调）
///
/// 测速参数本质由客户端发起方决定；服务端仅使用监听地址与端口。
/// 前端通过 `iperf_update_params` 同步此处。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IperfDynamicParams {
    /// 协议版本（会话创建时同步自 config，运行时可切换；引擎按此路由）
    #[serde(default)]
    pub version: IperfVersion,
    /// 服务端监听 IP（会话内可调；服务端运行时改动需重启生效）
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,
    /// 服务端监听端口（会话内可调；服务端运行时改动需重启生效）
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    /// 传输协议（TCP / UDP）
    #[serde(default)]
    pub protocol: IperfProtocol,
    /// 测试时长秒数（-t，默认 10）
    #[serde(default = "default_duration_secs")]
    pub duration_secs: u32,
    /// 客户端目标端口（-p，默认 5001）
    #[serde(default = "default_port")]
    pub port: u16,
    /// 并行流数（-P，默认 1）
    #[serde(default = "default_parallel")]
    pub parallel_streams: u32,
    /// 报告间隔秒数（-i，默认 1）
    #[serde(default = "default_report_interval")]
    pub report_interval_secs: u32,
    /// 目标带宽 bps（-b；iperf2 仅 UDP 生效，iperf3 TCP/UDP 均可）
    #[serde(default)]
    pub bandwidth_bps: Option<u64>,
    // ── iperf2 特有（语义与 iperf3 不同，UI 需区分） ──
    /// 双向同时测试（-d dualtest）
    #[serde(default)]
    pub bidirectional: bool,
    /// 顺序双向测试（-r tradeoff：客户端先发送再接收）
    #[serde(default)]
    pub tradeoff: bool,
    /// TCP 窗口大小字节（-w）
    #[serde(default)]
    pub window_size: Option<u32>,
    // ── iperf3 特有 ──
    /// 反向测试（-R：服务端发送，客户端接收）
    #[serde(default)]
    pub reverse: bool,
    /// 双向同时测试（--bidir）
    #[serde(default)]
    pub bidir: bool,
    /// 预热排除前 N 秒（-O，不计入统计）
    #[serde(default)]
    pub omit_secs: u32,
}

fn default_duration_secs() -> u32 {
    10
}
fn default_port() -> u16 {
    5001
}
fn default_parallel() -> u32 {
    1
}
fn default_report_interval() -> u32 {
    1
}

impl Default for IperfDynamicParams {
    fn default() -> Self {
        Self {
            version: IperfVersion::Iperf2,
            listen_ip: default_listen_ip(),
            listen_port: default_listen_port(),
            protocol: IperfProtocol::Tcp,
            duration_secs: default_duration_secs(),
            port: default_port(),
            parallel_streams: default_parallel(),
            report_interval_secs: default_report_interval(),
            bandwidth_bps: None,
            bidirectional: false,
            tradeoff: false,
            window_size: None,
            reverse: false,
            bidir: false,
            omit_secs: 0,
        }
    }
}

// ── 测试报告类型 ─────────────────────────────────────────

/// 单区间报告（-i 间隔输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IperfIntervalReport {
    pub start_secs: f64,
    pub end_secs: f64,
    pub transferred_bytes: u64,
    pub bandwidth_bps: f64,
    /// UDP 抖动（ms）
    pub jitter_ms: Option<f64>,
    pub lost_packets: Option<u64>,
    pub total_packets: Option<u64>,
    pub lost_percent: Option<f64>,
}

/// 测试汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IperfSummary {
    pub version: IperfVersion,
    pub role: IperfRole,
    pub protocol: IperfProtocol,
    pub duration_secs: f64,
    pub total_bytes: u64,
    pub avg_bandwidth_bps: f64,
    pub intervals: Vec<IperfIntervalReport>,
    /// UDP 抖动（ms）
    pub jitter_ms: Option<f64>,
    pub lost_packets: Option<u64>,
    pub total_packets: Option<u64>,
    pub lost_percent: Option<f64>,
}

/// 客户端一次测速的结果（fwd 必在；-d/-r 附带 rev；UDP 回报缺失时带警告）
#[derive(Debug, Clone)]
pub struct IperfClientResult {
    pub fwd: IperfSummary,
    pub rev: Option<IperfSummary>,
    /// 非致命警告（如 UDP 未收到服务器统计回报），由 done 事件带给前端展示
    pub warning: Option<String>,
}

// ── 状态查询 ─────────────────────────────────────────────

/// iperf 状态查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IperfStatus {
    pub server_running: bool,
    /// 服务端接待测试进行中
    pub test_running: bool,
    /// 客户端测速任务进行中（独立于服务端，避免相互覆盖）
    #[serde(default)]
    pub client_test_running: bool,
    pub listen_addr: Option<String>,
    pub listen_port: Option<u16>,
    pub version: IperfVersion,
    pub dynamic_params: IperfDynamicParams,
    pub last_summary: Option<IperfSummary>,
}

// ── IperfSideChannel ─────────────────────────────────────

/// iperf 侧通道资源
///
/// 持有服务端监听线程、动态参数与最近一次测试汇总。
/// 通过 `ProtocolConnection::side_channel` 传递给 `SessionStore`。
pub struct IperfSideChannel {
    /// Session 配置（不可变）
    pub config: IperfConfig,
    /// 动态参数（可实时修改）
    pub dynamic_params: Arc<Mutex<IperfDynamicParams>>,
    /// 服务端运行状态（由 server 线程在启动/退出时设置）
    pub server_running: Arc<AtomicBool>,
    /// 服务端接待测试进行状态
    pub test_running: Arc<AtomicBool>,
    /// 客户端测速任务进行状态（与服务端接待独立，避免相互覆盖）
    pub client_test_running: Arc<AtomicBool>,
    /// 取消标志：服务端监听线程的生命线（下个循环迭代检测后退出）。
    /// 与 `client_abort_flag` 相互独立——任一引擎自然结束/被停止不得影响另一个
    /// （标准设计：一个生命周期所有者一个取消信号）
    pub server_abort_flag: Arc<AtomicBool>,
    /// 取消标志：客户端测速任务（发送循环检测后退出）。
    /// 与 `server_abort_flag` 相互独立；会话关闭（shutdown）时两者一起置位
    pub client_abort_flag: Arc<AtomicBool>,
    /// 最近一次测试汇总（前端断线重连后可查询）
    pub last_summary: Arc<Mutex<Option<IperfSummary>>>,
    /// 服务端监听线程句柄（停止时 join）
    pub server_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    /// 服务端线程代际：try_start_server spawn 前递增；线程退出时仅当代际
    /// 未变才写回 running=false 与状态事件（防僵尸线程的迟到写回覆盖
    /// 新一代服务器的状态）
    pub server_epoch: Arc<AtomicU64>,
    /// 服务端启停互斥锁：try_start_server 与 iperf_server_stop 串行化，
    /// 停止请求不会落在 start 的 join/复位窗口内被静默覆盖
    pub lifecycle: tokio::sync::Mutex<()>,
}

impl IperfSideChannel {
    /// 创建新的 iperf 侧通道
    pub fn new(config: IperfConfig) -> Self {
        // 动态参数初始化时同步 config 的版本、监听地址与端口
        let params = IperfDynamicParams {
            version: config.version,
            listen_ip: config.listen_ip.clone(),
            listen_port: config.listen_port,
            port: config.listen_port,
            ..IperfDynamicParams::default()
        };
        Self {
            config,
            dynamic_params: Arc::new(Mutex::new(params)),
            server_running: Arc::new(AtomicBool::new(false)),
            test_running: Arc::new(AtomicBool::new(false)),
            client_test_running: Arc::new(AtomicBool::new(false)),
            server_abort_flag: Arc::new(AtomicBool::new(false)),
            client_abort_flag: Arc::new(AtomicBool::new(false)),
            last_summary: Arc::new(Mutex::new(None)),
            server_handle: Arc::new(Mutex::new(None)),
            server_epoch: Arc::new(AtomicU64::new(0)),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }

    /// 获取动态参数快照（锁中毒经统一恢复出口）
    pub fn get_params(&self) -> IperfDynamicParams {
        lock_or_recover(&self.dynamic_params, "dynamic_params").clone()
    }
}

impl SideChannel for IperfSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        // 会话关闭：一次性取消服务端监听与客户端测速（各自线程检测后退出）。
        // 递增代际作废旧线程的退出写回（重连后旧线程迟到 emit 不得翻转
        // 新服务器的状态；断连自身的 running:false 由 disconnect_session 发出）
        self.server_epoch.fetch_add(1, Ordering::SeqCst);
        self.server_abort_flag.store(true, Ordering::SeqCst);
        self.client_abort_flag.store(true, Ordering::SeqCst);
        self.server_running.store(false, Ordering::SeqCst);
        self.test_running.store(false, Ordering::SeqCst);
        self.client_test_running.store(false, Ordering::SeqCst);
        log::info!("[iperf] 已请求服务端停止");
    }
}

// ── IperfAdapter ─────────────────────────────────────────

/// iperf 协议适配器
///
/// 无状态结构体——每次 `connect()` 创建侧通道。
/// 通过 `connect()` 返回 `ProtocolConnection`，携带：
/// - `channel`: `None`（无终端 I/O — 容器会话模式，不创建 I/O loop）
/// - `side_channel`: `IperfSideChannel`（服务端监听 + 测试状态管理）
pub struct IperfAdapter;

impl IperfAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for IperfAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        // 解析配置
        let config: IperfConfig = serde_json::from_value(params.clone())
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("iperf 配置解析失败: {}", e),
            })?;

        log::info!("iperf 配置已解析: version={:?}, listen={}:{}", config.version, config.listen_ip, config.listen_port);

        let side_channel = Arc::new(IperfSideChannel::new(config));

        Ok(ProtocolConnection {
            channel: None,
            comm_handle: None,
            side_channel: Some(side_channel),
            teardown_delay: Duration::from_millis(100),
        })
    }

    fn content_type(&self) -> ContentType {
        ContentType::Terminal // 前端通过 manifest.content_type="custom" 路由
    }

    fn io_strategy(&self) -> IoStrategy {
        IoStrategy::Async
    }

    fn transfer_protocols(&self) -> Vec<TransferProtocolType> {
        vec![]
    }

    fn teardown_delay(&self) -> Duration {
        Duration::from_millis(100)
    }
}

// ── 共享辅助函数 ─────────────────────────────────────────

/// 锁中毒恢复：日志告警后取回锁内最后值（全插件统一出口——一次 panic
/// 不应让后续命令/线程连锁崩溃；client.rs/data_udp.rs 的 `if let Ok`
/// 静默跳过语义也统一为此恢复语义，避免摘要静默丢失）
pub(crate) fn lock_or_recover<'a, T>(m: &'a Mutex<T>, what: &str) -> MutexGuard<'a, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => {
            log::warn!("[iperf] {} 锁中毒，恢复最后状态", what);
            poisoned.into_inner()
        }
    }
}

/// 有界 join 服务端线程句柄（先 take 后 join；超时放弃返回 false）。
///
/// 供 try_start_server 与 connect_session_iperf（重连路径）共用：
/// 保证旧线程完全收尾后再绑新端口；旧线程的迟到状态写回由代际
/// （server_epoch）作废，无需依赖 join 完成。
pub fn join_server_handle(
    handle: &Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    timeout: Duration,
) -> bool {
    let taken = lock_or_recover(handle, "server_handle").take();
    let Some(h) = taken else {
        return true;
    };
    let deadline = Instant::now() + timeout;
    while !h.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if h.is_finished() {
        let _ = h.join();
        true
    } else {
        log::warn!("[iperf] 服务端线程 join 超时（放弃；端口可能仍被占用）");
        false
    }
}

/// 尝试从会话的 side_channel 启动 iperf 服务端。
///
/// 供 `connect_session_iperf` 和 `iperf_server_start` 共用。
/// 若 side_channel 不存在、非 iperf 类型、或已在运行，返回 `Err`。
///
/// 与 `iperf_server_stop` 经 lifecycle 锁互斥：Stop 不会落在 join/复位窗口
/// 内被静默覆盖；join 期间新到的停止请求（入口时 abort 尚未置位）使本次
/// 启动放弃。join 轮询在 spawn_blocking 中执行，不占用 tokio worker。
pub async fn try_start_server<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    side_channel: &Arc<dyn crate::kernel::plugin_adapter::SideChannel>,
    session_id: &str,
) -> Result<(), String> {
    let iperf_sc = side_channel
        .as_any()
        .downcast_ref::<IperfSideChannel>()
        .ok_or_else(|| "侧通道不是 iperf 类型".to_string())?;

    // 与 iperf_server_stop 串行化（tokio Mutex：await 不阻塞 worker）
    let _lifecycle = iperf_sc.lifecycle.lock().await;

    // 原子占位：消灭 check-then-act 双启动竞态（并发两个 start 只有一个胜出；
    // 占位在 bind 之前——绑定失败由线程退出路径置回 false 并 emit error）
    if iperf_sc
        .server_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("iperf 服务端已在运行或正在启动".into());
    }

    // 记录入口时 abort 状态：stop 先完成（入口已置位）属正常 restart 场景；
    // 等待期间新置位（Stop 后发先至）则放弃本次启动
    let abort_at_entry = iperf_sc.server_abort_flag.load(Ordering::SeqCst);

    // 有界 join 上一次的线程句柄（停止/断开流程置位 abort 后线程自行退出）
    let handle = iperf_sc.server_handle.clone();
    let _ = tokio::task::spawn_blocking(move || {
        join_server_handle(&handle, Duration::from_secs(15))
    })
    .await
    .unwrap_or(false);

    // 等待期间收到新的停止请求：尊重之，放弃启动（running 已被 Stop 复位）
    if !abort_at_entry && iperf_sc.server_abort_flag.load(Ordering::SeqCst) {
        log::info!(
            "[iperf] 启动等待期间收到停止请求，放弃启动 (session={})",
            session_id
        );
        return Ok(());
    }

    iperf_sc.server_abort_flag.store(false, Ordering::SeqCst);
    // 递增代际：作废旧线程（join 超时仍存活者）的退出写回；新线程持有
    // 新代际，其退出写回对后续 shutdown/restart 生效
    let my_epoch = iperf_sc.server_epoch.fetch_add(1, Ordering::SeqCst) + 1;
    server::spawn_iperf_server(
        app.clone(),
        iperf_sc.config.clone(),
        iperf_sc.dynamic_params.clone(),
        iperf_sc.server_abort_flag.clone(),
        iperf_sc.server_running.clone(),
        iperf_sc.test_running.clone(),
        iperf_sc.last_summary.clone(),
        iperf_sc.server_handle.clone(),
        iperf_sc.server_epoch.clone(),
        my_epoch,
        session_id.to_string(),
    );

    log::info!("[iperf] 服务端已启动 (session={})", session_id);
    Ok(())
}
