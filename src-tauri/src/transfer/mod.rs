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
//! - `manager` — TransferManager 传输策略选择

pub mod crc;
pub mod io;
pub mod manager;
pub mod protocol;
pub mod ssh_file_service;
pub mod types;
pub mod xmodem;
pub mod ymodem;
pub mod zmodem;


