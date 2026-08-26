//! TauTerm 内建协议插件
//!
//! 每个插件实现 `ProtocolAdapter` trait 并注册到 Plugin Host。
//! 未来可支持动态加载第三方插件。

pub mod iperf;
pub mod network;
pub mod serial;
pub mod ssh;
pub mod telnet;
pub mod tftp;
