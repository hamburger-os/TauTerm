//! TauTerm 内建协议插件
//!
//! 内建插件在应用 composition root 显式注册到 `PluginRuntime`。通用会话插件实现
//! `ProtocolAdapter`，专属能力通过类型化 contribution 注册；本模块不维护第二套插件目录。

pub mod iperf;
pub mod local_shell;
pub mod modbus;
pub mod network;
pub mod serial;
pub mod ssh;
pub mod telnet;
pub mod tftp;
pub mod trdp;
