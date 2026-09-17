//! Session data log segment writer.
//!
//! A `LogWriter` owns exactly one active immutable segment at a time. Rotation never rewrites an
//! existing segment: the current handle is flushed/closed and a fresh self-describing segment is
//! created. This keeps high-volume logging crash-safe and makes every rotated file independently
//! understandable.

use chrono::{Local, SecondsFormat};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::log_engine::{DataDirection, DataLogEntry};
use super::log_filename::session_segment_file_name;

pub struct LogWriter {
    file: Option<BufWriter<File>>,
    current_path: PathBuf,
    bytes_written: u64,
    split_threshold: u64,
    session_id: String,
    session_name: String,
    endpoint: String,
    start_time: chrono::DateTime<Local>,
    file_nonce: String,
    split_index: u32,
    data_mode: String,
    base_dir: PathBuf,
    buffer_size: usize,
}

impl LogWriter {
    pub fn new(
        log_dir: &Path,
        split_threshold: u64,
        buffer_size: usize,
        session_id: &str,
        session_name: &str,
        endpoint: &str,
        data_mode: &str,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let now = Local::now();
        let file_nonce = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let mut writer = Self {
            file: None,
            current_path: PathBuf::new(),
            bytes_written: 0,
            split_threshold,
            session_id: session_id.to_string(),
            session_name: session_name.to_string(),
            endpoint: endpoint.to_string(),
            start_time: now,
            file_nonce,
            split_index: 0,
            data_mode: data_mode.to_string(),
            base_dir: log_dir.to_path_buf(),
            buffer_size,
        };
        writer.open_segment()?;
        Ok(writer)
    }

    pub fn write_entry(&mut self, entry: &DataLogEntry) -> std::io::Result<()> {
        let line = self.format_entry(entry);
        let line_bytes = line.as_bytes();

        if self.bytes_written > 0
            && self.bytes_written + line_bytes.len() as u64 > self.split_threshold
        {
            self.rotate_file()?;
        }

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("session log writer is not open"))?;
        file.write_all(line_bytes)?;
        self.bytes_written += line_bytes.len() as u64;
        Ok(())
    }

    /// Apply runtime storage settings. A buffer-capacity change is materialized by rotating to a
    /// new segment because `BufWriter` cannot safely change capacity in place.
    pub fn reconfigure(
        &mut self,
        split_threshold: u64,
        buffer_size: usize,
    ) -> std::io::Result<bool> {
        self.split_threshold = split_threshold;
        if self.buffer_size == buffer_size {
            return Ok(false);
        }
        self.buffer_size = buffer_size;
        self.rotate_file()?;
        Ok(true)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok(())
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn file_name(&self) -> String {
        self.current_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown.log".to_string())
    }

    pub fn current_path(&self) -> &Path {
        &self.current_path
    }

    /// Re-open after an explicit clear operation. The deleted segment is never recreated under the
    /// same name; the sequence always advances.
    pub fn reopen(&mut self) -> std::io::Result<()> {
        self.rotate_file()
    }

    fn format_entry(&self, entry: &DataLogEntry) -> String {
        let ts = entry
            .timestamp
            .to_rfc3339_opts(SecondsFormat::Millis, false);
        let dir = match entry.direction {
            DataDirection::TX => "[TX]",
            DataDirection::RX => "[RX]",
        };

        match self.data_mode.as_str() {
            "text" => {
                let text = crate::kernel::charset::decode_to_utf8(&entry.payload, &entry.encoding)
                    .unwrap_or_else(|| String::from_utf8_lossy(&entry.payload).into_owned());
                format!("{ts} {dir} {text}\n")
            }
            "hex" => {
                let mut result = format!("{ts} {dir}\n");
                for (index, chunk) in entry.payload.chunks(16).enumerate() {
                    result.push_str(&format!("{:08X}  ", index * 16));
                    let hex_line = chunk
                        .iter()
                        .enumerate()
                        .map(|(byte_index, byte)| {
                            if byte_index == 7 {
                                format!("{byte:02X} ")
                            } else {
                                format!("{byte:02X}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    result.push_str(&format!("{hex_line:<49}"));
                    result.push_str(" |");
                    for byte in chunk {
                        result.push(if byte.is_ascii_graphic() || *byte == b' ' {
                            *byte as char
                        } else {
                            '.'
                        });
                    }
                    result.push_str("|\n");
                }
                result
            }
            "dual" => {
                let text: String = entry
                    .payload
                    .iter()
                    .map(|byte| match *byte {
                        b'\r' => '␍',
                        b'\n' => '␊',
                        b'\t' => '␉',
                        value if value < 0x20 || value == 0x7f => '·',
                        value if value.is_ascii() => value as char,
                        _ => '·',
                    })
                    .collect();
                let hex = entry
                    .payload
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{ts} {dir} {text}  |  {hex}\n")
            }
            _ => {
                let text = crate::kernel::charset::decode_to_utf8(&entry.payload, &entry.encoding)
                    .unwrap_or_else(|| String::from_utf8_lossy(&entry.payload).into_owned());
                format!("{ts} {dir} {text}\n")
            }
        }
    }

    fn rotate_file(&mut self) -> std::io::Result<()> {
        self.close()?;
        self.split_index = self.split_index.saturating_add(1);
        self.open_segment()
    }

    fn open_segment(&mut self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        let segment_created = Local::now();
        let session_key = Self::short_file_key(&self.session_id);
        let file_name = session_segment_file_name(
            &segment_created,
            &session_key,
            &self.file_nonce,
            self.split_index,
        );
        let path = self.base_dir.join(file_name);
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let mut buffered = BufWriter::with_capacity(self.buffer_size, file);
        let header = format!(
            "════ TauTerm Session Log ════\n\
             Session ID: {}\n\
             Session: {}\n\
             Endpoint: {}\n\
             Started: {}\n\
             Segment Created: {}\n\
             Data Mode: {}\n\
             Segment: {}\n\
             TauTerm: {}\n\
             ═══════════════════════════════\n\n",
            self.session_id,
            self.session_name,
            self.endpoint,
            self.start_time
                .to_rfc3339_opts(SecondsFormat::Millis, false),
            segment_created.to_rfc3339_opts(SecondsFormat::Millis, false),
            self.data_mode,
            self.split_index,
            env!("CARGO_PKG_VERSION")
        );
        buffered.write_all(header.as_bytes())?;
        buffered.flush()?;

        self.current_path = path;
        self.bytes_written = header.len() as u64;
        self.file = Some(buffered);
        Ok(())
    }

    fn short_file_key(value: &str) -> String {
        let filtered: String = value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .take(12)
            .collect();
        if filtered.is_empty() {
            "session".to_string()
        } else {
            filtered
        }
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::log_engine::{DataDirection, DataLogEntry};

    fn entry(payload: &[u8]) -> DataLogEntry {
        DataLogEntry {
            session_id: "session-123".to_string(),
            direction: DataDirection::RX,
            data_mode: "text".to_string(),
            encoding: "utf-8".to_string(),
            payload: payload.to_vec(),
            timestamp: Local::now(),
        }
    }

    #[test]
    fn every_rotated_segment_is_self_describing() {
        let temp = tempfile::tempdir().unwrap();
        let mut writer = LogWriter::new(
            temp.path(),
            1024 * 1024,
            1024,
            "session-123",
            "Serial test",
            "COM200",
            "text",
        )
        .unwrap();

        let first = writer.current_path().to_path_buf();
        assert!(writer.reconfigure(1024 * 1024, 2048).unwrap());
        writer.write_entry(&entry(b"rotated")).unwrap();
        writer.flush().unwrap();
        let second = writer.current_path().to_path_buf();

        assert_ne!(first, second);
        assert!(first.file_name().unwrap() < second.file_name().unwrap());
        let content = std::fs::read_to_string(second).unwrap();
        assert!(content.contains("TauTerm Session Log"));
        assert!(content.contains("Session ID: session-123"));
        assert!(content.contains("Segment Created:"));
        assert!(content.contains("Segment: 1"));
        assert!(content.contains("rotated"));
    }

    #[test]
    fn filenames_use_chronological_prefix_and_session_identity_not_user_names() {
        let temp = tempfile::tempdir().unwrap();
        let writer = LogWriter::new(
            temp.path(),
            1024 * 1024,
            1024,
            "2f09e8f4-778b-41e0-a7d0",
            "../../unsafe:name",
            "COM1",
            "text",
        )
        .unwrap();
        let name = writer.file_name();
        assert!(name.starts_with("TauTerm_"));
        assert!(name.contains("_session_2f09e8f4778b_p"));
        assert!(!name.contains("unsafe"));
        assert!(!name.contains(".."));
    }
}
