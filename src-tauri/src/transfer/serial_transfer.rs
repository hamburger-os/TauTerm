//! 串口文件传输适配器
//!
//! 将现有的同步 `TransferProtocol` trait 适配到统一的异步 `FileTransfer` trait。
//! 通过 `tokio::task::spawn_blocking` 桥接同步协议引擎到 tokio 运行时。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::UnboundedSender;

use crate::kernel::file_transfer::{FileTransfer, FileTransferError, TransferDirection, UnifiedProgress};
use crate::kernel::plugin_adapter::TransferProtocolType;
use crate::transfer::protocol::TransferProtocol;
use crate::transfer::types::{BatchFileResult, FileInfo, FileTransferEvent, TransferProgress};

/// 串口文件传输适配器
pub struct SerialFileTransfer {
    protocol_type: TransferProtocolType,
    protocol: Arc<Box<dyn TransferProtocol>>,
    port: Arc<std::sync::Mutex<Box<dyn serialport::SerialPort>>>,
}

impl SerialFileTransfer {
    pub fn new(
        protocol_type: TransferProtocolType,
        protocol: Box<dyn TransferProtocol>,
        port: Box<dyn serialport::SerialPort>,
    ) -> Self {
        Self { protocol_type, protocol: Arc::new(protocol), port: Arc::new(std::sync::Mutex::new(port)) }
    }

    /// 取出端口（传输完成后归还 I/O 循环）
    pub fn take_port(self) -> Result<Box<dyn serialport::SerialPort>, String> {
        Arc::try_unwrap(self.port)
            .map_err(|_| "SerialFileTransfer: port still referenced (Arc not unique)".to_string())
            .and_then(|m| m.into_inner()
                .map_err(|e| format!("SerialFileTransfer: port mutex poisoned: {}", e)))
    }
}

#[async_trait::async_trait]
impl FileTransfer for SerialFileTransfer {
    fn protocol(&self) -> &str {
        self.protocol_type.as_str()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn send(
        &self,
        files: &[FileInfo],
        _remote_dir: Option<&str>,
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        let proto = self.protocol_type.to_string();
        let files = files.to_vec();
        let port = self.port.clone();
        let protocol = self.protocol.clone();
        let progress_clone = progress.clone();

        // Bug #2 fix: 预计算聚合总字节并追踪已完成字节，避免文件边界重置为 0
        let aggregate_total: u64 = files.iter().map(|f| f.size).sum();
        let aggregate_completed = Arc::new(std::sync::atomic::AtomicU64::new(0));

        log::info!("串口发送开始: protocol={}, files={}", proto, files.len());

        let result = tokio::task::spawn_blocking(move || {
            let mut port_guard = port.lock().unwrap_or_else(|e| e.into_inner());
            crate::transfer::io::flush_port_buffer(&mut port_guard);

            let progress = progress;
            let proto = proto;
            let aggregate_total = aggregate_total;
            let aggregate_completed = aggregate_completed;

            // 进度回调 — 在 spawn_blocking 内创建，生命周期覆盖 send_files 调用
            let on_progress = |p: TransferProgress| {
                let _ = progress.send(UnifiedProgress::chunk(
                    &proto, &p.file_name,
                    p.bytes_transferred, p.total_bytes,
                    p.file_index as usize, p.total_files as usize,
                    p.aggregate_bytes_transferred, p.aggregate_total_bytes,
                    TransferDirection::Send,
                ));
            };

            let progress2 = progress.clone();
            let proto2 = proto.clone();
            let ac_start = aggregate_completed.clone();
            let ac_complete = aggregate_completed.clone();
            let on_file_event = move |e: FileTransferEvent| {
                match e {
                    FileTransferEvent::FileStart { file_name, file_index, total_files, file_size } => {
                        let ac = ac_start.load(Ordering::SeqCst);
                        let _ = progress2.send(UnifiedProgress::file_start(
                            &proto2, &file_name, file_size,
                            file_index as usize, total_files as usize,
                            ac, aggregate_total,
                            TransferDirection::Send,
                        ));
                    }
                    FileTransferEvent::FileComplete { file_name, file_index, total_files, bytes_transferred, success, error } => {
                        let ac = ac_complete.load(Ordering::SeqCst);
                        let new_ac = ac + bytes_transferred;
                        if success {
                            ac_complete.store(new_ac, Ordering::SeqCst);
                        }
                        let _ = progress2.send(UnifiedProgress::file_complete(
                            &proto2, &file_name, bytes_transferred,
                            file_index as usize, total_files as usize,
                            if success { new_ac } else { ac }, aggregate_total,
                            TransferDirection::Send, success, error,
                        ));
                    }
                }
            };

            let mut cancel_fn = || cancel.load(Ordering::SeqCst);

            protocol.send_files(&mut port_guard, &files, &on_progress, &on_file_event, &mut cancel_fn)
                .map_err(|e| e.to_string())
        }).await;

        // Bug #6 fix: 即使传输失败也发送 batch_complete，避免前端状态卡住
        let proto_str = self.protocol_type.to_string();
        match result {
            Ok(Ok(batch_results)) => {
                let completed = batch_results.iter().filter(|r| r.status == "completed").count();
                let failed = batch_results.iter().filter(|r| r.status == "failed").count();
                let skipped = batch_results.iter().filter(|r| r.status == "skipped").count();
                log::info!(
                    "串口发送完成: {} 成功, {} 失败, {} 跳过",
                    completed, failed, skipped
                );
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Send,
                    completed, failed, skipped,
                ));
                Ok(batch_results)
            }
            Ok(Err(e)) => {
                log::error!("串口发送失败: {}", e);
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Send, 0, 1, 0,
                ));
                Err(FileTransferError::Other(e))
            }
            Err(join_err) => {
                log::error!("spawn_blocking join 失败: {}", join_err);
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Send, 0, 1, 0,
                ));
                Err(FileTransferError::Other(format!("spawn_blocking join 失败: {}", join_err)))
            }
        }
    }

    async fn receive(
        &self,
        download_dir: &str,
        _remote_paths: &[String],
        progress: UnboundedSender<UnifiedProgress>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<BatchFileResult>, FileTransferError> {
        let proto = self.protocol_type.to_string();
        let download_dir = download_dir.to_string();
        let port = self.port.clone();
        let protocol = self.protocol.clone();
        let progress_clone = progress.clone();

        // Bug #2 fix: 追踪聚合已完成字节，避免文件边界重置为 0
        // 接收端无法预知总字节，aggregate_total 保持 0（未知）
        let aggregate_completed = Arc::new(std::sync::atomic::AtomicU64::new(0));

        log::info!("串口接收开始: protocol={}, download_dir={}", proto, download_dir);

        let result = tokio::task::spawn_blocking(move || {
            let mut port_guard = port.lock().unwrap_or_else(|e| e.into_inner());
            crate::transfer::io::flush_port_buffer(&mut port_guard);

            let progress = progress;
            let proto = proto;
            let download_dir = download_dir;
            let aggregate_completed = aggregate_completed;

            let on_progress = |p: TransferProgress| {
                let _ = progress.send(UnifiedProgress::chunk(
                    &proto, &p.file_name,
                    p.bytes_transferred, p.total_bytes,
                    p.file_index as usize, p.total_files as usize,
                    p.aggregate_bytes_transferred, p.aggregate_total_bytes,
                    TransferDirection::Receive,
                ));
            };

            let progress2 = progress.clone();
            let proto2 = proto.clone();
            let ac_start = aggregate_completed.clone();
            let ac_complete = aggregate_completed.clone();
            let on_file_event = move |e: FileTransferEvent| {
                match e {
                    FileTransferEvent::FileStart { file_name, file_index, total_files, file_size } => {
                        let ac = ac_start.load(Ordering::SeqCst);
                        let _ = progress2.send(UnifiedProgress::file_start(
                            &proto2, &file_name, file_size,
                            file_index as usize, total_files as usize,
                            ac, 0, // 接收端 aggregate_total 未知
                            TransferDirection::Receive,
                        ));
                    }
                    FileTransferEvent::FileComplete { file_name, file_index, total_files, bytes_transferred, success, error } => {
                        let ac = ac_complete.load(Ordering::SeqCst);
                        let new_ac = ac + bytes_transferred;
                        if success {
                            ac_complete.store(new_ac, Ordering::SeqCst);
                        }
                        let _ = progress2.send(UnifiedProgress::file_complete(
                            &proto2, &file_name, bytes_transferred,
                            file_index as usize, total_files as usize,
                            if success { new_ac } else { ac }, 0,
                            TransferDirection::Receive, success, error,
                        ));
                    }
                }
            };

            let mut cancel_fn = || cancel.load(Ordering::SeqCst);

            protocol.receive_files(&mut port_guard, &download_dir, &on_progress, &on_file_event, &mut cancel_fn)
                .map_err(|e| e.to_string())
        }).await;

        // Bug #6 fix: 即使接收失败也发送 batch_complete
        let proto_str = self.protocol_type.to_string();
        match result {
            Ok(Ok(batch_results)) => {
                let completed = batch_results.iter().filter(|r| r.status == "completed").count();
                let failed = batch_results.iter().filter(|r| r.status == "failed").count();
                let skipped = batch_results.iter().filter(|r| r.status == "skipped").count();
                log::info!(
                    "串口接收完成: {} 成功, {} 失败, {} 跳过",
                    completed, failed, skipped
                );
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Receive,
                    completed, failed, skipped,
                ));
                Ok(batch_results)
            }
            Ok(Err(e)) => {
                log::error!("串口接收失败: {}", e);
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Receive, 0, 1, 0,
                ));
                Err(FileTransferError::Other(e))
            }
            Err(join_err) => {
                log::error!("spawn_blocking join 失败: {}", join_err);
                let _ = progress_clone.send(UnifiedProgress::batch_complete(
                    &proto_str, TransferDirection::Receive, 0, 1, 0,
                ));
                Err(FileTransferError::Other(format!("spawn_blocking join 失败: {}", join_err)))
            }
        }
    }
}
