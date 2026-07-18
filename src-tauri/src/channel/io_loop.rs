//! 协议无关 I/O 循环引擎
//!
//! 同步 I/O 循环，基于 `dyn Channel` trait。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
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
    /// 请求 PTY 窗口大小调整（SSH 专用，其他协议忽略）
    ResizePty { cols: u32, rows: u32 },
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

    // 读到数据后的"立即重读"窗口：在此时间内不做 sleep，直接进入下次 read，
    // 以便快速吸收连续到达的小包（SSH 远端命令输出典型场景）。
    // 仅在 read 返回 0 字节或 WouldBlock 后才进入等待状态。
    let read_spin_window = Duration::from_millis(2);
    // 无数据时等待写命令的超时，到期后重新尝试 read。
    // 用 recv_timeout 替代无条件 sleep，使写命令能立即被处理（降低发送延迟）。
    let idle_wait = Duration::from_millis(5);

    std::thread::spawn(move || {
        let mut read_buf = [0u8; 16384];
        let mut last_data_time: Option<Instant> = None;

        loop {
            // 1. 检查取消信号
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }

            // 2. 尝试读取
            let read_outcome: ReadOutcome = match channel.read(&mut read_buf) {
                Ok(n) if n > 0 => {
                    rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    on_data(session_id.clone(), read_buf[..n].to_vec());
                    last_data_time = Some(Instant::now());
                    ReadOutcome::Data
                }
                Ok(_) => ReadOutcome::Empty {
                    should_idle: should_idle_now(&last_data_time, read_spin_window),
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => ReadOutcome::Empty {
                    should_idle: should_idle_now(&last_data_time, read_spin_window),
                },
                Err(_) => {
                    on_disconnect(session_id.clone());
                    break;
                }
            };

            // 3. 处理排队的写操作
            // - 有数据时：try_recv 快速排空，让 read 尽快重入
            // - 无数据且 should_idle 时：recv_timeout 阻塞等待写命令（带超时）
            let need_blocking_wait = matches!(read_outcome, ReadOutcome::Empty { should_idle: true });
            if need_blocking_wait {
                match write_rx.recv_timeout(idle_wait) {
                    Ok(cmd) => {
                        if !handle_cmd(
                            cmd,
                            &mut channel,
                            &session_id,
                            &tx_bytes,
                            &cancel_flag,
                            &mut on_disconnect,
                        ) {
                            return;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                last_data_time = None;
            }

            // 4. 排空剩余写命令（非阻塞）
            loop {
                match write_rx.try_recv() {
                    Ok(cmd) => {
                        if !handle_cmd(
                            cmd,
                            &mut channel,
                            &session_id,
                            &tx_bytes,
                            &cancel_flag,
                            &mut on_disconnect,
                        ) {
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
        }
    })
}

/// read 结果归类（简化主循环分支）
enum ReadOutcome {
    /// 读到数据
    Data,
    /// 未读到数据（0 字节或 WouldBlock/TimedOut）
    Empty {
        /// 是否应进入阻塞等待（超出 spin 窗口）
        should_idle: bool,
    },
}

/// 判断是否应进入 idle 等待状态
fn should_idle_now(last_data_time: &Option<Instant>, spin_window: Duration) -> bool {
    match last_data_time {
        Some(t) => t.elapsed() > spin_window,
        None => true,
    }
}

/// 处理一条 IoLoopCmd。返回 false 表示应退出循环（断开 / Shutdown / HandoffPort 已迁移）。
#[allow(clippy::too_many_arguments)]
fn handle_cmd(
    cmd: IoLoopCmd,
    channel: &mut Box<dyn Channel>,
    session_id: &str,
    tx_bytes: &Arc<AtomicU64>,
    cancel_flag: &Arc<AtomicBool>,
    on_disconnect: &mut impl FnMut(String),
) -> bool {
    match cmd {
        IoLoopCmd::Write(data) => {
            if channel.write_all(&data).is_err() || channel.flush().is_err() {
                on_disconnect(session_id.to_string());
                return false;
            }
            tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            true
        }
        IoLoopCmd::Shutdown => false,
        IoLoopCmd::ResizePty { cols, rows } => {
            if let Err(e) = channel.resize_pty(cols, rows) {
                log::warn!("PTY resize 失败: {}", e);
            }
            true
        }
        IoLoopCmd::HandoffPort { give_tx, return_rx } => {
            // 交出 channel 所有权给传输代码
            let _ = give_tx.send(std::mem::replace(channel, Box::new(StubChannel)));
            // 阻塞等待 channel 归还
            loop {
                if cancel_flag.load(Ordering::SeqCst) {
                    return false;
                }
                match return_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(returned_channel) => {
                        *channel = returned_channel;
                        return true;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return false,
                }
            }
        }
    }
}

/// 占位 Channel，仅在 HandoffPort 期间临时持有
///
/// HandoffPort 把真实 channel 所有权交给传输层，原变量需要一个占位值。
/// 此时 I/O 循环不会读写它（处于 handoff 阻塞等待中），任何操作都是 bug。
struct StubChannel;

impl Read for StubChannel {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("StubChannel: read during handoff (bug)"))
    }
}

impl Write for StubChannel {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("StubChannel: write during handoff (bug)"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("StubChannel: flush during handoff (bug)"))
    }
}

impl Channel for StubChannel {
    fn is_connected(&self) -> bool {
        false
    }
    fn set_timeout(&mut self, _dur: Duration) -> Result<(), crate::channel::error::ChannelError> {
        Ok(())
    }
}
