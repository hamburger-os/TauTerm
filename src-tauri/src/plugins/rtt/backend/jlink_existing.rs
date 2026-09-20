use super::RttBackend;
use crate::plugins::rtt::config::RttConfig;
use crate::plugins::rtt::error::{RttError, RttErrorCode};
use crate::plugins::rtt::model::{
    RttBackendCapabilities, RttBackendDescriptor, RttChannelDirectionInfo, RttChannelInfo,
    RttReadChunk,
};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

const MAX_CHANNELS_PER_POLL: usize = 4;
const MAX_READS_PER_CHANNEL_PER_POLL: usize = 4;

pub struct JlinkExistingRttBackend {
    port: u16,
    streams: BTreeMap<u32, TcpStream>,
    channels: Vec<RttChannelInfo>,
    poll_cursor: usize,
}

impl JlinkExistingRttBackend {
    pub fn open(config: &RttConfig) -> Result<Self, RttError> {
        let mut streams = BTreeMap::new();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.jlink_port);
        for channel in &config.jlink_channels {
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(1_500))
                .map_err(|error| {
                    RttError::new(
                        RttErrorCode::JlinkServerUnavailable,
                        format!(
                            "无法连接本机 J-Link RTT 服务 127.0.0.1:{}: {error}",
                            config.jlink_port
                        ),
                    )
                })?;
            stream
                .set_write_timeout(Some(Duration::from_millis(1_000)))
                .map_err(|error| RttError::backend(error.to_string()))?;
            stream
                .write_all(jlink_telnet_channel_config(*channel).as_bytes())
                .map_err(|error| {
                    RttError::new(
                        RttErrorCode::JlinkChannelConfigFailed,
                        format!("配置 J-Link RTT Channel {channel} 失败: {error}"),
                    )
                })?;
            stream.flush().map_err(|error| {
                RttError::new(
                    RttErrorCode::JlinkChannelConfigFailed,
                    format!("提交 J-Link RTT Channel {channel} 配置失败: {error}"),
                )
            })?;
            stream
                .set_nonblocking(true)
                .map_err(|error| RttError::backend(error.to_string()))?;
            streams.insert(*channel, stream);
        }
        let channels = config
            .jlink_channels
            .iter()
            .map(|index| RttChannelInfo {
                index: *index,
                name: None,
                up: Some(RttChannelDirectionInfo {
                    buffer_size: None,
                    usable: true,
                    issue: None,
                }),
                down: Some(RttChannelDirectionInfo {
                    buffer_size: None,
                    usable: true,
                    issue: None,
                }),
                metadata_complete: false,
            })
            .collect();
        Ok(Self {
            port: config.jlink_port,
            streams,
            channels,
            poll_cursor: 0,
        })
    }
}

fn jlink_telnet_channel_config(channel: u32) -> String {
    format!("$$SEGGER_TELNET_ConfigStr=RTTCh;{channel}$$")
}

impl RttBackend for JlinkExistingRttBackend {
    fn descriptor(&self) -> RttBackendDescriptor {
        RttBackendDescriptor {
            kind: "jlink_existing".into(),
            display_name: "J-Link Existing".into(),
            target: None,
            probe: Some(format!("127.0.0.1:{}", self.port)),
            control_block_address: None,
            capabilities: RttBackendCapabilities {
                enumerate_channels: false,
                channel_metadata: false,
                locator: false,
                direct_target_control: false,
            },
        }
    }

    fn channels(&self) -> &[RttChannelInfo] {
        &self.channels
    }

    fn poll(&mut self, output: &mut Vec<RttReadChunk>) -> Result<(), RttError> {
        let channel_count = self.channels.len();
        if channel_count == 0 {
            self.poll_cursor = 0;
            return Ok(());
        }

        let start = self.poll_cursor % channel_count;
        let channel_budget = channel_count.min(MAX_CHANNELS_PER_POLL);
        let mut buffer = [0u8; 8 * 1024];

        for offset in 0..channel_budget {
            let position = (start + offset) % channel_count;
            let channel_index = self.channels[position].index;
            let Some(stream) = self.streams.get_mut(&channel_index) else {
                continue;
            };
            for _ in 0..MAX_READS_PER_CHANNEL_PER_POLL {
                match stream.read(&mut buffer) {
                    Ok(0) => {
                        return Err(RttError::new(
                            RttErrorCode::TargetDisconnected,
                            format!("J-Link RTT Channel {channel_index} 连接已关闭"),
                        ));
                    }
                    Ok(count) => {
                        output.push(RttReadChunk {
                            channel_index,
                            data: buffer[..count].to_vec(),
                        });
                        if count < buffer.len() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(error) => {
                        return Err(RttError::new(
                            RttErrorCode::TargetDisconnected,
                            format!("读取 J-Link RTT Channel {channel_index} 失败: {error}"),
                        ));
                    }
                }
            }
        }
        self.poll_cursor = (start + channel_budget) % channel_count;
        Ok(())
    }

    fn write(&mut self, channel_index: u32, data: &[u8]) -> Result<usize, RttError> {
        let stream = self.streams.get_mut(&channel_index).ok_or_else(|| {
            RttError::new(
                RttErrorCode::RttChannelNotFound,
                format!("J-Link RTT Channel {channel_index} 未配置"),
            )
        })?;
        match stream.write(data) {
            Ok(count) => Ok(count),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(0),
            Err(error) if error.kind() == ErrorKind::Interrupted => Ok(0),
            Err(error) => Err(RttError::new(
                RttErrorCode::RttWriteFailed,
                format!("写入 J-Link RTT Channel {channel_index} 失败: {error}"),
            )),
        }
    }

    fn refresh_channels(&mut self) -> Result<Vec<RttChannelInfo>, RttError> {
        Ok(self.channels.clone())
    }

    fn shutdown(&mut self) {
        self.streams.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{jlink_telnet_channel_config, JlinkExistingRttBackend, MAX_CHANNELS_PER_POLL};
    use crate::plugins::rtt::backend::RttBackend;
    use crate::plugins::rtt::model::RttChannelInfo;
    use std::collections::BTreeMap;

    #[test]
    fn channel_selection_uses_segger_telnet_config_string() {
        assert_eq!(
            jlink_telnet_channel_config(3),
            "$$SEGGER_TELNET_ConfigStr=RTTCh;3$$"
        );
    }

    #[test]
    fn empty_backend_poll_is_stable() {
        let mut backend = JlinkExistingRttBackend {
            port: 19_021,
            streams: BTreeMap::new(),
            channels: Vec::<RttChannelInfo>::new(),
            poll_cursor: 99,
        };
        let mut output = Vec::new();
        backend.poll(&mut output).unwrap();
        assert_eq!(backend.poll_cursor, 0);
        assert!(output.is_empty());
        assert_eq!(MAX_CHANNELS_PER_POLL, 4);
    }
}
