//! 虚拟串口模块
//!
//! 通过 com0com 创建虚拟 COM 端口对，实现 TauTerm 与外部串口工具的
//! 双向数据桥接。
//!
//! ## 平台支持
//! - Windows: com0com 内核驱动 → 真正的 COM 端口对
//! - Linux/macOS: 待实现（PTY/socat 方案）

pub mod manager;
// pub mod bridge; — Task 2 添加
