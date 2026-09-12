//! 串口文件传输协议算法抽象层
//!
//! `SerialTransferProtocol` 只负责 X/Y/ZModem 在已接管串口上的协议算法；
//! 真正跨传输方式的扩展点是 `kernel::file_transfer::FileTransfer`。
//! 这个命名刻意避免未来新增 SFTP/FTP/WebDAV 时误把串口专用 trait 当成公共接口。

use std::io::{Read, Write};

use crate::kernel::plugin_adapter::TransferProtocolType;
use crate::transfer::types::{BatchFileResult, FileInfo, FileTransferEvent, TransferProgress};

/// Byte-oriented exclusive I/O used by inline transfer protocols.
/// Keeping this contract at Read + Write + Send prevents X/Y/ZModem from depending on the
/// concrete serialport handle or transport ownership details.
pub trait TransferIo: Read + Write + Send {}
impl<T: Read + Write + Send> TransferIo for T {}

/// XModem / YModem / ZModem 的串口协议算法接口。
pub trait SerialTransferProtocol: Send + Sync {
    fn send_files(
        &self,
        port: &mut Box<dyn TransferIo>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;

    fn receive_files(
        &self,
        port: &mut Box<dyn TransferIo>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;
}

/// 创建串口内联协议算法处理器。
pub fn create_protocol(
    protocol_type: &TransferProtocolType,
) -> Option<Box<dyn SerialTransferProtocol>> {
    match protocol_type.as_str() {
        "ymodem" => Some(Box::new(crate::transfer::ymodem::YModem::default())),
        "xmodem" => Some(Box::new(crate::transfer::xmodem::XModem)),
        "zmodem" => Some(Box::new(crate::transfer::zmodem::ZModem::default())),
        _ => None,
    }
}
