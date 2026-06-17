//! 协议无关 I/O 循环引擎
//!
//! 同步和异步 I/O 循环，基于 `dyn Channel` trait。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use crate::channel::Channel;

/// I/O 循环命令
#[derive(Debug)]
pub enum IoLoopCmd {
    Write(Vec<u8>),
    Shutdown,
    /// 端口移交（用于 Inline 传输策略）
    HandoffPort {
        give_tx: mpsc::SyncSender<Box<dyn Channel>>,
        return_rx: mpsc::Receiver<Box<dyn Channel>>,
    },
}

/// 启动同步 I/O 循环（std::thread 驱动）
///
/// 适用于串口、Pipe 等阻塞式传输。
/// 返回 `JoinHandle`。
#[allow(clippy::too_many_arguments)]
pub fn spawn_sync_io_loop(
    mut channel: Box<dyn Channel>,
    session_id: String,
    mut on_data: impl FnMut(String, Vec<u8>) + Send + 'static,
    mut on_disconnect: impl FnMut(String) + Send + 'static,
    write_rx: mpsc::Receiver<IoLoopCmd>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();

    // 取消监听线程
    std::thread::spawn(move || {
        let _ = cancel_rx.blocking_recv();
        cancel_flag_clone.store(true, Ordering::SeqCst);
    });

    let tick = Duration::from_millis(1);

    std::thread::spawn(move || {
        let mut read_buf = [0u8; 4096];

        loop {
            // 1. 检查取消信号
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }

            // 2. 尝试读取
            match channel.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    on_data(session_id.clone(), read_buf[..n].to_vec());
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => {
                    on_disconnect(session_id.clone());
                    break;
                }
            }

            // 3. 处理所有排队的写操作（公平调度）
            loop {
                match write_rx.try_recv() {
                    Ok(IoLoopCmd::Write(data)) => {
                        if channel.write_all(&data).is_err() || channel.flush().is_err() {
                            on_disconnect(session_id.clone());
                            return;
                        }
                        tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                    }
                    Ok(IoLoopCmd::Shutdown) => return,
                    Ok(IoLoopCmd::HandoffPort { give_tx, return_rx }) => {
                        // 交出 channel 所有权给传输代码
                        let _ = give_tx.send(channel);
                        // 阻塞等待 channel 归还
                        loop {
                            if cancel_flag.load(Ordering::SeqCst) {
                                return;
                            }
                            match return_rx.recv_timeout(Duration::from_millis(100)) {
                                Ok(returned_channel) => {
                                    channel = returned_channel;
                                    break;
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            }
                        }
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            // 4. 短暂休眠避免忙等
            std::thread::sleep(tick);
        }
    })
}

/// 启动异步 I/O 循环（tokio 驱动）
///
/// 供未来网络协议插件（SSH、Telnet 等）使用，当前仅同步 I/O 循环处于活跃状态。
#[allow(dead_code)]
///
/// 基础骨架——完整的异步 I/O 需要 async Channel trait，这是后续工作。
/// 当前使用 spawn_blocking 桥接同步 Channel 到 tokio 运行时。
#[allow(clippy::too_many_arguments)]
pub fn spawn_async_io_loop(
    channel: Box<dyn Channel>,
    session_id: String,
    on_data: impl Fn(String, Vec<u8>) + Send + 'static,
    on_disconnect: impl Fn(String) + Send + 'static,
    _write_rx: tokio::sync::mpsc::Receiver<IoLoopCmd>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    _tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_inner = cancel_flag.clone();

        // 取消监听 task
        tokio::spawn(async move {
            let _ = cancel_rx.await;
            cancel_flag_inner.store(true, Ordering::SeqCst);
        });

        // 使用 spawn_blocking 在独立线程中运行阻塞 I/O
        // 完整异步实现在后续版本完善
        let _ = tokio::task::spawn_blocking(move || {
            let mut ch = channel;
            let sid = session_id;
            let od = on_data;
            let odc = on_disconnect;
            let mut read_buf = [0u8; 4096];

            loop {
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }

                match ch.read(&mut read_buf) {
                    Ok(n) if n > 0 => {
                        rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                        od(sid.clone(), read_buf[..n].to_vec());
                    }
                    Ok(_) => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(_) => {
                        odc(sid.clone());
                        break;
                    }
                }

                std::thread::sleep(Duration::from_millis(1));
            }
        }).await;
    })
}
