//! 虚拟串口模块
//!
//! 创建虚拟端口对，实现 TauTerm 与外部串口工具的双向数据桥接。
//!
//! ## 平台支持
//! - Windows: com0com 内核驱动 → 真正的 COM 端口对
//! - Linux: socat 用户态 PTY → 虚拟终端对
//! - macOS: 尚未实现（可通过 Homebrew 安装 socat 后使用 Linux 路径）

pub mod backend;
#[cfg(target_os = "windows")]
pub mod manager;
pub mod bridge;

#[cfg(not(target_os = "windows"))]
pub mod socat;
