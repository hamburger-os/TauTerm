//! 串口模块
//!
//! 提供串口配置类型。
//!
//! **注意**: `manager` 模块已废弃，会话管理已迁移至 `session::manager::SessionManager`。

pub mod config;

/// 旧串口管理器（已废弃，由 `session::manager::SessionManager` 替代）
#[deprecated(since = "0.2.0", note = "请使用 session::manager::SessionManager")]
pub mod manager;
