from pathlib import Path
import re

ROOT = Path("src-tauri/src")


def read(rel: str) -> str:
    return (ROOT / rel).read_text()


def write(rel: str, content: str) -> None:
    (ROOT / rel).write_text(content)


def replace_required(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise RuntimeError(f"missing migration anchor: {label}")
    return text.replace(old, new)


# 1. Repair close metadata helper from the previous migration batch.
s = read("transport/runtime.rs")
s = replace_required(
    s,
    """    fn from_driver(\n        kind: TransportErrorKind,\n        reason: impl Into<String>,\n        exit_code: None,\n        signal: None,\n        metadata: StreamCloseMetadata,\n    ) -> Self {""",
    """    fn from_driver(\n        kind: TransportErrorKind,\n        reason: impl Into<String>,\n        metadata: StreamCloseMetadata,\n    ) -> Self {""",
    "TransportCloseInfo::from_driver signature",
)
write("transport/runtime.rs", s)

# 2. Add one explicit protocol/session attachment hook. This keeps session IDs out of transport.
s = read("kernel/plugin_adapter.rs")
if "pub trait SessionAttach" not in s:
    s = replace_required(
        s,
        "pub trait SideChannel: Send + Sync {",
        "pub trait SessionAttach: Send + Sync {\n    fn on_attached(&self, session_id: &str);\n}\n\npub trait SideChannel: Send + Sync {",
        "SessionAttach insertion",
    )
if "pub on_attached:" not in s:
    s = replace_required(
        s,
        "    pub channel_factory: Option<Arc<dyn SessionChannelFactory>>,\n    pub teardown_delay: std::time::Duration,",
        "    pub channel_factory: Option<Arc<dyn SessionChannelFactory>>,\n    pub on_attached: Option<Arc<dyn SessionAttach>>,\n    pub teardown_delay: std::time::Duration,",
        "ProtocolConnection on_attached field",
    )
write("kernel/plugin_adapter.rs", s)

# 3. Migrate headless/custom adapters to the new adapter contract.
for rel in ["plugins/iperf/mod.rs", "plugins/tftp/mod.rs", "plugins/modbus/mod.rs"]:
    s = read(rel)
    s = s.replace("use crate::channel::error::SessionError;", "use crate::session::SessionError;")
    s = s.replace(
        "use crate::channel::{ContentType, IoStrategy};",
        "use crate::kernel::plugin_adapter::ContentType;",
    )
    s = s.replace("use crate::channel::ContentType;", "use crate::kernel::plugin_adapter::ContentType;")
    s = re.sub(
        r"\n\s*fn io_strategy\(&self\) -> IoStrategy \{\n\s*IoStrategy::(?:Sync|Async)\n\s*\}\n",
        "\n",
        s,
    )
    s = s.replace(
        "            channel: None,\n            comm_handle: None,",
        "            data_plane: None,",
    )
    if "on_attached:" not in s:
        s = re.sub(
            r"(\s+channel_factory:\s*[^,]+,\n)(\s+)(teardown_delay:)",
            r"\1\2on_attached: None,\n\2\3",
            s,
        )
    write(rel, s)

# Existing DataPlane adapters need the new attachment field too.
for rel in ["plugins/serial/mod.rs", "plugins/local_shell/mod.rs"]:
    s = read(rel)
    if "on_attached:" not in s:
        s = re.sub(
            r"(\s+channel_factory:\s*[^,]+,\n)(\s+)(teardown_delay:)",
            r"\1\2on_attached: None,\n\2\3",
            s,
        )
    write(rel, s)

# 4. SSH driver: async russh details stay behind transport::AsyncByteStream.
ssh_driver = r'''//! SSH byte-stream driver.
//!
//! russh is asynchronous, but that execution model is confined to the transport layer.
//! Session/runtime code only sees a DataPlane handle.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::plugins::ssh::handler::SshHandler;
use crate::transport::{
    AsyncByteStream, ReadStatus, StreamCloseMetadata, TransportError, TransportErrorKind,
};

pub struct SshDriver {
    channel: russh::Channel<russh::client::Msg>,
    _handle: Arc<russh::client::Handle<SshHandler>>,
    pending: VecDeque<u8>,
    exit_status: Option<u32>,
    exit_signal: Option<String>,
}

impl SshDriver {
    pub fn new(
        channel: russh::Channel<russh::client::Msg>,
        handle: Arc<russh::client::Handle<SshHandler>>,
    ) -> Self {
        Self {
            channel,
            _handle: handle,
            pending: VecDeque::new(),
            exit_status: None,
            exit_signal: None,
        }
    }

    fn drain_pending(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.pending.len());
        for slot in &mut buf[..n] {
            *slot = self.pending.pop_front().expect("pending length checked");
        }
        n
    }

    fn deliver_chunk(&mut self, chunk: &[u8], buf: &mut [u8]) -> ReadStatus {
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        self.pending.extend(&chunk[n..]);
        ReadStatus::Data(n)
    }
}

#[async_trait::async_trait]
impl AsyncByteStream for SshDriver {
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        if buf.is_empty() {
            return Ok(ReadStatus::Idle);
        }
        if !self.pending.is_empty() {
            return Ok(ReadStatus::Data(self.drain_pending(buf)));
        }

        loop {
            match self.channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                    return Ok(self.deliver_chunk(data.as_ref(), buf));
                }
                Some(russh::ChannelMsg::ExtendedData { .. }) => continue,
                Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                    self.exit_status = Some(exit_status);
                }
                Some(russh::ChannelMsg::ExitSignal { signal_name, .. }) => {
                    self.exit_signal = Some(format!("{signal_name:?}"));
                }
                Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                    return Ok(ReadStatus::Eof);
                }
                Some(_) => {}
            }
        }
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.channel.data(data).await.map_err(|error| {
            TransportError::new(TransportErrorKind::Io, "ssh_write", error.to_string())
        })
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    async fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|error| {
                TransportError::new(
                    TransportErrorKind::Io,
                    "ssh_resize_terminal",
                    error.to_string(),
                )
            })
    }

    fn supports_terminal_control(&self) -> bool {
        true
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        StreamCloseMetadata {
            exit_code: self.exit_status,
            signal: self.exit_signal.clone(),
        }
    }
}
'''
write("plugins/ssh/driver.rs", ssh_driver)
old_ssh = ROOT / "channel/ssh_channel.rs"
if old_ssh.exists():
    old_ssh.unlink()

s = read("channel/mod.rs")
s = s.replace("pub mod ssh_channel;\n", "")
write("channel/mod.rs", s)

s = read("plugins/ssh/mod.rs")
if "mod driver;" not in s:
    s = s.replace("pub mod handler;", "mod driver;\npub mod handler;")
s = s.replace("use crate::channel::error::SessionError;", "use crate::session::SessionError;")
s = s.replace("use crate::channel::ssh_channel::SshChannel;\n", "")
s = s.replace(
    "use crate::channel::{ContentType, IoStrategy};",
    "use crate::kernel::plugin_adapter::ContentType;",
)
s = s.replace(
    "    ChannelKind, ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection,\n    SessionChannelFactory, SideChannel, TransferProtocolType,",
    "    ChannelOpenMode, EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionChannelFactory,\n    SideChannel, TransferProtocolType,",
)
if "use crate::transport::{AsyncBridgeDriver, DataPlaneRuntime};" not in s:
    s = s.replace(
        "use handler::SshHandler;",
        "use crate::transport::{AsyncBridgeDriver, DataPlaneRuntime};\nuse driver::SshDriver;\nuse handler::SshHandler;",
    )
s = replace_required(
    s,
    """        Ok(ProtocolConnection {\n            channel: Some(crate::kernel::plugin_adapter::ChannelKind::Async(Box::new(\n                result.channel,\n            ))),\n            comm_handle: None,\n            side_channel: Some(shared.clone()),\n            channel_factory: Some(shared),\n            teardown_delay: self.teardown_delay(),\n        })""",
    """        let bridge = AsyncBridgeDriver::new(Box::new(result.driver))?;\n        Ok(ProtocolConnection {\n            data_plane: Some(DataPlaneRuntime::spawn(Box::new(bridge))),\n            side_channel: Some(shared.clone()),\n            channel_factory: Some(shared),\n            on_attached: None,\n            teardown_delay: self.teardown_delay(),\n        })""",
    "SSH ProtocolConnection",
)
s = s.replace("    channel: SshChannel,", "    driver: SshDriver,")
s = s.replace(
    "    channel: ssh_channel,\n        session: handle,",
    "    driver: ssh_channel,\n        session: handle,",
)
s = s.replace(
    ") -> Result<SshChannel, SessionError> {",
    ") -> Result<SshDriver, SessionError> {",
)
s = s.replace(
    "    let ssh_channel = SshChannel::new(channel, handle);\n\n    Ok(ssh_channel)",
    "    Ok(SshDriver::new(channel, handle))",
)
s = replace_required(
    s,
    """        Ok(ChannelKind::Async(Box::new(\n            open_pty_shell_channel(self.handle()).await?,\n        )))""",
    """        let driver = open_pty_shell_channel(self.handle()).await?;\n        let bridge = AsyncBridgeDriver::new(Box::new(driver))?;\n        Ok(DataPlaneRuntime::spawn(Box::new(bridge)))""",
    "SSH child channel factory",
)
s = re.sub(
    r"\n\s*fn io_strategy\(&self\) -> IoStrategy \{.*?\n\s*\}\n",
    "\n",
    s,
    flags=re.S,
)
write("plugins/ssh/mod.rs", s)

# 5. Telnet driver: retain protocol parsing in plugin, expose BlockingByteStream.
s = read("plugins/telnet/channel.rs")
s = s.replace("//! Telnet 通道实现", "//! Telnet transport driver")
s = s.replace(
    "use crate::channel::{error::ChannelError, Channel};",
    "use crate::transport::{BlockingByteStream, ReadStatus, TransportError, TransportErrorKind};",
)
s = s.replace("TelnetChannel", "TelnetDriver")
start = s.find("impl Channel for TelnetDriver {")
if start < 0:
    raise RuntimeError("missing Telnet Channel impl")
depth = 0
end = None
for i in range(start, len(s)):
    if s[i] == "{":
        depth += 1
    elif s[i] == "}":
        depth -= 1
        if depth == 0:
            end = i + 1
            break
if end is None:
    raise RuntimeError("unbalanced Telnet Channel impl")
transport_impl = r'''impl BlockingByteStream for TelnetDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        match Read::read(self, buf) {
            Ok(0) => Ok(ReadStatus::Idle),
            Ok(n) => Ok(ReadStatus::Data(n)),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(ReadStatus::Eof),
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                Ok(ReadStatus::Idle)
            }
            Err(error) => Err(TransportError::io("telnet_read", error)),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        Write::write_all(self, data).map_err(|error| TransportError::io("telnet_write", error))
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        Write::flush(self).map_err(|error| TransportError::io("telnet_flush", error))
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        self.connected = false;
        Ok(())
    }

    fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
        let w = cols.clamp(1, u16::MAX as u32) as u16;
        let h = rows.clamp(1, u16::MAX as u32) as u16;
        let payload = [
            (w >> 8) as u8,
            (w & 0xFF) as u8,
            (h >> 8) as u8,
            (h & 0xFF) as u8,
        ];
        self.telnet
            .subnegotiate(TelnetOption::NAWS, &payload)
            .map_err(|error| {
                TransportError::new(TransportErrorKind::Io, "telnet_naws", error.to_string())
            })
    }

    fn supports_terminal_control(&self) -> bool {
        true
    }
}
'''
s = s[:start] + transport_impl + s[end:]
write("plugins/telnet/channel.rs", s)

s = read("plugins/telnet/mod.rs")
s = s.replace("use crate::channel::error::SessionError;", "use crate::session::SessionError;")
s = s.replace(
    "use crate::channel::{ContentType, IoStrategy};",
    "use crate::kernel::plugin_adapter::ContentType;",
)
s = s.replace(
    "    ChannelKind, EndpointInfo, ProtocolAdapter, ProtocolConnection, TransferProtocolType,",
    "    EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach, TransferProtocolType,",
)
s = s.replace(
    "use channel::{TelnetChannel, READ_TIMEOUT};",
    "use channel::{TelnetDriver, READ_TIMEOUT};\nuse crate::transport::DataPlaneRuntime;",
)
attach = '''struct TelnetSessionAttach {\n    slot: Arc<Mutex<Option<String>>>,\n}\n\nimpl SessionAttach for TelnetSessionAttach {\n    fn on_attached(&self, session_id: &str) {\n        if let Ok(mut slot) = self.slot.lock() {\n            *slot = Some(session_id.to_string());\n        }\n    }\n}\n\n'''
if "struct TelnetSessionAttach" not in s:
    s = s.replace("// ── Telnet 适配器", attach + "// ── Telnet 适配器")
s = s.replace(
    "let channel = TelnetChannel::new(telnet, probe, on_echo_change, session_id_slot);",
    "let driver = TelnetDriver::new(telnet, probe, on_echo_change, session_id_slot.clone());",
)
s = replace_required(
    s,
    """        Ok(ProtocolConnection {\n            channel: Some(ChannelKind::Sync(Box::new(channel))),\n            comm_handle: None,\n            side_channel: None,\n            channel_factory: None,\n            teardown_delay: self.teardown_delay(),\n        })""",
    """        Ok(ProtocolConnection {\n            data_plane: Some(DataPlaneRuntime::spawn(Box::new(driver))),\n            side_channel: None,\n            channel_factory: None,\n            on_attached: Some(Arc::new(TelnetSessionAttach { slot: session_id_slot })),\n            teardown_delay: self.teardown_delay(),\n        })""",
    "Telnet ProtocolConnection",
)
s = re.sub(
    r"\n\s*fn io_strategy\(&self\) -> IoStrategy \{\n\s*IoStrategy::Sync\n\s*\}\n",
    "\n",
    s,
)
# Keep tests operating directly on std::io Read/Write helpers.
s = s.replace("use crate::channel::Channel;\n", "")
s = s.replace("TelnetChannel", "TelnetDriver")
write("plugins/telnet/mod.rs", s)

# 6. Network adapter-level contract only. Deeper peer migration is a separate batch.
s = read("plugins/network/mod.rs")
s = s.replace("use crate::channel::error::SessionError;", "use crate::session::SessionError;")
s = s.replace(
    "    ChannelKind, ProtocolAdapter, ProtocolConnection, SideChannel,",
    "    ChannelKind, ProtocolAdapter, ProtocolConnection, SideChannel,",
)
s = s.replace(
    "    fn content_type(&self) -> crate::channel::ContentType {\n        crate::channel::ContentType::Custom\n    }",
    "    fn content_type(&self) -> crate::kernel::plugin_adapter::ContentType {\n        crate::kernel::plugin_adapter::ContentType::Custom\n    }",
)
s = re.sub(
    r"\n\s*fn io_strategy\(&self\) -> crate::channel::IoStrategy \{\n\s*crate::channel::IoStrategy::Sync\n\s*\}\n",
    "\n",
    s,
)
# Leave old network write routing alive until the peer batch, but ProtocolConnection no longer stores it.
s = s.replace(
    """        Ok(ProtocolConnection {\n            channel: None,\n            comm_handle: Some(comm_handle),\n            side_channel: Some(side),\n            channel_factory: None,\n            teardown_delay: self.teardown_delay(),\n        })""",
    """        drop(comm_handle);\n        Ok(ProtocolConnection {\n            data_plane: None,\n            side_channel: Some(side),\n            channel_factory: None,\n            on_attached: None,\n            teardown_delay: self.teardown_delay(),\n        })""",
)
write("plugins/network/mod.rs", s)

print("protocol DataPlane migration staged")
