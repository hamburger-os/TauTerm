//! 传输管理器（预留架构）
//!
//! **当前状态**: 未激活，传输策略路由已硬编码在 `commands.rs::handoff_and_spawn_transfer` 中。
//!
//! **未来用途**: 根据会话类型和通道能力自动选择传输策略：
//! - `Inline` — 串口 X/Y/ZModem（暂停 I/O 循环，直接使用传输通道）
//! - `SideChannel` — SSH SFTP/SCP（在现有会话内打开子通道）
//! - `SeparateConnection` — FTP（建立独立数据连接）
//!
//! 当需要支持非串口传输协议（如 SSH/SFTP/SCP）时，此模块将替代当前的硬编码路由。

use crate::channel::Channel;
use crate::kernel::plugin_adapter::TransferProtocolType;

/// 传输策略（预留）
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]  // 预留，当前未激活
pub enum TransferStrategy {
    /// Inline 策略：暂停 I/O 循环，直接使用传输通道
    /// 适用于串口的 YModem/XModem/ZModem
    Inline,
    /// SideChannel 策略：在现有会话内打开子通道传输
    /// 适用于 SSH 的 SFTP/SCP
    SideChannel,
    /// SeparateConnection 策略：建立独立数据连接
    /// 适用于 FTP
    SeparateConnection,
}

/// 传输管理器（预留）
#[allow(dead_code)]  // 预留，当前未激活
pub struct TransferManager;

#[allow(dead_code)]  // 预留，当前未激活
impl TransferManager {
    /// 根据通道能力和协议类型自动选择传输策略
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
            TransferProtocolType::Sftp | TransferProtocolType::Scp => {
                TransferStrategy::SideChannel
            }
            TransferProtocolType::Ftp => {
                TransferStrategy::SeparateConnection
            }
        }
    }
}
