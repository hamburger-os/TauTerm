//! 传输管理器
//!
//! 根据会话类型和通道能力自动选择传输策略。

use crate::channel::Channel;
use crate::kernel::plugin_adapter::TransferProtocolType;

/// 传输策略
#[derive(Debug, Clone, PartialEq)]
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

/// 传输管理器
pub struct TransferManager;

impl TransferManager {
    /// 根据通道能力和协议类型自动选择传输策略
    /// 供未来需要运行时判断 Channel 能力的场景使用。
    #[allow(dead_code)]
    pub fn select_strategy(
        channel: &mut Box<dyn Channel>,
        protocol: &TransferProtocolType,
    ) -> TransferStrategy {
        // 检查通道是否支持端口交出（Inline 策略）
        let supports_handoff = channel.try_handoff().is_some();

        Self::classify_strategy(protocol, supports_handoff)
    }

    /// 仅根据协议类型选择策略（不需要 Channel 实例）
    ///
    /// 用于在传输开始前验证策略选择，不消费 Channel。
    pub fn select_strategy_by_protocol(protocol: &TransferProtocolType) -> TransferStrategy {
        // 串口类协议默认支持 Inline
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
