//! 传输管理器
//!
//! 根据会话类型和通道能力自动选择传输策略：
//! - `Inline` — 串口 X/Y/ZModem（暂停 I/O 循环，直接使用传输通道）
//! - `SideChannel` — SSH SFTP（通过 `SshSideChannel` 在现有会话内操作）
//! - `SeparateConnection` — FTP（建立独立数据连接）
//!
//! 由 `commands.rs` 中的发送/接收路径调用，统一替代此前硬编码的 serial/ssh 分支路由。

use crate::channel::Channel;
use crate::kernel::plugin_adapter::TransferProtocolType;

/// 传输策略 — 控制命令层选用哪种传输执行路径
#[derive(Debug, Clone, PartialEq)]
pub enum TransferStrategy {
    /// Inline 策略：暂停 I/O 循环，直接使用传输通道
    /// 适用于串口的 YModem/XModem/ZModem
    Inline,
    /// SideChannel 策略：在现有会话内通过协议侧通道传输
    /// 适用于 SSH 的 SFTP
    SideChannel,
    /// SeparateConnection 策略：建立独立数据连接
    /// 适用于 FTP
    SeparateConnection,
}

/// 传输管理器 — 协议 → 策略路由器
pub struct TransferManager;

impl TransferManager {
    /// 根据通道能力和协议类型选择传输策略（预留：调用方当前使用 select_strategy_by_protocol）
    #[allow(dead_code)]
    pub fn select_strategy(
        channel: &mut Box<dyn Channel>,
        protocol: &TransferProtocolType,
    ) -> TransferStrategy {
        let supports_handoff = channel.try_handoff().is_some();
        Self::classify_strategy(protocol, supports_handoff)
    }

    /// 仅根据协议类型选择策略（不需要 Channel 实例）
    pub fn select_strategy_by_protocol(protocol: &TransferProtocolType) -> TransferStrategy {
        let default_handoff = matches!(
            protocol,
            TransferProtocolType::YModem
                | TransferProtocolType::XModem
                | TransferProtocolType::ZModem
        );
        Self::classify_strategy(protocol, default_handoff)
    }

    fn classify_strategy(protocol: &TransferProtocolType, supports_handoff: bool) -> TransferStrategy {
        match protocol {
            TransferProtocolType::YModem
            | TransferProtocolType::XModem
            | TransferProtocolType::ZModem => {
                if supports_handoff {
                    TransferStrategy::Inline
                } else {
                    TransferStrategy::SideChannel
                }
            }
            TransferProtocolType::Sftp => {
                TransferStrategy::SideChannel
            }
            TransferProtocolType::Ftp => {
                TransferStrategy::SeparateConnection
            }
        }
    }
}
