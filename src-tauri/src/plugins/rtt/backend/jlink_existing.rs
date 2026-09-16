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

pub struct JlinkExistingRttBackend {
    port: u16,
    streams: BTreeMap<u32, TcpStream>,
    channels: Vec<RttChannelInfo>,
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
                up: Some(RttChannelDirectionInfo { buffer_size: None }),
                down: Some(RttChannelDirectionInfo { buffer_size: None }),
                metadata_complete: false,
            })
            .collect();
        Ok(Self {
            port: config.jlink_port,
            streams,
            channels,
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
        let mut buffer = [0u8; 8 * 1024];
        for (channel_index, stream) in &mut self.streams {
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => {
                        return Err(RttError::new(
                            RttErrorCode::TargetDisconnected,
                            format!("J-Link RTT Channel {channel_index} 连接已关闭"),
                        ));
                    }
                    Ok(count) => output.push(RttReadChunk {
                        channel_index: *channel_index,
                        data: buffer[..count].to_vec(),
                    }),
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
    use super::jlink_telnet_channel_config;

    #[test]
    fn channel_selection_uses_segger_telnet_config_string() {
        assert_eq!(
            jlink_telnet_channel_config(3),
            "$$SEGGER_TELNET_ConfigStr=RTTCh;3$$"
        );
    }
}
