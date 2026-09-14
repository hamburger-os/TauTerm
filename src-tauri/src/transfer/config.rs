//! 文件传输协议的角色感知配置。
//!
//! X/Y/ZModem 不是客户端/服务端协议，而是发送方 / 接收方角色协议。
//! 这里只描述本机角色真正有权决定的参数；由对端握手或帧头决定的参数不会作为
//! 可写配置重复暴露。

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum XModemReceiveMode {
    /// 优先请求 CRC16；若发送方长期无响应，再降级为 8-bit checksum。
    Auto,
    /// 始终以 `C` 请求 CRC16。
    Crc16,
    /// 始终以 `NAK` 请求 8-bit checksum。
    Checksum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZModemCrcPolicy {
    /// 与接收方 `ZRINIT.CANFC32` 能力取交集：支持则 CRC32，否则 CRC16。
    Auto,
    /// 始终使用 CRC16，即使接收方支持 CRC32。
    Crc16,
    /// 要求接收方支持 CRC32；不支持时明确失败。
    Crc32Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZModemReceiveCrcCapability {
    /// 在 ZRINIT 中声明 CANFC32，允许发送方协商 CRC32。
    Auto,
    /// 不声明 CANFC32，只接受 CRC16 发送策略。
    Crc16Only,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum SendProtocolOptions {
    Ymodem {
        #[serde(rename = "blockSize")]
        block_size: usize,
    },
    Xmodem {
        #[serde(rename = "blockSize")]
        block_size: usize,
    },
    Zmodem {
        #[serde(rename = "crcPolicy")]
        crc_policy: ZModemCrcPolicy,
        #[serde(rename = "maxBlockSize")]
        max_block_size: usize,
    },
    Sftp,
}

impl SendProtocolOptions {
    pub const fn protocol(&self) -> &'static str {
        match self {
            Self::Ymodem { .. } => "ymodem",
            Self::Xmodem { .. } => "xmodem",
            Self::Zmodem { .. } => "zmodem",
            Self::Sftp => "sftp",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Ymodem { block_size } | Self::Xmodem { block_size } => {
                if matches!(*block_size, 128 | 1024) {
                    Ok(())
                } else {
                    Err(format!(
                        "{} 发送块大小仅支持 128 或 1024，收到 {}",
                        self.protocol(),
                        block_size
                    ))
                }
            }
            Self::Zmodem { max_block_size, .. } => {
                if matches!(*max_block_size, 1024 | 2048 | 4096 | 8192) {
                    Ok(())
                } else {
                    Err(format!(
                        "ZModem 最大数据块仅支持 1024/2048/4096/8192，收到 {}",
                        max_block_size
                    ))
                }
            }
            Self::Sftp => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum ReceiveProtocolOptions {
    Ymodem,
    Xmodem {
        #[serde(rename = "checkMode")]
        check_mode: XModemReceiveMode,
    },
    Zmodem {
        #[serde(rename = "crcCapability")]
        crc_capability: ZModemReceiveCrcCapability,
    },
    Sftp,
}

impl ReceiveProtocolOptions {
    pub const fn protocol(&self) -> &'static str {
        match self {
            Self::Ymodem => "ymodem",
            Self::Xmodem { .. } => "xmodem",
            Self::Zmodem { .. } => "zmodem",
            Self::Sftp => "sftp",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_options_reject_non_protocol_block_sizes() {
        assert!(SendProtocolOptions::Ymodem { block_size: 512 }
            .validate()
            .is_err());
        assert!(SendProtocolOptions::Xmodem { block_size: 512 }
            .validate()
            .is_err());
        assert!(SendProtocolOptions::Zmodem {
            crc_policy: ZModemCrcPolicy::Auto,
            max_block_size: 3072,
        }
        .validate()
        .is_err());
    }
}
