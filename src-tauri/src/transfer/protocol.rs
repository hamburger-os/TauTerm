//! 文件传输协议 trait
//!
//! 定义统一的文件传输协议接口，支持 YModem 及未来扩展（ZModem、Kermit）。

/// 传输进度信息
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TransferProgress {
    /// 当前文件名
    pub file_name: String,
    /// 已传输字节数
    pub bytes_transferred: u64,
    /// 文件总字节数
    pub total_bytes: u64,
    /// 方向：发送或接收
    pub direction: TransferDirection,
}

/// 传输方向
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TransferDirection {
    Send,
    Receive,
}

/// 传输状态
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TransferStatus {
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

/// 文件传输协议 trait（供未来扩展 ZModem/Kermit）
#[allow(dead_code)]
pub trait FileTransferProtocol {
    /// 发送文件
    fn send_files(
        &mut self,
        file_paths: &[String],
        progress_callback: Box<dyn Fn(TransferProgress) + Send>,
        cancel_signal: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// 接收文件
    fn receive_files(
        &mut self,
        download_dir: &str,
        progress_callback: Box<dyn Fn(TransferProgress) + Send>,
        cancel_signal: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}
