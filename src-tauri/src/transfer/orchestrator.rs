//! 传输编排器 — 策略隔离层
//!
//! 每种传输策略（Inline / SideChannel / SeparateConnection）拥有独立的
//! 编排器实现，封装完整的传输生命周期：setup → execute → cleanup。
//! commands.rs 为薄路由层，通过 `create_orchestrator()` 分发到对应实现。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::channel::io_loop::IoLoopCmd;
use crate::channel::Channel;
use crate::kernel::file_transfer::{
    FileTransfer, FileTransferError, FileTransferOptions, UnifiedProgress,
};
use crate::kernel::plugin_adapter::TransferProtocolType;
use crate::kernel::session_store::SessionState;
use crate::transfer::panic_guard::PanicGuard;
use crate::transfer::protocol::TransferProtocol;
use crate::transfer::serial_transfer::SerialFileTransfer;
use crate::transfer::types::FileInfo;
use crate::AppState;

// ── Context types ──────────────────────────────────────────────────────────

/// 发送传输上下文（upload）
pub struct SendContext {
    /// 内部会话 ID — 用于 SessionStore 操作
    pub session_id: String,
    pub files: Vec<FileInfo>,
    pub remote_dir: Option<String>,
    pub options: FileTransferOptions,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
}

/// 接收传输上下文（download）
pub struct ReceiveContext {
    /// 内部会话 ID — 用于 SessionStore 操作
    pub session_id: String,
    pub download_dir: String,
    pub remote_paths: Vec<String>,
    pub options: FileTransferOptions,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
}

/// 启动命令的确认结果。SideChannel 传输在后台任务注册完成后立即返回该 ID；
/// Inline 传输仍保持现有阻塞调用语义，但也返回同一身份模型。
#[derive(Debug, Clone, Serialize)]
pub struct TransferStartAck {
    pub transfer_id: String,
}

// ── Trait ──────────────────────────────────────────────────────────────────

/// 传输编排器 — 每个策略独立实现完整的传输生命周期
///
/// 职责：
/// 1. 从 session 获取/创建 FileTransfer 实例
/// 2. 设置并发守卫和取消信号
/// 3. 执行传输
/// 4. 清理资源（归还端口、释放锁、emit 完成事件）
/// 5. panic 安全（Drop 守卫确保清理）
#[async_trait]
pub trait TransferOrchestrator: Send + Sync {
    /// 协议标识（用于日志和事件）
    #[allow(dead_code)]
    fn protocol(&self) -> &str;

    /// 执行发送（upload）传输
    /// `client_session_id` — 前端传入的原始 sessionId，用于事件回传
    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;

    /// 执行接收（download）传输
    /// `client_session_id` — 前端传入的原始 sessionId，用于事件回传
    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_session_id: String,
    ) -> Result<TransferStartAck, String>;

    /// 取消正在进行的传输
    #[allow(dead_code)]
    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String>;
}

// ── Factory ────────────────────────────────────────────────────────────────

/// 根据协议类型创建对应的编排器
pub fn create_orchestrator(
    protocol_type: &TransferProtocolType,
) -> Result<Box<dyn TransferOrchestrator>, String> {
    if protocol_type.is_serial_inline() {
        Ok(Box::new(InlineTransferOrchestrator {
            pt: protocol_type.clone(),
        }))
    } else if protocol_type.is_side_channel() {
        Ok(Box::new(SideChannelTransferOrchestrator {
            pt: protocol_type.clone(),
        }))
    } else if protocol_type.is_separate_connection() {
        Err(format!(
            "协议 '{}' 的独立连接传输策略尚未实现",
            protocol_type
        ))
    } else {
        // 防御性：FromStr 已做白名单验证，此分支理论上不可达
        Err(format!("不支持的传输协议: '{}'", protocol_type))
    }
}

// ── Progress broadcaster (shared helper) ───────────────────────────────────

/// 在后台 task 中将 UnifiedProgress 广播为 Tauri 事件
/// session_id 在此注入，使前端可按会话过滤跨会话进度事件
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

// ── Helper: restore session state on error ─────────────────────────────────

fn restore_session_state(app: &AppHandle, session_id: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut store) = state.session_store.lock() {
            if let Some(h) = store.get_session_mut(session_id) {
                h.state = SessionState::Connected;
                h.channel_return_tx = None;
                let _ = h.transfer_scheduler.finish(None);
            }
        }
    }
}

fn emit_transfer_failed(app: &AppHandle, session_id: &str, protocol: &str) {
    let _ = app.emit(
        "file-transfer:finished",
        serde_json::json!({
            "session_id": session_id,
            "protocol": protocol,
            "success": false,
            "error": "传输启动失败",
        }),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  InlineTransferOrchestrator
// ═══════════════════════════════════════════════════════════════════════════

/// 内联传输编排器 — 串口协议（YModem / XModem / ZModem）
///
/// 传输期间从 I/O 线程接管串口（HandoffPort），在 spawn_blocking 中
/// 运行同步协议引擎，完成后归还端口。
pub struct InlineTransferOrchestrator {
    #[allow(dead_code)]
    pt: TransferProtocolType,
}

impl InlineTransferOrchestrator {
    /// 创建协议处理器（根据协议类型和用户参数）
    fn create_protocol_handler(
        &self,
        block_size: Option<usize>,
        checksum_mode: Option<String>,
        streaming: Option<bool>,
    ) -> Result<Box<dyn TransferProtocol>, String> {
        if self.pt.as_str() == "ymodem" {
            let bs = block_size.unwrap_or(1024).clamp(128, 1024);
            if let Some(ref cm) = checksum_mode {
                log::info!("YModem checksum_mode 请求: {}（协议自行协商）", cm);
            }
            if streaming.unwrap_or(false) {
                log::info!("YModem streaming 模式请求（协议自行协商）");
            }
            Ok(Box::new(crate::transfer::ymodem::YModem { block_size: bs }))
        } else {
            crate::transfer::protocol::create_protocol(&self.pt)
                .ok_or_else(|| format!("{} 协议未实现", self.pt))
        }
    }

    /// 从 I/O 线程接管串口
    /// 返回: (串口, 取消信号接收端)
    fn handoff_port(
        &self,
        app: &AppHandle,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<
        (
            Box<dyn serialport::SerialPort>,
            tokio::sync::oneshot::Receiver<()>,
        ),
        String,
    > {
        let (give_tx, give_rx) = std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
        let (return_tx, return_rx) = std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        {
            let app_state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
            let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(session_id);
            let write_tx = {
                let handle = store.get_session_mut(session_id).ok_or(not_found)?;
                if handle.state != SessionState::Connected {
                    return Err("会话未连接".into());
                }
                handle
                    .write_tx
                    .as_ref()
                    .cloned()
                    .ok_or("容器会话不支持端口移交（HandoffPort）")?
            };

            // 所有可能失败的静态前置检查完成后再占用 Scheduler 槽，避免启动失败留下 busy。
            store.reserve_inline_transfer(session_id, transfer_id, cancel_tx)?;
            let not_found = store.session_not_found(session_id);
            let handle = store.get_session_mut(session_id).ok_or(not_found)?;
            handle.state = SessionState::Transferring;
            handle.channel_return_tx = Some(return_tx);
            let _ = write_tx.send(IoLoopCmd::HandoffPort { give_tx, return_rx });
        }

        let mut channel = give_rx.recv().map_err(|e| {
            restore_session_state(app, session_id);
            format!("无法从 I/O 线程获取 Channel: {}", e)
        })?;

        let port_box = channel.try_handoff().ok_or_else(|| {
            emit_transfer_failed(app, session_id, self.pt.as_str());
            restore_session_state(app, session_id);
            "Channel 不支持端口移交".to_string()
        })?;

        let port = port_box
            .downcast::<Box<dyn serialport::SerialPort>>()
            .map_err(|_| {
                emit_transfer_failed(app, session_id, self.pt.as_str());
                restore_session_state(app, session_id);
                "端口类型转换失败".to_string()
            })?;
        drop(channel);

        Ok((*port, cancel_rx))
    }

    /// 归还串口到 I/O 线程（从 session handle 取出 channel_return_tx）
    fn return_port(
        &self,
        app: &AppHandle,
        session_id: &str,
        transfer_id: &str,
        port: Box<dyn serialport::SerialPort>,
    ) {
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Ok(mut store) = app_state.session_store.lock() {
                if let Some(h) = store.get_session_mut(session_id) {
                    let _ = h.transfer_scheduler.finish(Some(transfer_id));
                    h.state = SessionState::Connected;
                    if let Some(tx) = h.channel_return_tx.take() {
                        let new_channel = crate::channel::serial_channel::SerialChannel::new(port);
                        if let Err(e) = tx.send(Box::new(new_channel)) {
                            log::error!(
                                "return_port: 无法归还端口到 I/O 线程（receiver 已断开）— \
                                 端口已丢失 (session: {}): {:?}",
                                session_id,
                                e
                            );
                        }
                    }
                }
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
        // 1. 先分配任务身份，再由 Scheduler 预留 Inline 活动槽并 Handoff 端口。
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (port, cancel_rx) =
            self.handoff_port(&app, &ctx.session_id, &transfer_id)?;

        // 2. 创建协议处理器 + SerialFileTransfer
        //    若协议处理器创建失败，必须归还端口，否则 I/O 线程永久阻塞
        let protocol_handler =
            match self.create_protocol_handler(ctx.block_size, ctx.checksum_mode, ctx.streaming) {
                Ok(h) => h,
                Err(e) => {
                    self.return_port(&app, &ctx.session_id, &transfer_id, port);
                    emit_transfer_failed(&app, &ctx.session_id, self.pt.as_str());
                    return Err(e);
                }
            };
        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, port);

        let sid = ctx.session_id.clone();
        let proto_str = self.pt.to_string();

        // 3. 广播进度 — client_id + transfer_id。完成事件必须等待队列 drain。
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );

        // 4. 后台取消监听（cancel_rx 由 handoff 阶段创建，cancel_tx 已存入 session）
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let _cancel_thread = std::thread::spawn(move || {
            let _ = cancel_rx.blocking_recv();
            c.store(true, Ordering::SeqCst);
        });

        // 5. 发射启动事件 — client_id + transfer_id
        let _ = app.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &client_id,
                "transfer_id": &transfer_id,
                "protocol": &proto_str,
                "direction": "send",
            }),
        );

        // 6. 执行传输
        let progress_tx_clone = ctx.progress_tx.clone();
        let result = transfer
            .send(&ctx.files, None, &ctx.options, progress_tx_clone, cancel)
            .await;
        drop(ctx.progress_tx);
        let _ = broadcaster.await;

        // 7. 归还端口。此步骤完成后下一次串口传输才可安全启动。
        match transfer.take_port() {
            Ok(port) => {
                self.return_port(&app, &sid, &transfer_id, port);
            }
            Err(e) => {
                log::error!("无法归还端口: {}", e);
                if let Some(app_state) = app.try_state::<AppState>() {
                    if let Ok(mut store) = app_state.session_store.lock() {
                        if let Some(h) = store.get_session_mut(&sid) {
                            let _ = h.transfer_scheduler.finish(Some(&transfer_id));
                            h.state = SessionState::Connected;
                            h.channel_return_tx = None;
                        }
                    }
                }
            }
        }

        // 8. progress 队列已排空、资源已归还后才发射 finished。
        match result {
            Ok(_) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &client_id,
                        "transfer_id": &transfer_id,
                        "protocol": &proto_str,
                        "success": true,
                        "cancelled": false,
                    }),
                );
                Ok(TransferStartAck {
                    transfer_id: transfer_id.clone(),
                })
            }
            Err(e) => {
                let cancelled = matches!(&e, FileTransferError::Cancelled);
                let error = e.to_string();
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &client_id,
                        "transfer_id": &transfer_id,
                        "protocol": &proto_str,
                        "success": false,
                        "cancelled": cancelled,
                        "error": &error,
                    }),
                );
                Err(error)
            }
        }
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        // 1. 先分配任务身份，再由 Scheduler 预留 Inline 活动槽并 Handoff 端口。
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let (port, cancel_rx) =
            self.handoff_port(&app, &ctx.session_id, &transfer_id)?;

        // 2. 创建协议处理器 + SerialFileTransfer
        //    若协议处理器创建失败，必须归还端口，否则 I/O 线程永久阻塞
        let protocol_handler =
            match self.create_protocol_handler(ctx.block_size, ctx.checksum_mode, ctx.streaming) {
                Ok(h) => h,
                Err(e) => {
                    self.return_port(&app, &ctx.session_id, &transfer_id, port);
                    emit_transfer_failed(&app, &ctx.session_id, self.pt.as_str());
                    return Err(e);
                }
            };
        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, port);

        let sid = ctx.session_id.clone();
        let proto_str = self.pt.to_string();

        // 3. 广播进度 — client_id + transfer_id。完成事件必须等待队列 drain。
        let broadcaster = spawn_progress_broadcaster(
            app.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );

        // 4. 后台取消监听
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let _cancel_thread = std::thread::spawn(move || {
            let _ = cancel_rx.blocking_recv();
            c.store(true, Ordering::SeqCst);
        });

        // 5. 发射启动事件 — client_id + transfer_id
        let _ = app.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &client_id,
                "transfer_id": &transfer_id,
                "protocol": &proto_str,
                "direction": "receive",
            }),
        );

        // 6. 执行传输（内联串口接收：remote_paths 为空，由协议层自行协商文件列表）
        let progress_tx_clone = ctx.progress_tx.clone();
        let result = transfer
            .receive(
                &ctx.download_dir,
                &[],
                &ctx.options,
                progress_tx_clone,
                cancel,
            )
            .await;
        drop(ctx.progress_tx);
        let _ = broadcaster.await;

        // 7. 归还端口
        match transfer.take_port() {
            Ok(port) => {
                self.return_port(&app, &sid, &transfer_id, port);
            }
            Err(e) => {
                log::error!("无法归还端口: {}", e);
                if let Some(app_state) = app.try_state::<AppState>() {
                    if let Ok(mut store) = app_state.session_store.lock() {
                        if let Some(h) = store.get_session_mut(&sid) {
                            let _ = h.transfer_scheduler.finish(Some(&transfer_id));
                            h.state = SessionState::Connected;
                            h.channel_return_tx = None;
                        }
                    }
                }
            }
        }

        // 8. progress 队列已排空、资源已归还后才发射 finished。
        match result {
            Ok(_) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &client_id,
                        "transfer_id": &transfer_id,
                        "protocol": &proto_str,
                        "success": true,
                        "cancelled": false,
                    }),
                );
                Ok(TransferStartAck {
                    transfer_id: transfer_id.clone(),
                })
            }
            Err(e) => {
                let cancelled = matches!(&e, FileTransferError::Cancelled);
                let error = e.to_string();
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &client_id,
                        "transfer_id": &transfer_id,
                        "protocol": &proto_str,
                        "success": false,
                        "cancelled": cancelled,
                        "error": &error,
                    }),
                );
                Err(error)
            }
        }
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SideChannelTransferOrchestrator
// ═══════════════════════════════════════════════════════════════════════════

/// 侧通道传输编排器 — SSH SFTP
///
/// 通过 Session 的 SideChannel 获取 FileTransfer 实例，在 tokio task 中
/// 异步执行传输，完成后自动清理取消标志。
pub struct SideChannelTransferOrchestrator {
    #[allow(dead_code)]
    pt: TransferProtocolType,
}

#[async_trait]
impl TransferOrchestrator for SideChannelTransferOrchestrator {
    fn protocol(&self) -> &str {
        self.pt.as_str()
    }

    async fn execute_send(
        &self,
        app: AppHandle,
        ctx: SendContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let app_for_spawn = app.clone();

        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;

        let internal_id = ctx.session_id.clone();
        let transfer_id = uuid::Uuid::new_v4().to_string();

        // 1. 从 SideChannel 获取 FileTransfer 并设置取消标志
        //    合并为一次锁获取，消除 TOCTOU 窗口
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&internal_id);
            let handle = store.get_session_mut(&internal_id).ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            let ft = handle
                .side_channel
                .as_ref()
                .and_then(|sc| sc.create_file_transfer())
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&internal_id, &transfer_id)?;
            (ft, cancel_flag)
        };

        let rd = ctx.remote_dir.clone();
        let files = ctx.files.clone();
        let options = ctx.options.clone();
        let protocol = ft.protocol().to_string();
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };

        // 3. 广播进度 — client_id + transfer_id。finished 必须等待 broadcaster drain。
        let broadcaster = spawn_progress_broadcaster(
            app_for_spawn.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );

        log::info!(
            "侧通道发送: protocol={}, {} 个文件 → {:?}",
            protocol,
            files.len(),
            rd
        );

        // 4. 先创建一个带 start gate 的后台 task。task 在成功注册到 SessionStore 且
        // started 事件发出之前绝不访问传输资源，消除 close_session 与 task 注册竞态。
        let progress_tx = ctx.progress_tx;
        let internal_for_guard = internal_id.clone();
        let task_app = app_for_spawn.clone();
        let task_client_id = client_id.clone();
        let task_protocol = protocol.clone();
        let task_transfer_id = transfer_id.clone();
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }

            let mut guard = PanicGuard::new(
                task_app.clone(),
                internal_for_guard,
                task_client_id.clone(),
                task_protocol.clone(),
                task_transfer_id.clone(),
            );

            let result = ft
                .send(
                    &files,
                    rd.as_deref(),
                    &options,
                    progress_tx.clone(),
                    cancel_flag.clone(),
                )
                .await;

            // 关闭最后一个 sender 并等待所有 chunk/file_complete/batch_complete 真正 emit。
            drop(progress_tx);
            let _ = broadcaster.await;

            let cancelled = matches!(&result, Err(FileTransferError::Cancelled));
            let success = result.is_ok();
            let error = result.as_ref().err().map(|e| e.to_string());

            // 先释放 SessionStore 传输占用，再通知前端完成，消除立即连续上传竞态。
            guard.complete();
            let _ = task_app.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &task_client_id,
                    "transfer_id": &task_transfer_id,
                    "protocol": &task_protocol,
                    "success": success,
                    "cancelled": cancelled,
                    "error": error,
                }),
            );
        });

        // 5. 注册 task handle。持锁期间 session 不能被 close/remove；注册成功后才
        // 发布 started，随后打开 gate，保证 started → progress* → finished 顺序。
        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&internal_id, handle)?;
        }
        let _ = app_for_spawn.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &client_id,
                "transfer_id": &transfer_id,
                "protocol": &protocol,
                "direction": "send",
            }),
        );
        let _ = start_tx.send(());

        Ok(ack)
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
        client_id: String,
    ) -> Result<TransferStartAck, String> {
        let app_for_spawn = app.clone();

        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;

        let internal_id = ctx.session_id.clone();
        let transfer_id = uuid::Uuid::new_v4().to_string();

        // 1. 从 SideChannel 获取 FileTransfer 并设置取消标志
        //    合并为一次锁获取，消除 TOCTOU 窗口
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&internal_id);
            let handle = store.get_session_mut(&internal_id).ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            let ft = handle
                .side_channel
                .as_ref()
                .and_then(|sc| sc.create_file_transfer())
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&internal_id, &transfer_id)?;
            (ft, cancel_flag)
        };

        let download_dir = ctx.download_dir.clone();
        let remote_paths = ctx.remote_paths.clone();
        let options = ctx.options.clone();
        let protocol = ft.protocol().to_string();
        let ack = TransferStartAck {
            transfer_id: transfer_id.clone(),
        };

        // 3. 广播进度 — client_id + transfer_id。finished 必须等待 broadcaster drain。
        let broadcaster = spawn_progress_broadcaster(
            app_for_spawn.clone(),
            ctx.progress_rx,
            client_id.clone(),
            transfer_id.clone(),
        );

        log::info!(
            "侧通道接收: protocol={}, {} 个文件 → {}",
            protocol,
            remote_paths.len(),
            download_dir
        );

        // 4. 与发送路径相同：task 先注册、事件后发布、最后打开 start gate。
        let progress_tx = ctx.progress_tx;
        let internal_for_guard = internal_id.clone();
        let task_app = app_for_spawn.clone();
        let task_client_id = client_id.clone();
        let task_protocol = protocol.clone();
        let task_transfer_id = transfer_id.clone();
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }

            let mut guard = PanicGuard::new(
                task_app.clone(),
                internal_for_guard,
                task_client_id.clone(),
                task_protocol.clone(),
                task_transfer_id.clone(),
            );

            let result = ft
                .receive(
                    &download_dir,
                    &remote_paths,
                    &options,
                    progress_tx.clone(),
                    cancel_flag.clone(),
                )
                .await;

            drop(progress_tx);
            let _ = broadcaster.await;

            let cancelled = matches!(&result, Err(FileTransferError::Cancelled));
            let success = result.is_ok();
            let error = result.as_ref().err().map(|e| e.to_string());

            guard.complete();
            let _ = task_app.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &task_client_id,
                    "transfer_id": &task_transfer_id,
                    "protocol": &task_protocol,
                    "success": success,
                    "cancelled": cancelled,
                    "error": error,
                }),
            );
        });

        {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            store.register_transfer_task(&internal_id, handle)?;
        }
        let _ = app_for_spawn.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &client_id,
                "transfer_id": &transfer_id,
                "protocol": &protocol,
                "direction": "receive",
            }),
        );
        let _ = start_tx.send(());

        Ok(ack)
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_scheduled_transfer(session_id, None)
    }
}
