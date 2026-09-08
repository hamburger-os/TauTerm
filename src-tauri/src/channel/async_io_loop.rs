//! 协议无关异步 I/O 循环引擎
//!
//! 基于 tokio task 驱动，适用于 russh 等 async SSH 库。
//! 与同步 `io_loop::spawn_sync_io_loop` 并存：串口用 Sync，SSH 用 Async。

use crate::channel::io_loop::{IoLoopCmd, IoLoopContext};
use crate::channel::{AsyncChannel, DisconnectInfo};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
pub fn spawn_async_io_loop(
    mut channel: Box<dyn AsyncChannel>,
    mut on_data: impl FnMut(String, Vec<u8>) + Send + 'static,
    mut on_disconnect: impl FnMut(String, DisconnectInfo) + Send + 'static,
    context: IoLoopContext,
) -> tokio::task::JoinHandle<()> {
    let IoLoopContext {
        session_id,
        write_rx,
        cancel_rx,
        tx_bytes,
        rx_bytes,
    } = context;
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
                            let fallback = DisconnectInfo::remote_eof("Remote endpoint closed the session");
                            let info = channel.disconnect_info(fallback);
                            on_disconnect(session_id.clone(), info);
                            break;
                        }
                        Ok(n) => {
                            rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                            on_data(session_id.clone(), read_buf[..n].to_vec());
                        }
                        Err(e) => {
                            log::debug!("async io_loop read error: {}", e);
                            let fallback = DisconnectInfo::io_error(e.to_string());
                            let info = channel.disconnect_info(fallback);
                            on_disconnect(session_id.clone(), info);
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
            )
            .await
            {
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
    on_disconnect: &mut impl FnMut(String, DisconnectInfo),
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
                let fallback = DisconnectInfo::io_error("Failed writing to remote endpoint");
                let info = channel.disconnect_info(fallback);
                on_disconnect(session_id.to_string(), info);
                return false;
            }
            tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
            true
        }
        IoLoopCmd::Shutdown => {
            if let Err(error) = channel.shutdown().await {
                log::warn!("Async channel graceful shutdown failed: {error}");
            }
            false
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::error::ChannelError;
    use crate::channel::DisconnectKind;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

    struct MockAsyncChannel {
        reads: Arc<Mutex<VecDeque<Vec<u8>>>>,
        writes: Arc<Mutex<Vec<u8>>>,
        fail_write: bool,
        max_write: usize,
    }

    #[async_trait::async_trait]
    impl AsyncChannel for MockAsyncChannel {
        async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let data = {
                let mut reads = self.reads.lock().unwrap();
                reads.pop_front()
            };
            if let Some(data) = data {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(n)
            } else {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"))
            }
        }

        async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.fail_write {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "injected async write failure",
                ));
            }
            let n = buf.len().min(self.max_write.max(1));
            self.writes.lock().unwrap().extend_from_slice(&buf[..n]);
            Ok(n)
        }

        async fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }

        fn set_timeout(&mut self, _dur: Duration) -> Result<(), ChannelError> {
            Ok(())
        }
    }

    fn context(
        session_id: &str,
    ) -> (
        IoLoopContext,
        std::sync::mpsc::SyncSender<IoLoopCmd>,
        tokio::sync::oneshot::Sender<()>,
        Arc<AtomicU64>,
        Arc<AtomicU64>,
    ) {
        let (write_tx, write_rx) = std::sync::mpsc::sync_channel(16);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        (
            IoLoopContext {
                session_id: session_id.into(),
                write_rx,
                cancel_rx,
                tx_bytes: tx_bytes.clone(),
                rx_bytes: rx_bytes.clone(),
            },
            write_tx,
            cancel_tx,
            tx_bytes,
            rx_bytes,
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_io_loop_handles_partial_writes_and_counts_bytes() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let (context, write_tx, _cancel_tx, tx_bytes, _rx_bytes) = context("async-write");
        let handle = spawn_async_io_loop(
            Box::new(MockAsyncChannel {
                reads: Arc::new(Mutex::new(VecDeque::new())),
                writes: writes.clone(),
                fail_write: false,
                max_write: 2,
            }),
            |_id, _data| {},
            |_id, _info| {},
            context,
        );

        write_tx.send(IoLoopCmd::Write(b"abcdef".to_vec())).unwrap();
        write_tx.send(IoLoopCmd::Shutdown).unwrap();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(&*writes.lock().unwrap(), b"abcdef");
        assert_eq!(tx_bytes.load(Ordering::Relaxed), 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_io_loop_reports_injected_write_failure() {
        let (disconnect_tx, disconnect_rx) = tokio::sync::oneshot::channel();
        let disconnect_tx = Arc::new(Mutex::new(Some(disconnect_tx)));
        let (context, write_tx, _cancel_tx, tx_bytes, _rx_bytes) = context("async-fail");
        let tx_for_callback = disconnect_tx.clone();
        let handle = spawn_async_io_loop(
            Box::new(MockAsyncChannel {
                reads: Arc::new(Mutex::new(VecDeque::new())),
                writes: Arc::new(Mutex::new(Vec::new())),
                fail_write: true,
                max_write: usize::MAX,
            }),
            |_id, _data| {},
            move |_id, info| {
                if let Some(tx) = tx_for_callback.lock().unwrap().take() {
                    let _ = tx.send(info);
                }
            },
            context,
        );

        write_tx.send(IoLoopCmd::Write(b"boom".to_vec())).unwrap();
        let info = tokio::time::timeout(Duration::from_secs(2), disconnect_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(info.kind, DisconnectKind::IoError));
        assert_eq!(tx_bytes.load(Ordering::Relaxed), 0);
    }
}
