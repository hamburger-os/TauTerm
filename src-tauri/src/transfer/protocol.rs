//! 传输协议抽象层
//!
//! 定义 `TransferProtocol` trait 统一 X/Y/ZModem 收发接口。
//! 协议工厂通过 `From<TransferProtocolType>` 创建具体协议处理器。

use crate::kernel::plugin_adapter::TransferProtocolType;
use crate::transfer::types::{BatchFileResult, FileInfo, FileTransferEvent, TransferProgress};

/// 文件传输协议 trait
///
/// 统一 XModem、YModem、ZModem 三种协议的收发接口。
/// 所有协议实现者通过此 trait 提供一致的 API。
pub trait TransferProtocol: Send + Sync {
    /// 通过串口发送文件
    ///
    /// # 参数
    /// - `port`: 串口端口（`Box<dyn SerialPort>`）
    /// - `files`: 待发送文件列表（含路径、名称、大小、修改时间）
    /// - `on_progress`: 逐块进度回调
    /// - `on_file_event`: 文件级别事件回调（FileStart / FileComplete）
    /// - `cancel`: 取消检测闭包（返回 `true` 表示用户已取消）
    ///
    /// # 返回
    /// - `Ok(batch_results)`: 包含每个文件的传输结果
    ///   部分文件失败时仍返回 Ok — 调用方通过 `BatchFileResult.status` 判断。
    fn send_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;

    /// 通过串口接收文件
    ///
    /// # 参数
    /// - `port`: 串口端口
    /// - `download_dir`: 下载目录路径
    /// - `on_progress`: 逐块进度回调
    /// - `on_file_event`: 文件级别事件回调（FileStart / FileComplete）
    /// - `cancel`: 取消检测闭包
    ///
    /// # 返回
    /// - `Ok(batch_results)`: 包含每个接收文件的结果
    fn receive_files(
        &self,
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;
}

/// 根据协议类型创建对应的协议处理器
///
/// 工厂函数，返回 `Box<dyn TransferProtocol>` 供命令层使用。
pub fn create_protocol(
    protocol_type: &TransferProtocolType,
) -> Option<Box<dyn TransferProtocol>> {
    match protocol_type {
        TransferProtocolType::YModem => {
            Some(Box::new(crate::transfer::ymodem::YModem::default()))
        }
        TransferProtocolType::XModem => {
            Some(Box::new(crate::transfer::xmodem::XModem::default()))
        }
        TransferProtocolType::ZModem => {
            Some(Box::new(crate::transfer::zmodem::ZModem::default()))
        }
        // SFTP/SCP/FTP not implemented via this factory
        TransferProtocolType::Sftp | TransferProtocolType::Scp | TransferProtocolType::Ftp => None,
    }
}
