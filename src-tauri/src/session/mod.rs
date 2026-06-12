//! 终端会话抽象模块
//!
//! 定义 `TermSession` trait，抽象统一的终端连接接口。
//! 所有连接类型（串口、SSH、Telnet 等）均实现此 trait，
//! 使前端和命令层与具体协议解耦。

pub mod serial;

use serde::{Deserialize, Serialize};

/// 会话端点信息（展示给用户的可连接目标）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    /// 端点标识符（如 "COM1"、"192.168.1.1:22"）
    pub name: String,
    /// 人类可读描述
    pub description: String,
    /// 连接类型标识
    pub connection_type: ConnectionType,
}

/// 连接类型枚举
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    Serial,
    Ssh,
    Telnet,
}

impl ConnectionType {
    /// 所有可用连接类型（用于 UI 下拉菜单）
    pub fn all() -> &'static [ConnectionType] {
        &[ConnectionType::Serial, ConnectionType::Ssh, ConnectionType::Telnet]
    }

    /// 人类可读标签
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionType::Serial => "串口 (Serial)",
            ConnectionType::Ssh => "SSH",
            ConnectionType::Telnet => "Telnet",
        }
    }

    /// 是否为已实现的类型
    pub fn is_available(&self) -> bool {
        match self {
            ConnectionType::Serial => true,
            ConnectionType::Ssh => false,  // 未来版本实现
            ConnectionType::Telnet => false, // 未来版本实现
        }
    }
}

/// 会话状态
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
}

/// 终端会话 trait
///
/// 所有连接类型均实现此 trait，提供统一的：
/// - 端点枚举
/// - 连接/断开管理
/// - 数据读写
pub trait TermSession: Send {
    /// 枚举可用端点
    fn enumerate_endpoints(&self) -> Result<Vec<EndpointInfo>, String>;

    /// 连接到指定端点
    fn connect(
        &mut self,
        endpoint: &str,
        params: serde_json::Value,
        on_data: Box<dyn Fn(Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn() + Send>,
    ) -> Result<(), String>;

    /// 断开连接
    fn disconnect(&mut self) -> Result<(), String>;

    /// 写入数据
    fn write(&mut self, data: &[u8]) -> Result<(), String>;

    /// 获取当前状态
    fn state(&self) -> SessionState;

    /// 获取连接类型
    fn connection_type(&self) -> ConnectionType;
}
