//! 统一文件传输抽象层
//!
//! 定义 `FileTransfer` trait — 所有传输协议（XModem/YModem/ZModem/SFTP/FTP 等）
//! 的统一异步接口。串口同步协议通过内部 `spawn_blocking` 适配，SSH/SFTP
//! 自然 async。进度通过 `UnboundedSender<UnifiedProgress>` 统一广播，
//! 取消通过 `Arc<AtomicBool>` 统一信号。
//!
//! ## 与串口 `SerialTransferProtocol` trait 的区别
//!
//! - 串口算法 trait 依赖协议无关的 `TransferIo = Read + Write + Send`，不绑定具体 serialport handle
//! - 本 trait 协议无关 — 由具体实现持有各自的 I/O 资源
//! - 串口 trait 使用闭包回调传递进度，本 trait 使用 channel 广播
//! - 串口 trait 同步，本 trait async（统一 tokio 运行时调度）

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Send,
    Receive,
}

/// 进度流中的事件类型。
///
/// 使用显式枚举表达文件开始、数据进度、文件完成和批次完成，
/// 从数据模型上排除互相矛盾的阶段组合，也不再依赖特殊文件名表达控制事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferProgressKind {
    FileStart,
    Progress,
    FileComplete,
    BatchComplete,
}

/// 统一进度事件。
#[derive(Debug, Clone, Serialize)]
pub struct UnifiedProgress {
    /// 所属会话 ID（由 broadcaster 填充，用于前端跨会话过滤）
    #[serde(default)]
    pub session_id: String,
    /// 单次传输唯一 ID（由 orchestrator/broadcaster 注入）
    #[serde(default)]
    pub transfer_id: String,
    pub kind: TransferProgressKind,
    pub protocol: String,
    pub file_name: String,
    pub bytes_done: u64,
    /// 0 表示未知大小。
    pub bytes_total: u64,
    /// 后端 I/O 层测得的可靠速率（字节/秒）；None 表示没有样本。
    pub bytes_per_second: Option<f64>,
    pub file_index: usize,
    pub total_files: usize,
    pub aggregate_bytes: u64,
    pub aggregate_total: u64,
    pub direction: TransferDirection,
    pub file_success: Option<bool>,
    pub file_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgressPosition {
    pub file_index: usize,
    pub total_files: usize,
    pub aggregate_bytes: u64,
    pub aggregate_total: u64,
}

/// 基于单调时钟的 payload 吞吐率采样器。
///
/// 采样器只消费后端已经确认完成的累计 payload 字节，不依赖 WebView/IPC 到达时间。
/// 首个样本只建立基线；后续样本在一个短滑动窗口内计算平均速率，既避免把协议启动
/// 握手计入首块速度，也能抑制每块 ACK 带来的瞬时抖动。
#[derive(Debug)]
pub struct TransferRateMeter {
    samples: VecDeque<(Instant, u64)>,
    window: Duration,
    min_sample_interval: Duration,
}

impl Default for TransferRateMeter {
    fn default() -> Self {
        Self::new(Duration::from_secs(2), Duration::from_millis(200))
    }
}

impl TransferRateMeter {
    pub fn new(window: Duration, min_sample_interval: Duration) -> Self {
        Self {
            samples: VecDeque::new(),
            window,
            min_sample_interval,
        }
    }

    /// 记录一次累计 payload 字节样本，并在样本跨度足够时返回 bytes/s。
    ///
    /// 若累计字节回退（例如调用方切换到新的独立计数域），自动清空旧窗口重新建基线。
    pub fn sample(&mut self, bytes_done: u64) -> Option<f64> {
        let now = Instant::now();

        if self
            .samples
            .back()
            .is_some_and(|(_, previous_bytes)| bytes_done < *previous_bytes)
        {
            self.samples.clear();
        }

        self.samples.push_back((now, bytes_done));
        while self.samples.len() > 2 {
            let should_drop = self
                .samples
                .front()
                .is_some_and(|(sampled_at, _)| now.duration_since(*sampled_at) > self.window);
            if !should_drop {
                break;
            }
            self.samples.pop_front();
        }

        let (started_at, started_bytes) = *self.samples.front()?;
        let elapsed = now.duration_since(started_at);
        let delta = bytes_done.saturating_sub(started_bytes);
        if elapsed < self.min_sample_interval || delta == 0 {
            return None;
        }

        let bytes_per_second = delta as f64 / elapsed.as_secs_f64();
        (bytes_per_second.is_finite() && bytes_per_second > 0.0).then_some(bytes_per_second)
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

impl UnifiedProgress {
    pub fn file_start(
        protocol: &str,
        file_name: &str,
        file_size: u64,
        position: ProgressPosition,
        direction: TransferDirection,
    ) -> Self {
        let ProgressPosition {
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
        } = position;
        Self {
            session_id: String::new(),
            transfer_id: String::new(),
            kind: TransferProgressKind::FileStart,
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done: 0,
            bytes_total: file_size,
            bytes_per_second: None,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            file_success: None,
            file_error: None,
        }
    }

    pub fn chunk(
        protocol: &str,
        file_name: &str,
        bytes_done: u64,
        bytes_total: u64,
        position: ProgressPosition,
        direction: TransferDirection,
    ) -> Self {
        let ProgressPosition {
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
        } = position;
        Self {
            session_id: String::new(),
            transfer_id: String::new(),
            kind: TransferProgressKind::Progress,
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done,
            bytes_total,
            bytes_per_second: None,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            file_success: None,
            file_error: None,
        }
    }

    /// 带后端真实 I/O 测速样本的逐块进度。
    pub fn chunk_with_speed(
        protocol: &str,
        file_name: &str,
        bytes_done: u64,
        bytes_total: u64,
        position: ProgressPosition,
        direction: TransferDirection,
        bytes_per_second: Option<f64>,
    ) -> Self {
        let mut progress = Self::chunk(
            protocol,
            file_name,
            bytes_done,
            bytes_total,
            position,
            direction,
        );
        progress.bytes_per_second =
            bytes_per_second.filter(|value| value.is_finite() && *value > 0.0);
        progress
    }

    pub fn file_complete(
        protocol: &str,
        file_name: &str,
        bytes_transferred: u64,
        bytes_total: u64,
        position: ProgressPosition,
        direction: TransferDirection,
        success: bool,
        error: Option<String>,
    ) -> Self {
        let ProgressPosition {
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
        } = position;
        Self {
            session_id: String::new(),
            transfer_id: String::new(),
            kind: TransferProgressKind::FileComplete,
            protocol: protocol.to_string(),
            file_name: file_name.to_string(),
            bytes_done: bytes_transferred,
            bytes_total,
            bytes_per_second: None,
            file_index,
            total_files,
            aggregate_bytes,
            aggregate_total,
            direction,
            file_success: Some(success),
            file_error: error,
        }
    }

    /// 构造协议层批次完成事件。
    ///
    /// Skip 是用户明确选择的冲突策略结果，不属于执行失败；只有真正 failed 文件
    /// 才把 file_success 置 false。任务的最终 completed/failed/cancelled 仍由
    /// `file-transfer:finished` 唯一决定。
    pub fn batch_complete(
        protocol: &str,
        direction: TransferDirection,
        files_completed: usize,
        files_failed: usize,
        files_skipped: usize,
    ) -> Self {
        Self {
            session_id: String::new(),
            transfer_id: String::new(),
            kind: TransferProgressKind::BatchComplete,
            protocol: protocol.to_string(),
            file_name: String::new(),
            bytes_done: 0,
            bytes_total: 0,
            bytes_per_second: None,
            file_index: 0,
            total_files: files_completed + files_failed + files_skipped,
            aggregate_bytes: 0,
            aggregate_total: 0,
            direction,
            file_success: Some(files_failed == 0),
            file_error: None,
        }
    }
}

/// 目标冲突处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwritePolicy {
    /// 使用临时文件 + 安全提交替换现有目标。
    #[default]
    Replace,
    /// 若目标已存在则跳过，不视为传输失败。
    Skip,
    /// 若目标已存在则选择一个不冲突的新名称。
    KeepBoth,
}

impl OverwritePolicy {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value
            .unwrap_or("replace")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "replace" => Ok(Self::Replace),
            "skip" => Ok(Self::Skip),
            "keep-both" | "keep_both" | "keepboth" => Ok(Self::KeepBoth),
            other => Err(format!("无效的覆盖策略: {other}")),
        }
    }
}

/// 一次传输的协议无关选项。
#[derive(Debug, Clone, Default)]
pub struct FileTransferOptions {
    pub overwrite_policy: OverwritePolicy,
    /// 与源路径按索引对应；为空时由协议根据目标目录与源文件名推导。
    pub destination_paths: Vec<String>,
}

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

/// 真正协议无关的文件传输扩展点。
#[async_trait::async_trait]
pub trait FileTransfer: Send + Sync {
    fn protocol(&self) -> &str;

    async fn send(
        &self,
        files: &[crate::transfer::types::FileInfo],
        remote_dir: Option<&str>,
        options: &FileTransferOptions,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<crate::transfer::types::BatchFileResult>, FileTransferError>;

    async fn receive(
        &self,
        download_dir: &str,
        remote_paths: &[String],
        options: &FileTransferOptions,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<crate::transfer::types::BatchFileResult>, FileTransferError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_rate_meter_uses_delta_after_baseline() {
        let mut meter = TransferRateMeter::new(Duration::from_secs(1), Duration::ZERO);
        assert_eq!(meter.sample(1024), None);
        std::thread::sleep(Duration::from_millis(2));
        let rate = meter
            .sample(2048)
            .expect("second sample should produce a rate");
        assert!(rate.is_finite());
        assert!(rate > 0.0);
    }

    #[test]
    fn transfer_rate_meter_resets_when_counter_moves_backwards() {
        let mut meter = TransferRateMeter::new(Duration::from_secs(1), Duration::ZERO);
        assert_eq!(meter.sample(4096), None);
        std::thread::sleep(Duration::from_millis(1));
        assert!(meter.sample(8192).is_some());
        assert_eq!(meter.sample(1024), None);
    }

    #[test]
    fn failed_file_complete_keeps_original_total() {
        let progress = UnifiedProgress::file_complete(
            "ymodem",
            "firmware.bin",
            1024,
            4096,
            ProgressPosition {
                file_index: 0,
                total_files: 1,
                aggregate_bytes: 1024,
                aggregate_total: 4096,
            },
            TransferDirection::Send,
            false,
            Some("failed".into()),
        );
        assert_eq!(progress.bytes_done, 1024);
        assert_eq!(progress.bytes_total, 4096);
        assert_eq!(progress.file_success, Some(false));
    }
}
