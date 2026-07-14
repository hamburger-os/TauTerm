//! 串口通信句柄
//!
//! `SerialCommHandle` 将 SessionStore 中已有的 `write_tx` mpsc channel
//! 封装为 `CommHandle` trait 实现，使脚本引擎可以通过统一接口操作串口。

use std::sync::{mpsc, Mutex};

use crate::channel::io_loop::IoLoopCmd;
use crate::kernel::comm_handle::{CommError, CommHandle, DataCallback};

/// 串口通信句柄
///
/// 持有 I/O 循环的写入通道，将脚本引擎的 send 请求路由到 IoLoop。
pub struct SerialCommHandle {
    /// I/O 循环写入通道（写入串口）
    write_tx: mpsc::SyncSender<IoLoopCmd>,
    /// 已注册的接收回调列表
    callbacks: Mutex<Vec<DataCallback>>,
}

impl SerialCommHandle {
    /// 创建新的串口通信句柄
    pub fn new(write_tx: mpsc::SyncSender<IoLoopCmd>) -> Self {
        Self {
            write_tx,
            callbacks: Mutex::new(Vec::new()),
        }
    }
}

impl CommHandle for SerialCommHandle {
    fn send(&self, data: &[u8]) -> Result<(), CommError> {
        // 写入通道关闭（IoLoop 已退出）即视为断开，send 自然返回 Err。
        self.write_tx
            .send(IoLoopCmd::Write(data.to_vec()))
            .map_err(|e| CommError::SendError(e.to_string()))
    }

    fn on_receive(&self, callback: DataCallback) {
        self.callbacks.lock().unwrap_or_else(|e| {
            log::warn!("SerialCommHandle: Mutex poisoned during on_receive, recovering");
            e.into_inner()
        }).push(callback);
    }

    fn notify_receive(&self, data: &[u8]) {
        // 注意：持有 callbacks Mutex 期间调用所有回调。回调实现在 I/O 线程
        // 中同步执行 — 必须快速返回（当前仅做 mpsc::Sender::send()）。
        // 禁止在回调中调用 on_receive() 或 clear_receivers()（会获取同一 Mutex 而死锁）。
        let callbacks = self.callbacks.lock().unwrap_or_else(|e| {
            log::warn!("SerialCommHandle: Mutex poisoned during notify_receive, recovering");
            e.into_inner()
        });
        for cb in callbacks.iter() {
            cb(data);
        }
    }

    fn clear_receivers(&self) {
        self.callbacks.lock().unwrap_or_else(|e| {
            log::warn!("SerialCommHandle: Mutex poisoned during clear_receivers, recovering");
            e.into_inner()
        }).clear();
    }
}
