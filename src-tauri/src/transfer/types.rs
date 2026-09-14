//! 串口文件传输协议算法的共享数据结构。
//!
//! 方向直接复用统一文件传输领域模型；本模块只保留 X/Y/ZModem 同步算法所需的
//! 逐块回调、文件级事件、批次结果和本地文件元数据，避免维护第二套方向定义。

use serde::Serialize;

pub use crate::kernel::file_transfer::TransferDirection;

/// 串口协议算法的逐块进度回调。
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub file_name: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub file_index: u32,
    pub total_files: u32,
    pub aggregate_bytes_transferred: u64,
    pub aggregate_total_bytes: u64,
    pub direction: TransferDirection,
}

/// 串口协议算法的文件级事件（非逐块进度）。
#[derive(Debug, Clone)]
pub enum FileTransferEvent {
    FileStart {
        file_name: String,
        file_index: u32,
        total_files: u32,
        file_size: u64,
    },
    FileComplete {
        file_name: String,
        file_index: u32,
        total_files: u32,
        bytes_transferred: u64,
        success: bool,
        error: Option<String>,
    },
}

/// 单文件在批次中的终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchFileStatus {
    Completed,
    Failed,
    Skipped,
}

impl BatchFileStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl From<&str> for BatchFileStatus {
    fn from(value: &str) -> Self {
        match value {
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            other => panic!("invalid batch file status: {other}"),
        }
    }
}

impl From<String> for BatchFileStatus {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl PartialEq<&str> for BatchFileStatus {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::fmt::Display for BatchFileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 批次传输结果。
#[derive(Debug, Clone, Serialize)]
pub struct BatchFileResult {
    pub file_name: String,
    pub status: BatchFileStatus,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 文件信息（用于 SerialTransferProtocol trait 的发送接口）。
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub mtime: u64,
}

impl FileInfo {
    /// 从文件路径构造 FileInfo，自动读取元数据。
    pub fn from_path(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err("不跟随本地符号链接，请选择实际文件或目录".into());
        }
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
            size: if meta.is_dir() { 0 } else { meta.len() },
            is_dir: meta.is_dir(),
            mtime,
        })
    }
}
