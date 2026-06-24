//! 文件传输公共类型定义
//!
//! 所有协议（XModem/YModem/ZModem）共享的数据结构。

use serde::Serialize;

/// 传输方向
#[derive(Debug, Clone, PartialEq)]
pub enum TransferDirection {
    Send,
    Receive,
}

/// 传输进度
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub file_name: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub file_index: u32,
    pub total_files: u32,
    pub aggregate_bytes_transferred: u64,
    pub aggregate_total_bytes: u64,
    /// 传输方向（字段不直接在 Rust 侧读取，但通过 JSON 序列化发送到前端
    /// transfer-progress 事件，前端据此显示方向指示器）
    #[allow(dead_code)]
    pub direction: TransferDirection,
}

/// 文件级别事件（非逐块进度）
#[derive(Debug, Clone)]
pub enum FileTransferEvent {
    /// 文件开始传输
    FileStart {
        file_name: String,
        file_index: u32,
        total_files: u32,
        file_size: u64,
    },
    /// 文件传输完成（成功或失败）
    FileComplete {
        file_name: String,
        file_index: u32,
        total_files: u32,
        bytes_transferred: u64,
        success: bool,
        error: Option<String>,
    },
}

/// 批次传输结果，用于 transfer-complete 事件
#[derive(Debug, Clone, Serialize)]
pub struct BatchFileResult {
    pub file_name: String,
    pub status: String, // "completed" | "failed" | "skipped"
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 文件信息（用于 TransferProtocol trait 的发送接口）
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// 文件系统路径
    pub path: String,
    /// 文件名（不含路径）
    pub name: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 修改时间（Unix 时间戳）
    pub mtime: u64,
}

impl FileInfo {
    /// 从文件路径构造 FileInfo，自动读取元数据
    pub fn from_path(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let meta = std::fs::metadata(path)?;
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mtime = meta
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0);
        Ok(FileInfo {
            path: path.to_string(),
            name,
            size: meta.len(),
            mtime,
        })
    }
}
