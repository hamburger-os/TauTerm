//! 虚拟串口模块
//!
//! 创建 TauTerm 与外部串口工具之间的双向虚拟端点。
//!
//! ## 平台支持
//! - Windows: com0com 内核驱动；TauTerm 持有内部 bridge 端，只向 UI/外部工具暴露 external 端
//! - Linux: 进程内 POSIX PTY；TauTerm 持有 master，只向外暴露 slave
//! - macOS: 进程内 POSIX PTY；TauTerm 持有 master，只向外暴露 slave
//!
//! 上层只消费统一的 `VirtualPortBackend` 能力，不依赖平台端口对实现细节。

pub mod backend;
pub mod bridge;
#[cfg(target_os = "windows")]
pub mod manager;
#[cfg(not(target_os = "windows"))]
pub mod pty;
#[cfg(target_os = "windows")]
pub mod service_backend;
