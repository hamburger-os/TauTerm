//! 虚拟串口模块
//!
//! 创建 TauTerm 与外部串口工具之间的双向虚拟端点。
//!
//! ## 平台支持
//! - Windows: com0com 内核驱动 → 真正的虚拟 COM 端口对
//! - Linux: 进程内 POSIX PTY → TauTerm 持有 master，对外暴露 slave
//! - macOS: 进程内 POSIX PTY → TauTerm 持有 master，对外暴露 slave
//!
//! Unix 平台不再依赖 `socat`、Homebrew、系统 PATH 或 `/tmp` 符号链接。

pub mod backend;
pub mod bridge;
#[cfg(target_os = "windows")]
pub mod manager;
#[cfg(target_os = "windows")]
pub mod service_backend;

// Historical module name kept temporarily for source compatibility; its
// implementation is now the native in-process PTY backend.
#[cfg(not(target_os = "windows"))]
pub mod socat;
