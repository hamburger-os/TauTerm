//! TauTerm 内建协议插件
//!
//! 每个协议实现保持自包含；`catalog` 是唯一了解完整内建插件集合的 composition 模块。
//! Kernel、AppState 与应用 bootstrap 不维护第二套具体插件清单。

pub mod catalog;
pub mod iperf;
pub mod local_shell;
pub mod modbus;
pub mod network;
pub mod serial;
pub mod ssh;
pub mod telnet;
pub mod tftp;
pub mod trdp;
