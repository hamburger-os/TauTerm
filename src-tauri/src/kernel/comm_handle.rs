//! 通信抽象层
//!
//! 定义协议无关的 `CommHandle` trait，为脚本引擎提供统一的通信接口。
//! 当前仅实现串口 (SerialCommHandle)，未来 TCP/BLE/USB 等插件可实现
//! 同一 trait，使脚本引擎无需感知底层协议差异。
//!
//! ## 设计
//!
//! `CommHandle` 是 `Channel` trait 的上层抽象：
//! - `Channel` 关注字节流 I/O（Read/Write/超时/移交）
//! - `CommHandle` 关注通信语义（发送/接收回调/连接状态）
//!
//! `on_receive` 支持多个回调并行注册（终端显示 + 脚本引擎 + 日志记录）。

pub type DataCallback = Box<dyn Fn(&[u8]) + Send + 'static>;

/// 通信抽象 trait
///
/// 任何通信接口（串口、TCP、BLE、SSH channel 等）实现此 trait 后，
/// 即可被脚本引擎统一调用，无需感知底层协议。
pub trait CommHandle: Send + Sync {
    /// 向通信接口写入原始字节
    fn send(&self, data: &[u8]) -> Result<(), CommError>;

    /// 注册接收数据回调
    ///
    /// 可多次调用以注册多个回调（并行通知）。
    /// 回调在 I/O 线程中同步执行 — 实现者应确保回调快速返回。
    fn on_receive(&self, callback: DataCallback);

    /// 通知所有已注册的接收回调（数据扇出）
    ///
    /// 由 I/O 循环在收到数据后调用，将数据扇出到所有通过
    /// `on_receive()` 注册的消费者。
    fn notify_receive(&self, data: &[u8]);

    /// 清空所有已注册的接收回调
    ///
    /// 脚本引擎停止时调用，释放其注册的回调，避免 stop→start 循环
    /// 导致持废弃 channel 的死回调累积（内存/CPU 泄漏）。
    fn clear_receivers(&self);
}

/// 通信错误类型
#[derive(Debug, thiserror::Error)]
pub enum CommError {
    #[error("发送失败: {0}")]
    SendError(String),
}
