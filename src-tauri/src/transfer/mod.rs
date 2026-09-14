//! 文件传输模块
//!
//! 多策略传输架构：Inline / Auxiliary / SeparateConnection
//!
//! ## 模块结构
//!
//! - `config` — 发送方 / 接收方角色感知协议配置
//! - `types` — 公共类型定义（TransferProgress, BatchFileResult, FileTransferEvent, FileInfo）
//! - `protocol` — 串口 `SerialTransferProtocol` trait 与 `TransferIo` 契约
//! - `crc` — CRC-16/CCITT, CRC-32, 校验和计算
//! - `io` — 共享 I/O 工具（超时读取、协议同步、CAN 发送）
//! - `xmodem` — XModem 协议实现（Checksum / CRC 与 128B / 1K 正交建模）
//! - `ymodem` — YModem 协议实现（对齐 lrzsz 标准）
//! - `zmodem` — ZModem 协议实现（帧编码、能力协商、自适应块大小）
//! - `serial_transfer` — SerialFileTransfer 适配器：串口 SerialTransferProtocol → 通用 FileTransfer
//! - `sftp_transfer` — SftpFileTransfer 适配器：ssh_file_service 自由函数 → FileTransfer
//! - `orchestrator` — 传输策略的唯一解析与生命周期编排入口（Inline / Auxiliary）
//! - `panic_guard` — RAII 守卫确保 Auxiliary 传输 panic 时清理会话状态
//! - `scheduler` — Session 级传输准入、任务身份与取消信号的单一所有者

pub mod config;
pub mod crc;
pub mod io;
pub mod orchestrator;
pub mod panic_guard;
pub mod protocol;
pub mod scheduler;
pub mod serial_transfer;
pub mod sftp_transfer;
pub mod ssh_file_service;
pub mod types;
pub mod xmodem;
pub mod ymodem;
pub mod zmodem;
