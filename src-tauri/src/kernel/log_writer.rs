//! 日志文件写入器
//!
//! 管理单个日志文件的 BufWriter、格式化、自动分卷和安全的句柄关闭。
//! 配合 LogEngine 的消费者线程使用，每个活跃的日志会话对应一个 LogWriter。

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use chrono::Local;

use super::log_engine::{DataDirection, DataLogEntry};

/// 单个日志文件的写入器
pub struct LogWriter {
    file: Option<BufWriter<File>>,
    current_path: PathBuf,
    bytes_written: u64,
    split_threshold: u64,
    session_name: String,
    port_name: String,
    start_time: chrono::DateTime<Local>,
    split_index: u32,
    data_mode: String,
    base_dir: PathBuf,
    buffer_size: usize,
}

impl LogWriter {
    /// 创建新的 LogWriter 并写入文件头
    pub fn new(
        log_dir: &PathBuf,
        split_threshold: u64,
        buffer_size: usize,
        session_name: &str,
        port_name: &str,
        data_mode: &str,
    ) -> std::io::Result<Self> {
        let sanitized_name = Self::sanitize(session_name);
        let sanitized_port = Self::sanitize(port_name);
        let now = Local::now();
        let ts = now.format("%Y%m%d_%H%M%S");
        let filename = format!("Log_{}_{}_{}.log", sanitized_name, sanitized_port, ts);

        std::fs::create_dir_all(log_dir)?;
        let path = log_dir.join(&filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let mut buf_writer = BufWriter::with_capacity(buffer_size, file);

        // 写入文件头
        let header = format!(
            "════ TauTerm Session Log ════\n\
             Session: {}\n\
             Port: {}\n\
             Started: {}\n\
             Data Mode: {}\n\
             ═══════════════════════════════\n\n",
            session_name,
            port_name,
            now.format("%Y-%m-%d %H:%M:%S"),
            data_mode
        );
        let header_bytes = header.as_bytes();
        buf_writer.write_all(header_bytes)?;
        buf_writer.flush()?;

        Ok(Self {
            file: Some(buf_writer),
            current_path: path,
            bytes_written: header_bytes.len() as u64,
            split_threshold,
            session_name: sanitized_name,
            port_name: sanitized_port,
            start_time: now,
            split_index: 0,
            data_mode: data_mode.to_string(),
            base_dir: log_dir.clone(),
            buffer_size,
        })
    }

    /// 写入一条数据日志条目
    ///
    /// 根据 data_mode 格式化行，检查分卷阈值，写入文件缓冲区。
    /// 注意：BufWriter 不会每次调用都立即写入磁盘 — flush 由消费者线程的定时器/缓冲区策略控制。
    pub fn write_entry(&mut self, entry: &DataLogEntry) -> std::io::Result<()> {
        let line = self.format_entry(entry);
        let line_bytes = line.as_bytes();

        // 检查是否需要分卷
        if self.bytes_written + line_bytes.len() as u64 > self.split_threshold {
            self.rotate_file()?;
        }

        if let Some(ref mut f) = self.file {
            f.write_all(line_bytes)?;
            self.bytes_written += line_bytes.len() as u64;
        }
        Ok(())
    }

    /// 强制刷新缓冲区到磁盘
    pub fn flush(&mut self) -> std::io::Result<()> {
        if let Some(ref mut f) = self.file {
            f.flush()?;
        }
        Ok(())
    }

    /// 获取已写入字节数（用于状态上报）
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// 获取当前文件路径的文件名部分
    pub fn file_name(&self) -> String {
        self.current_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.log".into())
    }

    /// 清除日志文件后重新打开：关闭旧句柄，创建带递增序号的新文件
    pub fn reopen(&mut self) -> std::io::Result<()> {
        self.rotate_file()
    }

    // ── 内部方法 ──

    /// 根据数据模式格式化日志行
    fn format_entry(&self, entry: &DataLogEntry) -> String {
        let ts = entry.timestamp.format("%H:%M:%S%.3f");
        let dir = match entry.direction {
            DataDirection::TX => "[TX]",
            DataDirection::RX => "[RX]",
        };

        match self.data_mode.as_str() {
            "text" => {
                // 所见即所得：原始字节转为文本
                let text = String::from_utf8_lossy(&entry.payload);
                format!("{} {} {}\n", ts, dir, text)
            }
            "hex" => {
                // 经典 hexdump 格式：偏移 + HEX + ASCII
                let mut result = format!("{} {}\n", ts, dir);
                for (i, chunk) in entry.payload.chunks(16).enumerate() {
                    result.push_str(&format!("{:08X}  ", i * 16));
                    let hex_part: Vec<String> = chunk
                        .iter()
                        .enumerate()
                        .map(|(j, b)| {
                            let byte_str = format!("{:02X}", b);
                            // 在第 8 字节后添加额外空格分隔
                            if j == 7 {
                                format!("{} ", byte_str)
                            } else {
                                byte_str
                            }
                        })
                        .collect();
                    let hex_line = hex_part.join(" ");
                    result.push_str(&format!("{:<49}", hex_line));
                    result.push_str(" |");
                    for b in chunk {
                        if b.is_ascii_graphic() || *b == b' ' {
                            result.push(*b as char);
                        } else {
                            result.push('.');
                        }
                    }
                    result.push_str("|\n");
                }
                result
            }
            "dual" => {
                // Dual 模式：ASCII 文本 + HEX 双栏
                let text: String = entry.payload.iter().map(|&b| {
                    match b {
                        b'\r' => '␍',
                        b'\n' => '␊',
                        b'\t' => '␉',
                        _ if b < 0x20 => '·',
                        _ => b as char,
                    }
                }).collect();
                let hex_part: Vec<String> = entry
                    .payload
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect();
                let hex_str = hex_part.join(" ");
                format!("{} {} {}  |  {}\n", ts, dir, text, hex_str)
            }
            _ => {
                // 未知模式，回退为原始文本
                let text = String::from_utf8_lossy(&entry.payload);
                format!("{} {} {}\n", ts, dir, text)
            }
        }
    }

    /// 分卷：关闭当前文件，创建带递增序号的新文件
    fn rotate_file(&mut self) -> std::io::Result<()> {
        // 先 flush 旧文件
        if let Some(mut f) = self.file.take() {
            f.flush()?;
            // BufWriter 的 Drop 会再次 flush，但我们显式调用以确保
        }

        self.split_index += 1;
        let ts = self.start_time.format("%Y%m%d_%H%M%S");
        let new_filename = format!(
            "Log_{}_{}_{}_{}.log",
            self.session_name, self.port_name, ts, self.split_index
        );
        let new_path = self.base_dir.join(&new_filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&new_path)?;
        self.file = Some(BufWriter::with_capacity(self.buffer_size, file));
        self.current_path = new_path;
        self.bytes_written = 0;

        Ok(())
    }

    /// 文件名安全化：替换文件系统非法字符为下划线
    fn sanitize(name: &str) -> String {
        name.chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect()
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        // BufWriter 的 Drop 会自动 flush 并关闭底层文件句柄
        // 但为了明确，我们显式 flush
        if let Some(ref mut f) = self.file {
            let _ = f.flush();
        }
    }
}
