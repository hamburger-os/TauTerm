//! 终端会话抽象模块
//!
//! 定义 `SessionImpl` 枚举，统一管理不同连接类型的终端会话。
//! 所有连接类型（串口、SSH、Telnet 等）均作为枚举变体，
//! 使前端和命令层与具体协议解耦。

pub mod serial;
pub mod manager;

use serde::{Deserialize, Serialize};
use serial::SerialSession;

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

    /// 人类可读标签（前端 i18n 覆盖此值）
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionType::Serial => "Serial",
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
    /// 会话已连接但正在进行文件传输（串口由传输代码临时持有）
    Transferring,
}

/// I/O 统计快照（协议无关）
///
/// 由 I/O 线程实时更新，StatsCollector 定期采集并推送至前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub tab_id: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connected_at: Option<u64>,
}

/// 会话实现枚举
///
/// 使用具体枚举替代 trait 对象，无 vtable 开销，变体扩展明确。
/// 未来添加 SSH/Telnet 时只需增加变体。
pub enum SessionImpl {
    Serial(SerialSession),
    // Ssh(SshSession),     // 未来版本
    // Telnet(TelnetSession), // 未来版本
}

impl SessionImpl {
    /// 获取会话状态
    pub fn state(&self) -> SessionState {
        match self {
            SessionImpl::Serial(s) => s.state(),
        }
    }

    /// 获取连接类型
    pub fn connection_type(&self) -> ConnectionType {
        match self {
            SessionImpl::Serial(_) => ConnectionType::Serial,
        }
    }
}
