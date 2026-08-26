//! 虚拟串口模块
//!
//! 创建虚拟端口对，实现 TauTerm 与外部串口工具的双向数据桥接。
//!
//! ## 平台支持
//! - Windows: com0com 内核驱动 → 真正的 COM 端口对
//! - Linux: socat 用户态 PTY → 虚拟终端对
//! - macOS: socat 用户态 PTY → 虚拟终端对（`brew install socat`，与 Linux 共用同一后端）

pub mod backend;
pub mod bridge;
#[cfg(target_os = "windows")]
pub mod manager;
#[cfg(target_os = "windows")]
pub mod service_backend;

#[cfg(not(target_os = "windows"))]
pub mod socat;
