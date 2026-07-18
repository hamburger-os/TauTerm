//! 协议无关异步 I/O 循环引擎
//!
//! 基于 tokio task 驱动，适用于 russh 等 async SSH 库。
//! 与同步 `io_loop::spawn_sync_io_loop` 并存：串口用 Sync，SSH 用 Async。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use crate::channel::AsyncChannel;
use crate::channel::io_loop::IoLoopCmd;

/// 启动异步 I/O 循环（tokio task 驱动）
///
/// 适用于 SSH（russh async API）等基于 tokio 的协议。
/// 返回 `tokio::task::JoinHandle<()>`。
///
/// 与 `spawn_sync_io_loop` 契约一致：
/// - 同样的 `IoLoopCmd` 命令通道（保留 `std::sync::mpsc::SyncSender<IoLoopCmd>` 避免波及串口路径）
/// - 同样的 `on_data`/`on_disconnect` 回调
/// - 同样的取消机制（`tokio::sync::oneshot`）
/// - 同样的 `tx_bytes`/`rx_bytes` 计数
#[allow(clippy::too_many_arguments)]
pub fn spawn_async_io_loop(
    mut channel: Box<dyn AsyncChannel>,
    session_id: String,
    mut on_data: impl FnMut(String, Vec<u8>) + Send + 'static,
    mut on_disconnect: impl FnMut(String) + Send + 'static,
    write_rx: mpsc::Receiver<IoLoopCmd>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // 将 std::sync::mpsc::Receiver 包成异步流。
        // 使用 tokio::task::spawn_blocking 避免在 async 上下文中阻塞 recv。
        let write_rx = tokio::task::spawn_blocking(move || {
            // 在 blocking 线程中阻塞接收命令，通过 channel 转发给 async 任务
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<IoLoopCmd>();
            std::thread::spawn(move || {
                while let Ok(cmd) = write_rx.recv() {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
            });
            rx
        });
        let mut write_rx = match write_rx.await {
            Ok(rx) => rx,
            Err(_) => return,
        };

        let mut cancel_rx = cancel_rx;
        let mut read_buf = [0u8; 16384];

        loop {
            tokio::select! {
                biased;

                // 1. 取消信号（最高优先级）
                _ = &mut cancel_rx => {
                    break;
                }

                // 2. 写命令
                Some(cmd) = write_rx.recv() => {
                    if !handle_cmd_async(
                        cmd,
                        &mut channel,
                        &session_id,
                        &tx_bytes,
                        &mut on_disconnect,
                    ).await {
                        return;
                    }
                }

                // 3. 读取
                read_result = channel.read(&mut read_buf) => {
                    match read_result {
                        Ok(0) => {
                            // 远端关闭
                            on_disconnect(session_id.clone());
                            break;
                        }
                        Ok(n) => {
                            rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                            on_data(session_id.clone(), read_buf[..n].to_vec());
                        }
                        Err(e) => {
                            log::debug!("async io_loop read error: {}", e);
                            on_disconnect(session_id.clone());
                            break;
                        }
                    }
                }
            }
        }

        // 排空剩余写命令
        while let Ok(cmd) = write_rx.try_recv() {
            if !handle_cmd_async(
                cmd,
                &mut channel,
                &session_id,
                &tx_bytes,
                &mut on_disconnect,
            ).await {
                return;
            }
        }
    })
}

/// 处理一条 IoLoopCmd（异步版本）。返回 false 表示应退出循环。
async fn handle_cmd_async(
    cmd: IoLoopCmd,
    channel: &mut Box<dyn AsyncChannel>,
    session_id: &str,
    tx_bytes: &Arc<AtomicU64>,
    on_disconnect: &mut impl FnMut(String),
) -> bool {
    match cmd {
        IoLoopCmd::Write(data) => {
            let len = data.len();
            let mut written = 0;
            let mut ok = true;
            while written < len {
                match channel.write(&data[written..]).await {
                    Ok(0) => {
                        ok = false;
                        break;
                    }
                    Ok(n) => written += n,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && channel.flush().await.is_err() {
                ok = false;
            }
            if !ok {
                on_disconnect(session_id.to_string());
                return false;
            }
            tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
            true
        }
        IoLoopCmd::Shutdown => false,
        IoLoopCmd::ResizePty { cols, rows } => {
            if let Err(e) = channel.resize_pty(cols, rows).await {
                log::warn!("PTY resize 失败: {}", e);
            }
            true
        }
        IoLoopCmd::HandoffPort { .. } => {
            // SSH 使用 SideChannel 策略，不应触发 HandoffPort
            log::error!("异步 I/O 循环收到 HandoffPort 命令（SSH 不支持端口移交）");
            false
        }
    }
}
