//! 传输编排器 — 策略隔离层
//!
//! 每种传输策略（Inline / SideChannel / SeparateConnection）拥有独立的
//! 编排器实现，封装完整的传输生命周期：setup → execute → cleanup。
//! commands.rs 为薄路由层，通过 `create_orchestrator()` 分发到对应实现。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::channel::io_loop::IoLoopCmd;
use crate::channel::Channel;
use crate::kernel::file_transfer::{FileTransfer, UnifiedProgress};
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
    pub session_id: String,
    pub files: Vec<FileInfo>,
    pub remote_dir: Option<String>,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
}

/// 接收传输上下文（download）
pub struct ReceiveContext {
    pub session_id: String,
    pub download_dir: String,
    pub remote_paths: Vec<String>,
    pub progress_tx: UnboundedSender<UnifiedProgress>,
    pub progress_rx: UnboundedReceiver<UnifiedProgress>,
    pub block_size: Option<usize>,
    pub checksum_mode: Option<String>,
    pub streaming: Option<bool>,
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
    async fn execute_send(&self, app: AppHandle, ctx: SendContext) -> Result<(), String>;

    /// 执行接收（download）传输
    async fn execute_receive(&self, app: AppHandle, ctx: ReceiveContext) -> Result<(), String>;

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
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(mut progress) = rx.recv().await {
            progress.session_id = session_id.clone();
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
                h.cancel_transfer_tx = None;
                h.channel_return_tx = None;
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
            Ok(Box::new(crate::transfer::ymodem::YModem {
                block_size: bs,
            }))
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
    ) -> Result<
        (
            Box<dyn serialport::SerialPort>,
            tokio::sync::oneshot::Receiver<()>,
        ),
        String,
    > {
        let (give_tx, give_rx) =
            std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
        let (return_tx, return_rx) =
            std::sync::mpsc::sync_channel::<Box<dyn Channel>>(1);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        {
            let app_state = app
                .try_state::<AppState>()
                .ok_or("无法获取应用状态")?;
            let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(session_id);
            let handle = store.get_session_mut(session_id).ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            if handle.transfer_cancel.is_some() {
                return Err("该会话已有传输进行中，请等待完成或取消后再试".into());
            }
            handle.state = SessionState::Transferring;
            handle.channel_return_tx = Some(return_tx);
            handle.cancel_transfer_tx = Some(cancel_tx);
            let _ = handle.write_tx.send(IoLoopCmd::HandoffPort {
                give_tx,
                return_rx,
            });
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
        port: Box<dyn serialport::SerialPort>,
    ) {
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Ok(mut store) = app_state.session_store.lock() {
                if let Some(h) = store.get_session_mut(session_id) {
                    h.cancel_transfer_tx = None;
                    h.state = SessionState::Connected;
                    if let Some(tx) = h.channel_return_tx.take() {
                        let new_channel =
                            crate::channel::serial_channel::SerialChannel::new(port);
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

    async fn execute_send(&self, app: AppHandle, ctx: SendContext) -> Result<(), String> {
        // 1. Handoff 端口（return_tx 和 cancel_tx 已存入 session handle）
        let (port, cancel_rx) = self.handoff_port(&app, &ctx.session_id)?;

        // 2. 创建协议处理器 + SerialFileTransfer
        //    若协议处理器创建失败，必须归还端口，否则 I/O 线程永久阻塞
        let protocol_handler = match self.create_protocol_handler(
            ctx.block_size,
            ctx.checksum_mode,
            ctx.streaming,
        ) {
            Ok(h) => h,
            Err(e) => {
                self.return_port(&app, &ctx.session_id, port);
                emit_transfer_failed(&app, &ctx.session_id, self.pt.as_str());
                return Err(e);
            }
        };
        let transfer =
            SerialFileTransfer::new(self.pt.clone(), protocol_handler, port);

        let sid = ctx.session_id.clone();
        let proto_str = self.pt.to_string();

        // 3. 广播进度
        spawn_progress_broadcaster(app.clone(), ctx.progress_rx, sid.clone());

        // 4. 后台取消监听（cancel_rx 由 handoff 阶段创建，cancel_tx 已存入 session）
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let _cancel_thread = std::thread::spawn(move || {
            let _ = cancel_rx.blocking_recv();
            c.store(true, Ordering::SeqCst);
        });

        // 5. 发射启动事件
        let _ = app.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &sid,
                "protocol": &proto_str,
                "direction": "send",
            }),
        );

        // 6. 执行传输
        let progress_tx_clone = ctx.progress_tx.clone();
        let result = transfer
            .send(&ctx.files, None, progress_tx_clone, cancel)
            .await;

        // 7. 归还端口
        match transfer.take_port() {
            Ok(port) => {
                self.return_port(&app, &sid, port);
            }
            Err(e) => {
                log::error!("无法归还端口: {}", e);
                if let Some(app_state) = app.try_state::<AppState>() {
                    if let Ok(mut store) = app_state.session_store.lock() {
                        if let Some(h) = store.get_session_mut(&sid) {
                            h.cancel_transfer_tx = None;
                            h.state = SessionState::Connected;
                            h.channel_return_tx = None;
                        }
                    }
                }
            }
        }

        // 8. 发射完成事件并返回结果
        match result {
            Ok(_) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({ "session_id": &sid, "protocol": &proto_str, "success": true }),
                );
                Ok(())
            }
            Err(e) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &sid,
                        "protocol": &proto_str,
                        "success": false,
                        "error": e.to_string(),
                    }),
                );
                Err(e.to_string())
            }
        }
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
    ) -> Result<(), String> {
        // 1. Handoff 端口（return_tx 和 cancel_tx 已存入 session handle）
        let (port, cancel_rx) = self.handoff_port(&app, &ctx.session_id)?;

        // 2. 创建协议处理器 + SerialFileTransfer
        //    若协议处理器创建失败，必须归还端口，否则 I/O 线程永久阻塞
        let protocol_handler = match self.create_protocol_handler(
            ctx.block_size,
            ctx.checksum_mode,
            ctx.streaming,
        ) {
            Ok(h) => h,
            Err(e) => {
                self.return_port(&app, &ctx.session_id, port);
                emit_transfer_failed(&app, &ctx.session_id, self.pt.as_str());
                return Err(e);
            }
        };
        let transfer =
            SerialFileTransfer::new(self.pt.clone(), protocol_handler, port);

        let sid = ctx.session_id.clone();
        let proto_str = self.pt.to_string();

        // 3. 广播进度
        spawn_progress_broadcaster(app.clone(), ctx.progress_rx, sid.clone());

        // 4. 后台取消监听
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let _cancel_thread = std::thread::spawn(move || {
            let _ = cancel_rx.blocking_recv();
            c.store(true, Ordering::SeqCst);
        });

        // 5. 发射启动事件
        let _ = app.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &sid,
                "protocol": &proto_str,
                "direction": "receive",
            }),
        );

        // 6. 执行传输（内联串口接收：remote_paths 为空，由协议层自行协商文件列表）
        let progress_tx_clone = ctx.progress_tx.clone();
        let result = transfer
            .receive(&ctx.download_dir, &[], progress_tx_clone, cancel)
            .await;

        // 7. 归还端口
        match transfer.take_port() {
            Ok(port) => {
                self.return_port(&app, &sid, port);
            }
            Err(e) => {
                log::error!("无法归还端口: {}", e);
                if let Some(app_state) = app.try_state::<AppState>() {
                    if let Ok(mut store) = app_state.session_store.lock() {
                        if let Some(h) = store.get_session_mut(&sid) {
                            h.cancel_transfer_tx = None;
                            h.state = SessionState::Connected;
                            h.channel_return_tx = None;
                        }
                    }
                }
            }
        }

        // 8. 发射完成事件并返回结果
        match result {
            Ok(_) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({ "session_id": &sid, "protocol": &proto_str, "success": true }),
                );
                Ok(())
            }
            Err(e) => {
                let _ = app.emit(
                    "file-transfer:finished",
                    serde_json::json!({
                        "session_id": &sid,
                        "protocol": &proto_str,
                        "success": false,
                        "error": e.to_string(),
                    }),
                );
                Err(e.to_string())
            }
        }
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app
            .try_state::<AppState>()
            .ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        let not_found = store.session_not_found(session_id);
        let handle = store
            .get_session_mut(session_id)
            .ok_or(not_found)?;

        if let Some(tx) = handle.cancel_transfer_tx.take() {
            let _ = tx.send(());
            Ok(())
        } else {
            Err("没有正在进行的传输".into())
        }
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

    async fn execute_send(&self, app: AppHandle, ctx: SendContext) -> Result<(), String> {
        let app_for_spawn = app.clone();

        let state = app
            .try_state::<AppState>()
            .ok_or("无法获取应用状态")?;

        // 1. 从 SideChannel 获取 FileTransfer 并设置取消标志
        //    合并为一次锁获取，消除 TOCTOU 窗口
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&ctx.session_id);
            let handle = store
                .get_session_mut(&ctx.session_id)
                .ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            if handle.transfer_cancel.is_some() {
                return Err("该会话已有传输进行中，请等待完成或取消后再试".into());
            }
            let ft = handle
                .side_channel
                .as_ref()
                .and_then(|sc| sc.create_file_transfer())
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&ctx.session_id)?;
            (ft, cancel_flag)
        };

        let sid = ctx.session_id.clone();
        let rd = ctx.remote_dir.clone();
        let files = ctx.files.clone();

        // 3. 广播进度
        spawn_progress_broadcaster(app_for_spawn.clone(), ctx.progress_rx, sid.clone());

        // 4. 发射启动事件
        let _ = app_for_spawn.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &sid,
                "protocol": ft.protocol(),
                "direction": "send",
            }),
        );

        log::info!(
            "侧通道发送: protocol={}, {} 个文件 → {:?}",
            ft.protocol(),
            files.len(),
            rd
        );

        // 5. 后台执行传输（RAII 守卫确保 panic/abort 安全）
        let progress_tx_clone = ctx.progress_tx.clone();
        let handle = tokio::spawn(async move {
            let mut guard = PanicGuard::new(app_for_spawn.clone(), sid.clone());

            let result = ft
                .send(&files, rd.as_deref(), progress_tx_clone, cancel_flag.clone())
                .await;

            // 传输完成（成功或失败），emit 事件后 defuse 守卫。
            // transfer_done 由 PanicGuard::drop 统一处理，避免重复调用。
            let _ = app_for_spawn.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &sid,
                    "protocol": ft.protocol(),
                    "success": result.is_ok(),
                    "error": result.as_ref().err().map(|e| e.to_string()),
                }),
            );
            guard.defuse();
        });

        // 6. 注册 task handle（供 close_session 等待）
        if let Ok(mut store) = state.session_store.lock() {
            let _ = store.register_transfer_task(&ctx.session_id, handle);
        }

        Ok(())
    }

    async fn execute_receive(
        &self,
        app: AppHandle,
        ctx: ReceiveContext,
    ) -> Result<(), String> {
        let app_for_spawn = app.clone();

        let state = app
            .try_state::<AppState>()
            .ok_or("无法获取应用状态")?;

        // 1. 从 SideChannel 获取 FileTransfer 并设置取消标志
        //    合并为一次锁获取，消除 TOCTOU 窗口
        let (ft, cancel_flag) = {
            let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(&ctx.session_id);
            let handle = store
                .get_session_mut(&ctx.session_id)
                .ok_or(not_found)?;
            if handle.state != SessionState::Connected {
                return Err("会话未连接".into());
            }
            if handle.transfer_cancel.is_some() {
                return Err("该会话已有传输进行中，请等待完成或取消后再试".into());
            }
            let ft = handle
                .side_channel
                .as_ref()
                .and_then(|sc| sc.create_file_transfer())
                .ok_or_else(|| "此会话不支持侧通道文件传输".to_string())?;
            let cancel_flag = store.transfer_start(&ctx.session_id)?;
            (ft, cancel_flag)
        };

        let sid = ctx.session_id.clone();
        let download_dir = ctx.download_dir.clone();
        let remote_paths = ctx.remote_paths.clone();

        // 3. 广播进度
        spawn_progress_broadcaster(app_for_spawn.clone(), ctx.progress_rx, sid.clone());

        // 4. 发射启动事件
        let _ = app_for_spawn.emit(
            "file-transfer:started",
            serde_json::json!({
                "session_id": &sid,
                "protocol": ft.protocol(),
                "direction": "receive",
            }),
        );

        log::info!(
            "侧通道接收: protocol={}, {} 个文件 → {}",
            ft.protocol(),
            remote_paths.len(),
            download_dir
        );

        // 5. 后台执行传输（RAII 守卫确保 panic/abort 安全）
        let progress_tx_clone = ctx.progress_tx.clone();
        let handle = tokio::spawn(async move {
            let mut guard = PanicGuard::new(app_for_spawn.clone(), sid.clone());

            let result = ft
                .receive(
                    &download_dir,
                    &remote_paths,
                    progress_tx_clone,
                    cancel_flag.clone(),
                )
                .await;

            // 传输完成（成功或失败），emit 事件后 defuse 守卫。
            // transfer_done 由 PanicGuard::drop 统一处理，避免重复调用。
            let _ = app_for_spawn.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &sid,
                    "protocol": ft.protocol(),
                    "success": result.is_ok(),
                    "error": result.as_ref().err().map(|e| e.to_string()),
                }),
            );
            guard.defuse();
        });

        if let Ok(mut store) = state.session_store.lock() {
            let _ = store.register_transfer_task(&ctx.session_id, handle);
        }

        Ok(())
    }

    fn cancel(&self, app: AppHandle, session_id: &str) -> Result<(), String> {
        let state = app
            .try_state::<AppState>()
            .ok_or("无法获取应用状态")?;
        let mut store = state.session_store.lock().map_err(|e| e.to_string())?;
        store.cancel_transfer_op(session_id)
    }
}
