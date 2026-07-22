//! 统一文件传输抽象层
//!
//! 定义 `FileTransfer` trait — 所有传输协议（XModem/YModem/ZModem/SFTP/FTP 等）
//! 的统一异步接口。串口同步协议通过内部 `spawn_blocking` 适配，SSH/SFTP
//! 自然 async。进度通过 `UnboundedSender<UnifiedProgress>` 统一广播，
//! 取消通过 `Arc<AtomicBool>` 统一信号。
//!
//! ## 与旧 `TransferProtocol` trait 的区别
//!
//! - 旧 trait 绑定 `Box<dyn SerialPort>`，仅支持串口协议
//! - 新 trait 协议无关 — 由具体实现持有各自的 I/O 资源
//! - 旧 trait 使用闭包回调传递进度，新 trait 使用 channel 广播
//! - 旧 trait 同步，新 trait async（统一 tokio 运行时调度）

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Send,
    Receive,
}

/// 统一进度事件
///
/// 替代旧的 `TransferProgress`（串口）和 `SftpProgressPayload`（SFTP）双轨制。
/// 前端只需监听一个 `file-transfer:progress` 事件，通过 `protocol` 字段区分协议。
#[derive(Debug, Clone, Serialize)]
pub struct UnifiedProgress {
    /// 所属会话 ID（由 spawn_progress_broadcaster 填充，用于前端跨会话过滤）
    #[serde(default)]
    pub session_id: String,
    /// 协议标识（如 "ymodem", "sftp"）
    pub protocol: String,
    /// 当前传输的文件名
    pub file_name: String,
    /// 当前文件已传输字节
    pub bytes_done: u64,
    /// 当前文件总字节（0 表示未知大小）
    pub bytes_total: u64,
    /// 当前文件在批次中的索引（0-based）
    pub file_index: usize,
    /// 批次中文件总数
    pub total_files: usize,
    /// 聚合已传输字节（已完成文件 + 当前文件进度）
    pub aggregate_bytes: u64,
    /// 聚合总字节
    pub aggregate_total: u64,
    /// 传输方向
    pub direction: TransferDirection,
    /// 是否为文件开始事件（新文件开始传输）
    pub is_file_start: bool,
    /// 是否为文件完成事件
    pub is_file_complete: bool,
    /// 文件完成时：是否成功
    pub file_success: Option<bool>,
    /// 文件完成时：错误信息
    pub file_error: Option<String>,
    /// 传输已完成（整个批次结束）
    pub is_batch_complete: bool,
}

#[allow(clippy::too_many_arguments)]
impl UnifiedProgress {
    /// 构造文件开始事件
    pub fn file_start(
        protocol: &str,
        file_name: &str,
        file_size: u64,
        file_index: usize,
        total_files: usize,
        aggregate_bytes: u64,
        aggregate_total: u64,
        direction: TransferDirection,
    ) -> Self {
        Self {
            session_id: String::new(),
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done: 0,
            bytes_total: file_size,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            is_file_start: true,
            is_file_complete: false,
            file_success: None,
            file_error: None,
            is_batch_complete: false,
        }
    }

    /// 构造逐块进度事件
    pub fn chunk(
        protocol: &str,
        file_name: &str,
        bytes_done: u64,
        bytes_total: u64,
        file_index: usize,
        total_files: usize,
        aggregate_bytes: u64,
        aggregate_total: u64,
        direction: TransferDirection,
    ) -> Self {
        Self {
            session_id: String::new(),
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done,
            bytes_total,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            is_file_start: false,
            is_file_complete: false,
            file_success: None,
            file_error: None,
            is_batch_complete: false,
        }
    }

    /// 构造文件完成事件
    pub fn file_complete(
        protocol: &str,
        file_name: &str,
        bytes_transferred: u64,
        file_index: usize,
        total_files: usize,
        aggregate_bytes: u64,
        aggregate_total: u64,
        direction: TransferDirection,
        success: bool,
        error: Option<String>,
    ) -> Self {
        Self {
            session_id: String::new(),
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done: bytes_transferred,
            bytes_total: bytes_transferred,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            is_file_start: false,
            is_file_complete: true,
            file_success: Some(success),
            file_error: error,
            is_batch_complete: false,
        }
    }

    /// 构造批次完成事件
    ///
    /// 当 `files_failed > 0` 或 `files_skipped > 0` 时设置 `file_success: Some(false)`,
    /// 前端据此判断批次是否成功 (此前 `file_error` 始终为 `None` 导致前端误判为"completed")。
    pub fn batch_complete(
        protocol: &str,
        direction: TransferDirection,
        files_completed: usize,
        files_failed: usize,
        files_skipped: usize,
    ) -> Self {
        let has_issues = files_failed > 0 || files_skipped > 0;
        Self {
            session_id: String::new(),
            protocol: protocol.to_string(),
            file_name: "__batch_complete__".to_string(),
            bytes_done: 0,
            bytes_total: 0,
            file_index: 0,
            total_files: files_completed + files_failed + files_skipped,
            aggregate_bytes: 0,
            aggregate_total: 0,
            direction,
            is_file_start: false,
            is_file_complete: false,
            file_success: Some(!has_issues),
            file_error: None,
            is_batch_complete: true,
        }
    }
}

/// 文件传输错误
#[derive(Debug, thiserror::Error)]
pub enum FileTransferError {
    #[error("传输被取消")]
    Cancelled,

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("会话错误: {0}")]
    Session(String),

    #[error("{0}")]
    Other(String),
}

/// 统一文件传输 trait
///
/// 所有传输协议（串口 X/Y/ZModem、SSH SFTP、未来 FTP/WebDAV 等）
/// 必须实现此 trait。使用 `async_trait` 统一异步签名：
/// - 同步协议（串口）内部使用 `tokio::task::spawn_blocking`
/// - 异步协议（SSH）直接 await
///
/// # 生命周期
///
/// 1. `send()` / `receive()` 被调用
/// 2. 实现者通过 `progress` channel 发送 `UnifiedProgress` 事件
/// 3. 定期检查 `cancel` 标志，若为 true 则返回 `FileTransferError::Cancelled`
/// 4. 完成后返回 `Vec<BatchFileResult>`
#[async_trait::async_trait]
pub trait FileTransfer: Send + Sync {
    /// 返回协议标识字符串（如 "ymodem", "sftp"）
    fn protocol(&self) -> &str;

    /// 返回 `&dyn Any` 以供向下转型到具体实现类型
    fn as_any(&self) -> &dyn Any;


    /// 发送文件
    ///
    /// # 参数
    /// - `files`: 待发送文件列表（`path` 是本地路径，`name` 是文件名）
    /// - `remote_dir`: 远程目标目录（串口协议传 `None`；SFTP 等侧通道协议用于构建远程路径）
    /// - `progress`: 进度事件发送通道
    /// - `cancel`: 取消标志，实现者应定期检查
    async fn send(
        &self,
        files: &[crate::transfer::types::FileInfo],
        remote_dir: Option<&str>,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<crate::transfer::types::BatchFileResult>, FileTransferError>;

    /// 接收文件
    ///
    /// # 参数
    /// - `download_dir`: 下载目标目录
    /// - `remote_paths`: 待下载的远程文件路径列表（串口协议传空 vec，由协议自行协商）
    /// - `progress`: 进度事件发送通道
    /// - `cancel`: 取消标志，实现者应定期检查
    async fn receive(
        &self,
        download_dir: &str,
        remote_paths: &[String],
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<crate::transfer::types::BatchFileResult>, FileTransferError>;
}
