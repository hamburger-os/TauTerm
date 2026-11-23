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
//!
//! **展示链路不丢字节**：丢块会打断 xterm 正在解析的 VT 序列（可能丢掉半个
//! CSI/OSC），让屏幕进入永久错位状态，比"慢一拍"严重得多。因此队列满时
//! `push` 阻塞提交方（该会话的接收 pump 线程）而不是丢块。
//!
//! 注意背压的边界：pump 只是 DataPlane 的一个订阅者，Transport actor 发布时
//! 仍然是 `try_send`（不阻塞物理读侧）。持续堵住超过订阅队列容量时，订阅者会
//! 因 `BacklogExceeded` 被摘除，表现为该会话**显式断开**——仍然不是静默丢块，
//! 但也不是"一直慢下去"。

use crate::session::RuntimeIdentity;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const BATCH_WINDOW_MS: u64 = 16;
const BATCH_FLUSH_THRESHOLD: usize = 32 * 1024;
/// 提交方被展示队列堵住的告警阈值。出现即说明 UI 消费明显跟不上 PTY 输出。
const SLOW_PUSH_WARN: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct BatchedData {
    pub identity: RuntimeIdentity,
    pub data_b64: String,
}

enum BatchCmd {
    Push(RuntimeIdentity, Vec<u8>),
    Shutdown,
}

pub struct DataBatcher {
    tx: mpsc::SyncSender<BatchCmd>,
}

impl DataBatcher {
    pub fn new<F>(emit_fn: F) -> Self
    where
        F: Fn(BatchedData) + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<BatchCmd>(512);

        thread::Builder::new()
            .name("data-batcher".into())
            .spawn(move || {
                Self::run(rx, emit_fn);
            })
            .expect("failed to spawn data-batcher thread");

        Self { tx }
    }

    /// 入队显示数据。队列满时阻塞，而不是像以前的 `try_send` 那样丢块。
    ///
    /// 唯一会丢弃的情形是 batcher 线程已经不在了（会话已关闭）：此时没有消费者，
    /// 丢弃是唯一可能的收尾行为，且它发生在会话生命周期结束之后。
    pub fn push(&self, session_id: RuntimeIdentity, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }
        let started = Instant::now();
        if self.tx.send(BatchCmd::Push(session_id, data)).is_err() {
            // batcher 线程已随会话断开而退出；没有消费者时丢弃是唯一可能的收尾行为。
            return;
        }
        let waited = started.elapsed();
        if waited > SLOW_PUSH_WARN {
            log::warn!("DataBatcher: presentation queue stalled the session pump for {waited:?}");
        }
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(BatchCmd::Shutdown);
    }

    fn run<F>(rx: mpsc::Receiver<BatchCmd>, emit_fn: F)
    where
        F: Fn(BatchedData),
    {
        struct Pending {
            buf: Vec<u8>,
            window_start: Option<Instant>,
        }

        let mut pending_map: std::collections::HashMap<RuntimeIdentity, Pending> =
            std::collections::HashMap::new();

        let window = Duration::from_millis(BATCH_WINDOW_MS);
        let check_interval = Duration::from_millis(2);

        loop {
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

                    if entry.buf.len() >= BATCH_FLUSH_THRESHOLD {
                        let buf = std::mem::take(&mut entry.buf);
                        emit_fn(BatchedData {
                            identity: session_id.clone(),
                            data_b64: base64_encode(&buf),
                        });
                        pending_map.remove(&session_id);
                    }
                }
                Ok(BatchCmd::Shutdown) => {
                    for (sid, p) in pending_map.drain() {
                        if !p.buf.is_empty() {
                            emit_fn(BatchedData {
                                identity: sid,
                                data_b64: base64_encode(&p.buf),
                            });
                        }
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    let expired: Vec<RuntimeIdentity> = pending_map
                        .iter()
                        .filter_map(|(sid, p)| {
                            p.window_start.and_then(|s| {
                                if s + window <= now {
                                    Some(sid.clone())
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    for sid in expired {
                        if let Some(p) = pending_map.remove(&sid) {
                            if !p.buf.is_empty() {
                                emit_fn(BatchedData {
                                    identity: sid,
                                    data_b64: base64_encode(&p.buf),
                                });
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    for (sid, p) in pending_map.drain() {
                        if !p.buf.is_empty() {
                            emit_fn(BatchedData {
                                identity: sid,
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

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);

    let (chunks, rem) = input.as_chunks::<3>();
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = chunk[1] as u32;
        let b2 = chunk[2] as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize]);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize]);
        out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize]);
        out.push(B64_ALPHABET[(n & 0x3F) as usize]);
    }

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
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_batcher_aggregates_within_window() {
        let (emit_tx, emit_rx) = mpsc::channel();
        let batcher = DataBatcher::new(move |batch| {
            emit_tx
                .send(batch)
                .expect("test receiver should stay alive");
        });

        batcher.push(
            RuntimeIdentity {
                session_id: "s1".into(),
                runtime_id: crate::session::RuntimeId(1),
            },
            b"hello".to_vec(),
        );
        batcher.push(
            RuntimeIdentity {
                session_id: "s1".into(),
                runtime_id: crate::session::RuntimeId(1),
            },
            b" ".to_vec(),
        );
        batcher.push(
            RuntimeIdentity {
                session_id: "s1".into(),
                runtime_id: crate::session::RuntimeId(1),
            },
            b"world".to_vec(),
        );

        let got = emit_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("batch should be emitted within timeout");
        assert_eq!(got.identity.session_id, "s1");
        assert_eq!(got.data_b64, base64_encode(b"hello world"));

        assert!(
            emit_rx
                .recv_timeout(Duration::from_millis(BATCH_WINDOW_MS * 2))
                .is_err(),
            "the three packets should aggregate into exactly one emit"
        );
    }
}
