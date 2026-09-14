//! 传输编排器 — 策略隔离与统一任务生命周期。
//!
//! 所有策略遵守同一启动契约：validate/setup → reserve/register task → emit started
//! → return TransferStartAck。实际传输始终在后台任务中执行，终态只通过
//! `file-transfer:finished` 表达。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::kernel::file_transfer::{
    FileTransfer, FileTransferError, FileTransferOptions, UnifiedProgress,
};
use crate::kernel::plugin_adapter::{TransferExecutionMode, TransferProtocolType};
use crate::kernel::session_store::SessionState;
use crate::transfer::config::{ReceiveProtocolOptions, SendProtocolOptions};
use crate::transfer::panic_guard::PanicGuard;
use crate::transfer::protocol::SerialTransferProtocol;
use crate::transfer::serial_transfer::SerialFileTransfer;
use crate::transfer::types::{BatchFileResult, FileInfo};
use crate::AppState;

pub struct SendContext {
    pub session_id: String,
    pub files: Vec<FileInfo>,
    pub remote_dir: Option<String>,
    pub options: FileTransferOptions,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub protocol_options: SendProtocolOptions,
}

pub struct ReceiveContext {
    pub session_id: String,
    pub download_dir: String,
    pub remote_paths: Vec<String>,
    pub options: FileTransferOptions,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub protocol_options: ReceiveProtocolOptions,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferStartAck {
    pub transfer_id: String,
}

#[async_trait]
pub trait TransferOrchestrator: Send + Sync {
    #[allow(dead_code)]
    fn protocol(&self) -> &str;

    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;

    #[allow(dead_code)]
    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String>;
}

/// 协议 descriptor 到执行策略的唯一解析入口。
pub fn create_orchestrator(
    protocol_type: &TransferProtocolType,
) -> Result<Box<dyn TransferOrchestrator>, String> {
    let descriptor = protocol_type
        .descriptor()
        .ok_or_else(|| format!("不支持的传输协议: '{}'", protocol_type))?;
    match descriptor.execution_mode {
        TransferExecutionMode::Inline => Ok(Box::new(InlineTransferOrchestrator {
            pt: protocol_type.clone(),
        })),
        TransferExecutionMode::Auxiliary => Ok(Box::new(AuxiliaryTransferOrchestrator {
            pt: protocol_type.clone(),
        })),
        TransferExecutionMode::SeparateConnection => Err(format!(
            "协议 '{}' 的独立连接传输策略尚未实现",
            protocol_type
        )),
    }
}

pub fn spawn_progress_broadcaster(
    app: AppHandle,
    mut rx: UnboundedReceiver<UnifiedProgress>,
    session_id: String,
    transfer_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(mut progress) = rx.recv().await {
            progress.session_id = session_id.clone();
            progress.transfer_id = transfer_id.clone();
            let _ = app.emit("file-transfer:progress", &progress);
        }
    })
}

fn restore_session_state(app: &AppHandle, session_id: &str, transfer_id: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut store) = state.session_store.lock() {
            if let Some(handle) = store.get_session_mut(session_id) {
                let _ = handle.transfer_scheduler.finish(Some(transfer_id));
                if handle.state != SessionState::Disconnected {
                    handle.state = SessionState::Connected;
                }
            }
        }
    }
}

fn transfer_terminal_summary(
    result: &Result<Vec<BatchFileResult>, FileTransferError>,
) -> (bool, bool, Option<String>) {
    match result {
        Ok(results) => {
            let failed = results
                .iter()
                .filter(|item| item.status == "failed")
                .count();
            if failed == 0 {
                (true, false, None)
            } else {
                let first_error = results
                    .iter()
                    .filter(|item| item.status == "failed")
                    .filter_map(|item| item.error.as_deref())
                    .next()
                    .unwrap_or("部分文件传输失败");
                (
                    false,
                    false,
                    Some(format!("{} 个文件传输失败：{}", failed, first_error)),
                )
            }
        }
        Err(FileTransferError::Cancelled) => {
            (false, true, Some(FileTransferError::Cancelled.to_string()))
        }
        Err(error) => (false, false, Some(error.to_string())),
    }
}

fn emit_transfer_finished(
    app: &AppHandle,
    client_session_id: &str,
    transfer_id: &str,
    protocol: &str,
    result: &Result<Vec<BatchFileResult>, FileTransferError>,
) {
    let (success, cancelled, error) = transfer_terminal_summary(result);
    let results = result.as_ref().ok();
    let _ = app.emit(
        "file-transfer:finished",
        serde_json::json!({
            "session_id": client_session_id,
            "transfer_id": transfer_id,
            "protocol": protocol,
            "success": success,
            "cancelled": cancelled,
            "error": error,
            "results": results,
        }),
    );
}

fn emit_transfer_started(
    app: &AppHandle,
    client_session_id: &str,
    transfer_id: &str,
    protocol: &str,
    direction: &str,
) {
    let _ = app.emit(
        "file-transfer:started",
        serde_json::json!({
            "session_id": client_session_id,
            "transfer_id": transfer_id,
            "protocol": protocol,
            "direction": direction,
        }),
    );
}

pub struct InlineTransferOrchestrator {
    pt: TransferProtocolType,
}

impl InlineTransferOrchestrator {
    fn create_send_protocol_handler(
        &self,
        options: &SendProtocolOptions,
    ) -> Result<Box<dyn SerialTransferProtocol>, String> {
        match options {
            SendProtocolOptions::Ymodem { block_size } => {
                Ok(Box::new(crate::transfer::ymodem::YModem {
                    block_size: *block_size,
                }))
            }
            SendProtocolOptions::Xmodem { block_size } => Ok(Box::new(
                crate::transfer::xmodem::XModem::sender(*block_size),
            )),
            SendProtocolOptions::Zmodem {
                crc_policy,
                max_block_size,
            } => Ok(Box::new(crate::transfer::zmodem::ZModem::sender(
                *crc_policy,
                *max_block_size,
            ))),
            SendProtocolOptions::Sftp => Err("SFTP 不是内联串口协议".into()),
        }
    }

    fn create_receive_protocol_handler(
        &self,
        options: &ReceiveProtocolOptions,
    ) -> Result<Box<dyn SerialTransferProtocol>, String> {
        match options {
            ReceiveProtocolOptions::Ymodem => {
                Ok(Box::new(crate::transfer::ymodem::YModem::default()))
            }
            ReceiveProtocolOptions::Xmodem { check_mode } => Ok(Box::new(
                crate::transfer::xmodem::XModem::receiver(*check_mode),
            )),
            ReceiveProtocolOptions::Zmodem { crc_capability } => Ok(Box::new(
                crate::transfer::zmodem::ZModem::receiver(*crc_capability),
            )),
            ReceiveProtocolOptions::Sftp => Err("SFTP 不是内联串口协议".into()),
        }
    }

    fn acquire_exclusive_io(
        &self,
        app: &AppHandle,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<
        (
            Box<dyn crate::transfer::protocol::TransferIo>,
            Arc<AtomicBool>,
        ),
        String,
    > {
        let (io, cancel) = {
            let app_state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
            let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(session_id);
            let io = {
                let handle = store.get_session(session_id).ok_or(not_found)?;
                if handle.state != SessionState::Connected {
                    return Err("会话未连接".into());
                }
                handle
                    .io
                    .as_ref()
                    .cloned()
                    .ok_or("当前会话没有可独占的数据面")?
            };
            let cancel = store.reserve_inline_transfer(session_id, transfer_id)?;
            let not_found = store.session_not_found(session_id);
            let handle = store.get_session_mut(session_id).ok_or(not_found)?;
            handle.state = SessionState::Transferring;
            (io, cancel)
        };

        match io.acquire_exclusive(format!("file-transfer:{transfer_id}")) {
            Ok(lease) => Ok((Box::new(lease), cancel)),
            Err(error) => {
                restore_session_state(app, session_id, transfer_id);
                Err(format!("无法获取文件传输独占 I/O: {error}"))
            }
        }
    }
}

#[async_trait]
impl TransferOrchestrator for InlineTransferOrchestrator {
    fn protocol(&self) -> &str {
        self.pt.as_str()
    }

    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;
        let protocol_handler = match self.create_send_protocol_handler(&ctx.protocol_options) {
            Ok(handler) => handler,
            Err(error) => {
                drop(io);
                restore_session_state(&app, &ctx.session_id, &transfer_id);
                return Err(error);
            }
        };
        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, io);
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();

        let task_app = app.clone();
        let task_sid = ctx.session_id.clone();
        let task_client_id = client_id.clone();
        let task_transfer_id = transfer_id.clone();
        let task_protocol = self.pt.clone();
        let task_files = ctx.files;
        let task_options = ctx.options;
        let progress_tx = ctx.progress_tx;
        let handle = tokio::spawn(async move {
            if start_rx.await.is_err() {
                drop(progress_tx);
                let _ = broadcaster.await;
                drop(transfer);
                restore_session_state(&task_app, &task_sid, &task_transfer_id);
                return;
            }
            let result = transfer
                .send(
                    &task_files,
                    None,
                    &task_options,
                    progress_tx.clone(),
                    cancel,
                )
                .await;
            drop(progress_tx);
            let _ = broadcaster.await;
            drop(transfer);
            restore_session_state(&task_app, &task_sid, &task_transfer_id);
            emit_transfer_finished(
                &task_app,
                &task_client_id,
                &task_transfer_id,
                task_protocol.as_str(),
                &result,
            );
        });

        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&ctx.session_id, handle)?;
        }
        emit_transfer_started(&app, &client_id, &transfer_id, self.pt.as_str(), "send");
        let _ = start_tx.send(());
        Ok(ack)
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;
        let protocol_handler = match self.create_receive_protocol_handler(&ctx.protocol_options) {
            Ok(handler) => handler,
            Err(error) => {
                drop(io);
                restore_session_state(&app, &ctx.session_id, &transfer_id);
                return Err(error);
            }
        };
        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, io);
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();

        let task_app = app.clone();
        let task_sid = ctx.session_id.clone();
        let task_client_id = client_id.clone();
        let task_transfer_id = transfer_id.clone();
        let task_protocol = self.pt.clone();
        let download_dir = ctx.download_dir;
        let task_options = ctx.options;
        let progress_tx = ctx.progress_tx;
        let handle = tokio::spawn(async move {
            if start_rx.await.is_err() {
                drop(progress_tx);
                let _ = broadcaster.await;
                drop(transfer);
                restore_session_state(&task_app, &task_sid, &task_transfer_id);
                return;
            }
            let result = transfer
                .receive(
                    &download_dir,
                    &[],
                    &task_options,
                    progress_tx.clone(),
                    cancel,
                )
                .await;
            drop(progress_tx);
            let _ = broadcaster.await;
            drop(transfer);
            restore_session_state(&task_app, &task_sid, &task_transfer_id);
            emit_transfer_finished(
                &task_app,
                &task_client_id,
                &task_transfer_id,
                task_protocol.as_str(),
                &result,
            );
        });

        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&ctx.session_id, handle)?;
        }
        emit_transfer_started(&app, &client_id, &transfer_id, self.pt.as_str(), "receive");
        let _ = start_tx.send(());
        Ok(ack)
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
}

pub struct AuxiliaryTransferOrchestrator {
    pt: TransferProtocolType,
}

#[async_trait]
impl TransferOrchestrator for AuxiliaryTransferOrchestrator {
    fn protocol(&self) -> &str {
        self.pt.as_str()
    }

    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let internal_id = ctx.session_id.clone();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&internal_id);
            let handle = store.get_session_mut(&internal_id).ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            let ft = handle
                .file_transfer
                .clone()
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&internal_id, &transfer_id)?;
            (ft, cancel_flag)
        };
        let protocol = ft.protocol().to_string();
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();

        let task_app = app.clone();
        let task_internal_id = internal_id.clone();
        let task_client_id = client_id.clone();
        let task_protocol = protocol.clone();
        let task_transfer_id = transfer_id.clone();
        let files = ctx.files;
        let remote_dir = ctx.remote_dir;
        let options = ctx.options;
        let progress_tx = ctx.progress_tx;
        let handle = tokio::spawn(async move {
            let mut guard = PanicGuard::new(
                task_app.clone(),
                task_internal_id,
                task_client_id.clone(),
                task_protocol.clone(),
                task_transfer_id.clone(),
            );
            if start_rx.await.is_err() {
                drop(progress_tx);
                let _ = broadcaster.await;
                return;
            }
            let result = ft
                .send(
                    &files,
                    remote_dir.as_deref(),
                    &options,
                    progress_tx.clone(),
                    cancel_flag,
                )
                .await;
            drop(progress_tx);
            let _ = broadcaster.await;
            guard.complete();
            emit_transfer_finished(
                &task_app,
                &task_client_id,
                &task_transfer_id,
                &task_protocol,
                &result,
            );
        });

        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&internal_id, handle)?;
        }
        emit_transfer_started(&app, &client_id, &transfer_id, &protocol, "send");
        let _ = start_tx.send(());
        Ok(ack)
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let internal_id = ctx.session_id.clone();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&internal_id);
            let handle = store.get_session_mut(&internal_id).ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            let ft = handle
                .file_transfer
                .clone()
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&internal_id, &transfer_id)?;
            (ft, cancel_flag)
        };
        let protocol = ft.protocol().to_string();
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();

        let task_app = app.clone();
        let task_internal_id = internal_id.clone();
        let task_client_id = client_id.clone();
        let task_protocol = protocol.clone();
        let task_transfer_id = transfer_id.clone();
        let download_dir = ctx.download_dir;
        let remote_paths = ctx.remote_paths;
        let options = ctx.options;
        let progress_tx = ctx.progress_tx;
        let handle = tokio::spawn(async move {
            let mut guard = PanicGuard::new(
                task_app.clone(),
                task_internal_id,
                task_client_id.clone(),
                task_protocol.clone(),
                task_transfer_id.clone(),
            );
            if start_rx.await.is_err() {
                drop(progress_tx);
                let _ = broadcaster.await;
                return;
            }
            let result = ft
                .receive(
                    &download_dir,
                    &remote_paths,
                    &options,
                    progress_tx.clone(),
                    cancel_flag,
                )
                .await;
            drop(progress_tx);
            let _ = broadcaster.await;
            guard.complete();
            emit_transfer_finished(
                &task_app,
                &task_client_id,
                &task_transfer_id,
                &task_protocol,
                &result,
            );
        });

        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&internal_id, handle)?;
        }
        emit_transfer_started(&app, &client_id, &transfer_id, &protocol, "receive");
        let _ = start_tx.send(());
        Ok(ack)
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
}
