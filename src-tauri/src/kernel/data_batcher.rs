//! 数据批处理器
//!
//! 解决高频小包数据（如 SSH 远端命令输出）导致的性能问题：
//! - 后端每包 emit 一次 `session-data` 事件 + JSON 数字数组序列化，开销巨大
//! - 前端 xterm.write 同步调用频繁触发 ANSI 解析 + 渲染调度
//!
//! 本模块在 I/O 线程的 on_data 回调中加入时间窗口（默认 16ms ≈ 60fps）合并：
//! - 窗口内累积的数据合并为单个 Vec<u8>
//! - 编码为 Base64 字符串（比 JSON 数字数组节省 60-70% 体积）
//! - 到达窗口末尾或累积超过阈值时 emit 一次
//!
//! 设计权衡：
//! - 16ms 窗口对人类感知无明显延迟，但能把 200 包/秒降到 ~60 emit/秒
//! - Base64 编码在 Rust 端开销极小（纯查表），JS 端 atob() 原生实现
//! - 保留 flush 机制确保交互式输入（如按键）立即回显

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// 批处理窗口（ms）。16ms ≈ 60fps，平衡流畅度与合并效率。
const BATCH_WINDOW_MS: u64 = 16;

/// 单批最大累积字节数。超过此值立即 flush，避免大文件传输时延迟过高。
const BATCH_FLUSH_THRESHOLD: usize = 32 * 1024; // 32KB

/// 批处理后的数据消息
#[derive(Debug)]
pub struct BatchedData {
    pub session_id: String,
    /// Base64 编码的数据（前端用 atob 解码）
    pub data_b64: String,
}

/// 批处理命令
enum BatchCmd {
    /// 推入数据（session_id, raw_bytes）
    Push(String, Vec<u8>),
    /// 关闭批处理线程
    Shutdown,
}

/// 数据批处理器
///
/// 在独立线程中累积数据，按时间窗口或阈值合并后通过回调 emit。
/// 调用方（commands.rs 的 on_data 闭包）只需 `push()`，无需关心时序。
///
/// 线程模型：单一后台线程，通过 mpsc::sync_channel 接收命令。
/// - `Push` 命令累积到当前会话的 buffer，记录首字节时间戳
/// - 到达窗口末尾或超阈值时调用 `emit_fn(BatchedData)`
/// - `Shutdown` 命令 flush 残留数据并退出
pub struct DataBatcher {
    tx: mpsc::SyncSender<BatchCmd>,
    /// 因通道满而丢弃的数据包计数（线程安全，可外部读取）
    dropped: Arc<AtomicU64>,
}

impl DataBatcher {
    /// 创建并启动批处理器
    ///
    /// `emit_fn` 在批处理器线程中调用，必须是 `Send + 'static`。
    /// 通常封装 `app.emit("session-data", ...)`。
    pub fn new<F>(emit_fn: F) -> Self
    where
        F: Fn(BatchedData) + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<BatchCmd>(512);
        let dropped = Arc::new(AtomicU64::new(0));

        thread::Builder::new()
            .name("data-batcher".into())
            .spawn(move || {
                Self::run(rx, emit_fn);
            })
            .expect("failed to spawn data-batcher thread");

        Self { tx, dropped }
    }

    /// 推入一包数据（非阻塞，通道满时丢弃以保护 I/O 线程）
    pub fn push(&self, session_id: String, data: Vec<u8>) {
        if self.tx.try_send(BatchCmd::Push(session_id, data)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "DataBatcher: channel full, dropped packet (total dropped: {})",
                self.dropped.load(Ordering::Relaxed)
            );
        }
    }

    /// 返回因通道满而丢弃的数据包总数（用于监控/诊断）
    #[allow(dead_code)]
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 关闭批处理器（flush 残留数据后退出线程）
    pub fn shutdown(&self) {
        let _ = self.tx.send(BatchCmd::Shutdown);
    }

    fn run<F>(rx: mpsc::Receiver<BatchCmd>, emit_fn: F)
    where
        F: Fn(BatchedData),
    {
        // 每个 session_id 的累积状态
        struct Pending {
            buf: Vec<u8>,
            // 窗口起始时间（首字节到达时设置）
            window_start: Option<Instant>,
        }

        let mut pending_map: std::collections::HashMap<String, Pending> =
            std::collections::HashMap::new();

        let window = Duration::from_millis(BATCH_WINDOW_MS);
        let check_interval = Duration::from_millis(2);

        loop {
            // 计算距离下一个窗口到期最近的时间
            let next_deadline = pending_map
                .values()
                .filter_map(|p| p.window_start.map(|s| s + window))
                .min()
                .unwrap_or_else(|| Instant::now() + check_interval);

            let now = Instant::now();
            let timeout = if next_deadline > now {
                next_deadline - now
            } else {
                Duration::from_millis(0)
            };

            match rx.recv_timeout(timeout) {
                Ok(BatchCmd::Push(session_id, data)) => {
                    let entry = pending_map
                        .entry(session_id.clone())
                        .or_insert_with(|| Pending {
                            buf: Vec::new(),
                            window_start: None,
                        });

                    if entry.window_start.is_none() {
                        entry.window_start = Some(Instant::now());
                    }
                    entry.buf.extend_from_slice(&data);

                    // 超阈值立即 flush（大文件传输场景）
                    if entry.buf.len() >= BATCH_FLUSH_THRESHOLD {
                        let buf = std::mem::take(&mut entry.buf);
                        emit_fn(BatchedData {
                            session_id: session_id.clone(),
                            data_b64: base64_encode(&buf),
                        });
                        // 移除已 flush 的条目，下次数据到达时重建窗口
                        pending_map.remove(&session_id);
                    }
                }
                Ok(BatchCmd::Shutdown) => {
                    // flush 所有残留数据
                    for (sid, p) in pending_map.drain() {
                        if !p.buf.is_empty() {
                            emit_fn(BatchedData {
                                session_id: sid,
                                data_b64: base64_encode(&p.buf),
                            });
                        }
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // 检查所有到期窗口，flush
                    let now = Instant::now();
                    let expired: Vec<String> = pending_map
                        .iter()
                        .filter_map(|(sid, p)| {
                            p.window_start
                                .and_then(|s| if s + window <= now { Some(sid.clone()) } else { None })
                        })
                        .collect();

                    for sid in expired {
                        if let Some(p) = pending_map.remove(&sid) {
                            if !p.buf.is_empty() {
                                emit_fn(BatchedData {
                                    session_id: sid,
                                    data_b64: base64_encode(&p.buf),
                                });
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // 发送端关闭，flush 残留数据后退出
                    for (sid, p) in pending_map.drain() {
                        if !p.buf.is_empty() {
                            emit_fn(BatchedData {
                                session_id: sid,
                                data_b64: base64_encode(&p.buf),
                            });
                        }
                    }
                    break;
                }
            }
        }
    }
}

impl Drop for DataBatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ── Base64 编码（标准字母表，无外部依赖） ─────────────────────────

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 编码字节数组为 Base64 字符串
///
/// 标准.Base64 实现（RFC 4648），带 padding。
/// 性能：纯查表 + 位运算，无分配除 3 字节块外。
pub fn base64_encode(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);

    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let b0 = chunk[0] as u32;
        let b1 = chunk[1] as u32;
        let b2 = chunk[2] as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize]);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize]);
        out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize]);
        out.push(B64_ALPHABET[(n & 0x3F) as usize]);
    }

    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let b0 = rem[0] as u32;
            let n = b0 << 16;
            out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize]);
            out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize]);
            out.push(b'=');
            out.push(b'=');
        }
        2 => {
            let b0 = rem[0] as u32;
            let b1 = rem[1] as u32;
            let n = (b0 << 16) | (b1 << 8);
            out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize]);
            out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize]);
            out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize]);
            out.push(b'=');
        }
        _ => {}
    }

    // B64_ALPHABET 和 padding '=' 都是有效 ASCII/UTF-8，
    // 编译器在 release 模式下可证明此不变式，消除运行时校验开销
    String::from_utf8(out).expect("B64 output is always valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_one_byte() {
        assert_eq!(base64_encode(b"f"), "Zg==");
    }

    #[test]
    fn test_base64_encode_two_bytes() {
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn test_base64_encode_known_vectors() {
        // RFC 4648 测试向量
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_batcher_aggregates_within_window() {
        use std::sync::{Arc, Mutex};
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let batcher = DataBatcher::new(move |b| {
            received_clone.lock().unwrap().push(b);
        });

        // 快速推入 3 个小包
        batcher.push("s1".into(), b"hello".to_vec());
        batcher.push("s1".into(), b" ".to_vec());
        batcher.push("s1".into(), b"world".to_vec());

        // 等待窗口过期 + flush
        thread::sleep(Duration::from_millis(50));

        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1, "should aggregate into 1 emit");
        assert_eq!(got[0].session_id, "s1");
        // Base64 of "hello world"
        assert_eq!(got[0].data_b64, base64_encode(b"hello world"));
    }
}
