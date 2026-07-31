//! TFTP 协议插件
//!
//! 基于 `tftpd` crate 提供完整的 TFTP 客户端 + 服务端支持。
//! 采用 SideChannel 模式（对齐 SSH SFTP），UDP socket 在侧通道中管理，
//! 不占用终端 I/O 循环。
//!
//! 一个 TFTP Session 同时承担客户端和服务端角色：
//! - 客户端：用户主动 GET/PUT 文件到远程 TFTP 服务器
//! - 服务端：监听端口响应外部设备的 RRQ/WRQ 请求

pub mod counting_socket;
pub mod transfer;
pub mod server;
pub mod client;

use std::any::Any;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::channel::error::SessionError;
use crate::channel::{ContentType, IoStrategy};
use crate::kernel::plugin_adapter::{
    ProtocolAdapter, ProtocolConnection, SideChannel, TransferProtocolType,
};

// ── 配置类型 ─────────────────────────────────────────────────

/// TFTP Session 配置（ConnectDialog 创建时设定，不可变）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpConfig {
    /// 服务端绑定 IP 地址
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,
    /// 服务端绑定端口（默认 69）
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    /// 文件根目录（必须为绝对路径）
    pub file_root: String,
    /// 允许远程 PUT（WRQ）
    #[serde(default = "default_true")]
    pub write_enabled: bool,
    /// 允许覆盖已存在文件
    #[serde(default = "default_true")]
    pub overwrite: bool,
    /// 单端口模式
    #[serde(default)]
    pub single_port: bool,
}

fn default_listen_ip() -> String {
    "0.0.0.0".into()
}
fn default_listen_port() -> u16 {
    69
}
fn default_true() -> bool {
    true
}

/// TFTP 动态参数（Session 内实时可调）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpDynamicParams {
    /// 传输块大小（512–65464，默认 512）
    #[serde(default = "default_blksize")]
    pub blksize: u16,
    /// 每块重传超时秒数（1–255，默认 5）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u8,
    /// 滑动窗口块数（1–65535，默认 1 = 停等方式）
    #[serde(default = "default_windowsize")]
    pub windowsize: u16,
    /// 每块最大重传次数（默认 6）
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
    /// Block 号回绕策略
    #[serde(default = "default_rollover")]
    pub rollover: TftpRollover,
    /// 窗口间发包延迟（ms，0 = 不延迟）
    #[serde(default)]
    pub window_wait: u64,
    /// 传输失败时删除不完整文件
    #[serde(default = "default_true")]
    pub clean_on_error: bool,
    /// 每个包重复发送次数（1–4，默认 1 = 不重复，用于不可靠网络）
    #[serde(default = "default_repeat_count")]
    pub repeat_count: u8,
}

fn default_blksize() -> u16 {
    512
}
fn default_timeout_secs() -> u8 {
    5
}
fn default_windowsize() -> u16 {
    1
}
fn default_max_retries() -> usize {
    6
}
fn default_rollover() -> TftpRollover {
    TftpRollover::None
}
fn default_repeat_count() -> u8 {
    1
}

impl Default for TftpDynamicParams {
    fn default() -> Self {
        Self {
            blksize: default_blksize(),
            timeout_secs: default_timeout_secs(),
            windowsize: default_windowsize(),
            max_retries: default_max_retries(),
            rollover: default_rollover(),
            window_wait: 0,
            clean_on_error: true,
            repeat_count: default_repeat_count(),
        }
    }
}

/// Block 号回绕策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TftpRollover {
    /// 不允许回绕
    None,
    /// 强制回绕到 0（默认）
    Enforce0,
    /// 强制回绕到 1
    Enforce1,
    /// 不关心
    DontCare,
}


// ── 传输状态（保留供未来 transfer 注册表使用）────────────────

/// 传输方向
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    /// 发送（本地→远程）
    Send,
    /// 接收（远程→本地）
    Receive,
}

/// 传输角色
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferRole {
    /// 客户端发起
    Client,
    /// 服务端响应
    Server,
}

/// 传输状态
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    /// 等待中
    Pending,
    /// 传输中
    Transferring,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 活跃传输记录（保留供未来 transfer 注册表使用）
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTransfer {
    pub id: u64,
    pub direction: TransferDirection,
    pub role: TransferRole,
    pub remote_addr: String,
    pub filename: String,
    pub local_path: Option<String>,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub blocks_transferred: u64,
    pub status: TransferStatus,
    pub error: Option<String>,
    pub started_at_ms: u64,
}

// ── 服务端请求 ───────────────────────────────────────────────

/// 待审批的服务端请求（保留供未来审批流程使用）
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub id: String,
    pub remote_addr: String,
    pub filename: String,
    /// 请求类型：read（RRQ，GET）或 write（WRQ，PUT）
    pub is_write: bool,
    pub file_size: Option<u64>,
}

// ── 状态查询 ─────────────────────────────────────────────────

/// TFTP 状态查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftpStatus {
    pub server_running: bool,
    pub listen_addr: Option<String>,
    pub listen_port: Option<u16>,
    pub file_root: String,
    pub dynamic_params: TftpDynamicParams,
}

// ── TftpSideChannel ──────────────────────────────────────────

/// TFTP 侧通道资源
///
/// 持有共享 UDP socket 和所有传输状态。
/// 通过 `ProtocolConnection::side_channel` 传递给 `SessionStore`。
pub struct TftpSideChannel {
    /// 共享监听 UDP socket（使用 std::net::UdpSocket 因为 tftpd::Worker 需要同步 I/O）
    pub socket: Arc<std::net::UdpSocket>,
    /// Session 配置（不可变）
    pub config: TftpConfig,
    /// 动态参数（可实时修改）
    pub dynamic_params: Arc<Mutex<TftpDynamicParams>>,
    /// 服务端运行状态（由 server 线程在启动/退出时设置）
    pub server_running: Arc<AtomicBool>,
    /// 取消标志
    pub abort_flag: Arc<AtomicBool>,
    /// 传输 ID 计数器（session 内单调递增，从 1 开始，客户端和服务端共用）
    pub next_transfer_id: Arc<AtomicU64>,
    /// 服务端活跃传输计数（用于并发限制）
    pub active_server_transfers: Arc<AtomicU64>,
}

impl TftpSideChannel {
    /// 创建新的 TFTP 侧通道。
    pub fn new(
        socket: Arc<std::net::UdpSocket>,
        config: TftpConfig,
    ) -> Self {
        Self {
            socket,
            config,
            dynamic_params: Arc::new(Mutex::new(TftpDynamicParams::default())),
            server_running: Arc::new(AtomicBool::new(false)),
            abort_flag: Arc::new(AtomicBool::new(false)),
            next_transfer_id: Arc::new(AtomicU64::new(1)),
            active_server_transfers: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 获取动态参数快照。
    pub fn get_params(&self) -> TftpDynamicParams {
        self.dynamic_params.lock().unwrap().clone()
    }
}

impl SideChannel for TftpSideChannel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn shutdown(&self) {
        // 置位取消标志。Server 线程在下个循环迭代检测到后退出。
        // 无需阻塞等待——server 线程持有的 Arc<UdpSocket> clone 在线程退出前
        // 保持 socket 存活，不会发生 "socket 先释放、线程后访问" 的 use-after-free。
        self.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        log::info!("[TFTP] 已请求服务端停止");
    }
}

// ── TftpAdapter ──────────────────────────────────────────────

/// TFTP 协议适配器
///
/// 无状态结构体——每次 `connect()` 绑定一个新的 UDP socket 并创建侧通道。
/// 通过 `connect()` 返回 `ProtocolConnection`，携带：
/// - `channel`: `None`（无终端 I/O — 会话使用容器模式，不创建 I/O loop）
/// - `side_channel`: `TftpSideChannel`（UDP socket + 传输管理）
pub struct TftpAdapter;

impl TftpAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for TftpAdapter {
    async fn connect(
        &self,
        _endpoint: &str,
        params: &serde_json::Value,
    ) -> Result<ProtocolConnection, SessionError> {
        // 解析配置
        let config: TftpConfig = serde_json::from_value(params.clone())
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("TFTP 配置解析失败: {}", e),
            })?;

        // 验证文件根目录
        let root = PathBuf::from(&config.file_root);
        if !root.is_absolute() {
            return Err(SessionError::ConnectionFailed {
                reason: "TFTP 文件根目录必须是绝对路径".into(),
            });
        }

        // 绑定 UDP socket
        let listen_addr: SocketAddr = format!("{}:{}", config.listen_ip, config.listen_port)
            .parse()
            .map_err(|e| SessionError::ConnectionFailed {
                reason: format!("TFTP 监听地址无效: {}", e),
            })?;

        let socket = std::net::UdpSocket::bind(listen_addr)
            .map_err(|e| SessionError::IoError(std::io::Error::new(
                e.kind(),
                format!("无法绑定 TFTP 端口 {}: {}", listen_addr, e),
            )))?;

        log::info!("TFTP socket 已绑定到 {}", listen_addr);

        let side_channel = Arc::new(TftpSideChannel::new(
            Arc::new(socket),
            config,
        ));

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

// ── 共享辅助函数 ────────────────────────────────────────────

/// 尝试从会话的 side_channel 启动 TFTP 服务端。
///
/// 供 `connect_session_tftp` 和 `tftp_server_start` 共用，
/// 消除命令层对 `TftpSideChannel` 内部字段的直接操作。
///
/// 若 side_channel 不存在、非 TFTP 类型、或已在运行，返回 `Err`。
pub fn try_start_server(
    app: &tauri::AppHandle,
    side_channel: &Arc<dyn crate::kernel::plugin_adapter::SideChannel>,
    session_id: &str,
) -> Result<(), String> {
    let tftp_sc = side_channel
        .as_any()
        .downcast_ref::<TftpSideChannel>()
        .ok_or_else(|| "侧通道不是 TFTP 类型".to_string())?;

    if tftp_sc.server_running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("TFTP 服务端已在运行".into());
    }

    tftp_sc.abort_flag.store(false, std::sync::atomic::Ordering::Relaxed);
    server::spawn_tftp_server(
        app.clone(),
        tftp_sc.socket.clone(),
        tftp_sc.config.clone(),
        tftp_sc.dynamic_params.clone(),
        tftp_sc.abort_flag.clone(),
        tftp_sc.server_running.clone(),
        tftp_sc.next_transfer_id.clone(),
        tftp_sc.active_server_transfers.clone(),
        session_id.to_string(),
    );

    log::info!("[TFTP] 服务端已启动 (session={})", session_id);
    Ok(())
}

