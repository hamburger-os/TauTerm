//! 文件传输模块
//!
//! 多策略传输架构：Inline / SideChannel / SeparateConnection

pub mod manager;
pub mod ymodem;

pub use manager::TransferManager;
