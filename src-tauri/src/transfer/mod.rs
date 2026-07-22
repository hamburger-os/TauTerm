//! 文件传输模块
//!
//! 多策略传输架构：Inline / SideChannel / SeparateConnection
//!
//! ## 模块结构
//!
//! - `types` — 公共类型定义（TransferProgress, BatchFileResult, FileTransferEvent, FileInfo）
//! - `protocol` — TransferProtocol trait 和协议工厂
//! - `crc` — CRC-16/CCITT, CRC-32, 校验和计算
//! - `io` — 共享 I/O 工具（超时读取、缓冲区刷新、CAN 发送）
//! - `xmodem` — XModem 协议实现（标准/CRC/1K 变体）
//! - `ymodem` — YModem 协议实现（对齐 lrzsz 标准）
//! - `zmodem` — ZModem 协议实现（帧编码、滑动窗口、断点续传）
//! - `serial_transfer` — SerialFileTransfer 适配器：旧 TransferProtocol → 新 FileTransfer
//! - `sftp_transfer` — SftpFileTransfer 适配器：ssh_file_service 自由函数 → FileTransfer
//! - `orchestrator` — TransferOrchestrator trait + 策略处理器（Inline / SideChannel）
//! - `panic_guard` — RAII 守卫确保 SideChannel 传输 panic 时清理会话状态
//! - `manager` — TransferManager 传输策略选择

pub mod crc;
pub mod io;
pub mod manager;
pub mod orchestrator;
pub mod panic_guard;
pub mod protocol;
pub mod serial_transfer;
pub mod sftp_transfer;
pub mod ssh_file_service;
pub mod types;
pub mod xmodem;
pub mod ymodem;
pub mod zmodem;


