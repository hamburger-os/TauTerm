from __future__ import annotations

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected exactly one occurrence, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new, 1))


def regex_once(path: str, pattern: str, repl: str, *, flags: int = re.S) -> None:
    text = read(path)
    new_text, count = re.subn(pattern, repl, text, count=1, flags=flags)
    if count != 1:
        raise RuntimeError(f"{path}: regex expected exactly one match, found {count}: {pattern[:100]!r}")
    write(path, new_text)


def replace_all_exact(path: str, old: str, new: str, expected: int) -> None:
    text = read(path)
    count = text.count(old)
    if count != expected:
        raise RuntimeError(f"{path}: expected {expected} occurrences, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new))


# 1. Scheduler API caller: remove the obsolete oneshot compatibility parameter completely.
replace_once(
    "src-tauri/src/kernel/session_store.rs",
    '''    pub fn reserve_inline_transfer(\n        &mut self,\n        session_id: &str,\n        transfer_id: &str,\n        cancel_tx: tokio::sync::oneshot::Sender<()>,\n    ) -> Result<(), String> {\n        let not_found = self.session_not_found(session_id);\n        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;\n        handle\n            .transfer_scheduler\n            .reserve_inline(transfer_id, cancel_tx)\n    }''',
    '''    pub fn reserve_inline_transfer(\n        &mut self,\n        session_id: &str,\n        transfer_id: &str,\n    ) -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>, String> {\n        let not_found = self.session_not_found(session_id);\n        let handle = self.sessions.get_mut(session_id).ok_or(not_found)?;\n        handle.transfer_scheduler.reserve_inline(transfer_id)\n    }''',
)

# 2. Command contract: one tagged, role-aware protocolOptions object; no flattened legacy fields.
replace_once(
    "src-tauri/src/commands.rs",
    '''pub struct FileTransferSendRequest {\n    pub session_id: String,\n    pub protocol: String,\n    pub file_paths: Vec<String>,\n    pub remote_dir: Option<String>,\n    pub overwrite_policy: Option<String>,\n    pub block_size: Option<usize>,\n    pub checksum_mode: Option<String>,\n    pub streaming: Option<bool>,\n}''',
    '''pub struct FileTransferSendRequest {\n    pub session_id: String,\n    pub protocol_options: crate::transfer::config::SendProtocolOptions,\n    pub file_paths: Vec<String>,\n    pub remote_dir: Option<String>,\n    pub overwrite_policy: Option<String>,\n}''',
)
replace_once(
    "src-tauri/src/commands.rs",
    '''pub struct FileTransferReceiveRequest {\n    pub session_id: String,\n    pub protocol: String,\n    pub download_dir: String,\n    pub remote_paths: Vec<String>,\n    pub destination_paths: Option<Vec<String>>,\n    pub overwrite_policy: Option<String>,\n    pub block_size: Option<usize>,\n    pub checksum_mode: Option<String>,\n    pub streaming: Option<bool>,\n}''',
    '''pub struct FileTransferReceiveRequest {\n    pub session_id: String,\n    pub protocol_options: crate::transfer::config::ReceiveProtocolOptions,\n    pub download_dir: String,\n    pub remote_paths: Vec<String>,\n    pub destination_paths: Option<Vec<String>>,\n    pub overwrite_policy: Option<String>,\n}''',
)
replace_once(
    "src-tauri/src/commands.rs",
    '''    let FileTransferSendRequest {\n        session_id,\n        protocol,\n        file_paths,\n        remote_dir,\n        overwrite_policy,\n        block_size,\n        checksum_mode,\n        streaming,\n    } = request;''',
    '''    let FileTransferSendRequest {\n        session_id,\n        protocol_options,\n        file_paths,\n        remote_dir,\n        overwrite_policy,\n    } = request;\n    protocol_options.validate()?;\n    let protocol = protocol_options.protocol().to_string();''',
)
replace_once(
    "src-tauri/src/commands.rs",
    '''    let FileTransferReceiveRequest {\n        session_id,\n        protocol,\n        download_dir,\n        remote_paths,\n        destination_paths,\n        overwrite_policy,\n        block_size,\n        checksum_mode,\n        streaming,\n    } = request;''',
    '''    let FileTransferReceiveRequest {\n        session_id,\n        protocol_options,\n        download_dir,\n        remote_paths,\n        destination_paths,\n        overwrite_policy,\n    } = request;\n    let protocol = protocol_options.protocol().to_string();''',
)
replace_all_exact(
    "src-tauri/src/commands.rs",
    '''            block_size,\n            checksum_mode,\n            streaming,''',
    '''            protocol_options,''',
    2,
)

# 3. Orchestrator: construct role-specific protocol handlers and use the scheduler's one cancel token.
replace_once(
    "src-tauri/src/transfer/orchestrator.rs",
    '''use crate::transfer::panic_guard::PanicGuard;\nuse crate::transfer::protocol::SerialTransferProtocol;''',
    '''use crate::transfer::config::{ReceiveProtocolOptions, SendProtocolOptions};\nuse crate::transfer::panic_guard::PanicGuard;\nuse crate::transfer::protocol::SerialTransferProtocol;''',
)
replace_once(
    "src-tauri/src/transfer/orchestrator.rs",
    '''    pub progress_tx: UnboundedSender<UnifiedProgress>,\n    pub progress_rx: UnboundedReceiver<UnifiedProgress>,\n    pub block_size: Option<usize>,\n    pub checksum_mode: Option<String>,\n    pub streaming: Option<bool>,\n}''',
    '''    pub progress_tx: UnboundedSender<UnifiedProgress>,\n    pub progress_rx: UnboundedReceiver<UnifiedProgress>,\n    pub protocol_options: SendProtocolOptions,\n}''',
)
replace_once(
    "src-tauri/src/transfer/orchestrator.rs",
    '''    pub progress_tx: UnboundedSender<UnifiedProgress>,\n    pub progress_rx: UnboundedReceiver<UnifiedProgress>,\n    pub block_size: Option<usize>,\n    pub checksum_mode: Option<String>,\n    pub streaming: Option<bool>,\n}''',
    '''    pub progress_tx: UnboundedSender<UnifiedProgress>,\n    pub progress_rx: UnboundedReceiver<UnifiedProgress>,\n    pub protocol_options: ReceiveProtocolOptions,\n}''',
)
regex_once(
    "src-tauri/src/transfer/orchestrator.rs",
    r'''impl InlineTransferOrchestrator \{.*?\n\}\n\n#\[async_trait\]\nimpl TransferOrchestrator for InlineTransferOrchestrator''',
    '''impl InlineTransferOrchestrator {\n    fn create_send_protocol_handler(\n        &self,\n        options: &SendProtocolOptions,\n    ) -> Result<Box<dyn SerialTransferProtocol>, String> {\n        match options {\n            SendProtocolOptions::Ymodem { block_size } => Ok(Box::new(\n                crate::transfer::ymodem::YModem {\n                    block_size: *block_size,\n                },\n            )),\n            SendProtocolOptions::Xmodem { block_size } => Ok(Box::new(\n                crate::transfer::xmodem::XModem::sender(*block_size),\n            )),\n            SendProtocolOptions::Zmodem {\n                crc_policy,\n                max_block_size,\n            } => Ok(Box::new(crate::transfer::zmodem::ZModem::sender(\n                *crc_policy,\n                *max_block_size,\n            ))),\n            SendProtocolOptions::Sftp => Err("SFTP 不是内联串口协议".into()),\n        }\n    }\n\n    fn create_receive_protocol_handler(\n        &self,\n        options: &ReceiveProtocolOptions,\n    ) -> Result<Box<dyn SerialTransferProtocol>, String> {\n        match options {\n            ReceiveProtocolOptions::Ymodem => {\n                Ok(Box::new(crate::transfer::ymodem::YModem::default()))\n            }\n            ReceiveProtocolOptions::Xmodem { check_mode } => Ok(Box::new(\n                crate::transfer::xmodem::XModem::receiver(*check_mode),\n            )),\n            ReceiveProtocolOptions::Zmodem { crc_capability } => Ok(Box::new(\n                crate::transfer::zmodem::ZModem::receiver(*crc_capability),\n            )),\n            ReceiveProtocolOptions::Sftp => Err("SFTP 不是内联串口协议".into()),\n        }\n    }\n\n    fn acquire_exclusive_io(\n        &self,\n        app: &AppHandle,\n        session_id: &str,\n        transfer_id: &str,\n    ) -> Result<(Box<dyn crate::transfer::protocol::TransferIo>, Arc<AtomicBool>), String> {\n        let (io, cancel) = {\n            let app_state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;\n            let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;\n            let not_found = store.session_not_found(session_id);\n            let io = {\n                let handle = store.get_session(session_id).ok_or(not_found)?;\n                if handle.state != SessionState::Connected {\n                    return Err("会话未连接".into());\n                }\n                handle\n                    .io\n                    .as_ref()\n                    .cloned()\n                    .ok_or("当前会话没有可独占的数据面")?\n            };\n            let cancel = store.reserve_inline_transfer(session_id, transfer_id)?;\n            let not_found = store.session_not_found(session_id);\n            let handle = store.get_session_mut(session_id).ok_or(not_found)?;\n            handle.state = SessionState::Transferring;\n            (io, cancel)\n        };\n\n        match io.acquire_exclusive(format!("file-transfer:{transfer_id}")) {\n            Ok(lease) => Ok((Box::new(lease), cancel)),\n            Err(error) => {\n                restore_session_state(app, session_id, transfer_id);\n                Err(format!("无法获取文件传输独占 I/O: {error}"))\n            }\n        }\n    }\n}\n\n#[async_trait]\nimpl TransferOrchestrator for InlineTransferOrchestrator''',
)
replace_once(
    "src-tauri/src/transfer/orchestrator.rs",
    '''        self.validate_options(ctx.block_size, ctx.checksum_mode.as_deref(), ctx.streaming)?;\n        let transfer_id = uuid::Uuid::new_v4().to_string();\n        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;\n        let protocol_handler = match self.create_protocol_handler(ctx.block_size) {''',
    '''        let transfer_id = uuid::Uuid::new_v4().to_string();\n        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;\n        let protocol_handler = match self.create_send_protocol_handler(&ctx.protocol_options) {''',
)
replace_once(
    "src-tauri/src/transfer/orchestrator.rs",
    '''        self.validate_options(ctx.block_size, ctx.checksum_mode.as_deref(), ctx.streaming)?;\n        let transfer_id = uuid::Uuid::new_v4().to_string();\n        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;\n        let protocol_handler = match self.create_protocol_handler(ctx.block_size) {''',
    '''        let transfer_id = uuid::Uuid::new_v4().to_string();\n        let (io, cancel) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;\n        let protocol_handler = match self.create_receive_protocol_handler(&ctx.protocol_options) {''',
)

# 4. Remove the obsolete generic serial protocol factory; role-aware construction lives in orchestrator.
protocol = read("src-tauri/src/transfer/protocol.rs")
protocol = protocol.replace("use crate::kernel::plugin_adapter::TransferProtocolType;\n", "")
protocol = re.sub(r'''\n/// 创建串口内联协议算法处理器。\npub fn create_protocol\(.*\Z''', "\n", protocol, flags=re.S)
write("src-tauri/src/transfer/protocol.rs", protocol)

# 5. YMODEM receive must preserve prefetched bytes handed over by the transport runtime.
replace_once(
    "src-tauri/src/transfer/ymodem.rs",
    '''    fs::create_dir_all(download_dir)?;\n    io::flush_port_buffer(port);''',
    '''    fs::create_dir_all(download_dir)?;''',
)

# 6. XMODEM: checksum negotiation and packet size are orthogonal protocol dimensions.
x = read("src-tauri/src/transfer/xmodem.rs")
x = x.replace(
    '''//! 支持三种变体：\n//! - Standard: 128B 块 + 1 字节校验和\n//! - CRC: 128B 块 + 2 字节 CRC-16/CCITT\n//! - OneK: 1024B 块 + 2 字节 CRC-16/CCITT\n''',
    '''//! 发送端独立选择 128B / 1K 数据块；接收端通过 NAK / `C` 协商\n//! checksum / CRC16。两者是正交维度，`G` 不代表 XMODEM-1K。\n''',
)
x = x.replace("use crate::transfer::crc::{self, crc16_ccitt_feedthrough_verify, crc16_ccitt_zero_pad};\n", "use crate::transfer::config::XModemReceiveMode;\nuse crate::transfer::crc::{self, crc16_ccitt_feedthrough_verify, crc16_ccitt_zero_pad};\n")
x = x.replace("const G: u8 = 0x47;\n", "")
start = x.index("// ── XModem 变体枚举")
end = x.index("impl SerialTransferProtocol for XModem")
new_model = '''// ── XMODEM 角色与校验模式 ───────────────────────────────\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\nenum XModemCheckMode {\n    Checksum,\n    Crc16,\n}\n\nimpl XModemCheckMode {\n    const fn init_byte(self) -> u8 {\n        match self {\n            Self::Checksum => NAK,\n            Self::Crc16 => C,\n        }\n    }\n}\n\n#[derive(Debug, Clone, Copy)]\nenum XModemRole {\n    Sender { block_size: usize },\n    Receiver { check_mode: XModemReceiveMode },\n}\n\n/// XMODEM 协议处理器。实例在创建时绑定本机角色，避免把发送参数误用于接收。\n#[derive(Debug, Clone, Copy)]\npub struct XModem {\n    role: XModemRole,\n}\n\nimpl XModem {\n    pub fn sender(block_size: usize) -> Self {\n        Self {\n            role: XModemRole::Sender { block_size },\n        }\n    }\n\n    pub fn receiver(check_mode: XModemReceiveMode) -> Self {\n        Self {\n            role: XModemRole::Receiver { check_mode },\n        }\n    }\n}\n\n'''
x = x[:start] + new_model + x[end:]
x = x.replace(
    '''        xmodem_send(port, files, on_progress, on_file_event, cancel)''',
    '''        let XModemRole::Sender { block_size } = self.role else {\n            return Err("XModem 接收器不能执行发送".into());\n        };\n        xmodem_send(port, files, block_size, on_progress, on_file_event, cancel)''',
    1,
)
x = x.replace(
    '''        xmodem_receive(port, download_dir, on_progress, on_file_event, cancel)''',
    '''        let XModemRole::Receiver { check_mode } = self.role else {\n            return Err("XModem 发送器不能执行接收".into());\n        };\n        xmodem_receive(\n            port,\n            download_dir,\n            check_mode,\n            on_progress,\n            on_file_event,\n            cancel,\n        )''',
    1,
)
x = x.replace(
    '''    files: &[FileInfo],\n    on_progress: &dyn Fn(TransferProgress),''',
    '''    files: &[FileInfo],\n    block_size: usize,\n    on_progress: &dyn Fn(TransferProgress),''',
    1,
)
x = x.replace(
    '''    // ── 阶段 1: 等待接收方发送启动字节（getnak）──\n    // 接收方发送 NAK/C/G 表示就绪，同时声明其期望的变体\n    let variant = getnak(port, cancel)?;\n\n    log::info!(\n        "XModem send: variant={:?}, file=\\\"{}\\\", size={}",\n        variant,\n        file_info.name,\n        file_info.size\n    );''',
    '''    // ── 阶段 1: 接收方只通过 NAK/C 协商校验方式；块长由发送端配置。\n    let check_mode = getnak(port, cancel)?;\n\n    log::info!(\n        "XModem send: check={:?}, block_size={}, file=\\\"{}\\\", size={}",\n        check_mode,\n        block_size,\n        file_info.name,\n        file_info.size\n    );''',
)
x = x.replace("    let block_size = variant.block_size();\n", "", 1)
x = x.replace(
    "send_block(port, block_num, &send_buf[..block_size], &variant, cancel)",
    "send_block(port, block_num, &send_buf[..block_size], block_size, check_mode, cancel)",
    1,
)
# Replace getnak and send_block together, up to send_eot.
pattern = re.compile(r'''/// 等待接收方发送 NAK/C/G 启动字节.*?\nfn getnak\(.*?\n\}\n\n/// 发送单个数据块.*?\nfn send_block\(.*?\n\}\n\n/// 发送 EOT''', re.S)
replacement = '''/// 等待接收方发送 NAK/C 启动字节，返回协商的校验方式。\nfn getnak(\n    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,\n    cancel: &mut dyn FnMut() -> bool,\n) -> Result<XModemCheckMode, Box<dyn std::error::Error>> {\n    for retry in 0..INIT_TIMEOUT_SECS {\n        if cancel() {\n            io::send_cancel(port);\n            return Err("传输已取消".into());\n        }\n        match read_byte_with_timeout(port, 1000)? {\n            Some(NAK) => return Ok(XModemCheckMode::Checksum),\n            Some(C) => return Ok(XModemCheckMode::Crc16),\n            Some(CAN) => return Err("接收方取消了传输".into()),\n            Some(other) => log::debug!(\n                "XModem getnak: ignoring unsupported init byte 0x{:02X} (retry {})",\n                other,\n                retry\n            ),\n            None => {}\n        }\n    }\n    Err(format!(\n        "等待 XModem 启动信号超时（{} 秒）。请先在设备终端中执行 XModem 接收命令（如 rx、loadx）。",\n        INIT_TIMEOUT_SECS\n    )\n    .into())\n}\n\n/// 发送单个数据块。块长由发送端选择，校验方式由接收端握手选择。\nfn send_block(\n    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,\n    block_num: u8,\n    data: &[u8],\n    block_size: usize,\n    check_mode: XModemCheckMode,\n    cancel: &mut dyn FnMut() -> bool,\n) -> Result<(), Box<dyn std::error::Error>> {\n    let header_byte = match block_size {\n        BLOCK_SIZE_128 => SOH,\n        BLOCK_SIZE_1K => STX,\n        other => return Err(format!("XModem 不支持 {} 字节块", other).into()),\n    };\n    let trailer_size = match check_mode {\n        XModemCheckMode::Checksum => 1,\n        XModemCheckMode::Crc16 => 2,\n    };\n    let mut packet = Vec::with_capacity(3 + block_size + trailer_size);\n    packet.push(header_byte);\n    packet.push(block_num);\n    packet.push(!block_num);\n    packet.extend_from_slice(data);\n\n    match check_mode {\n        XModemCheckMode::Checksum => {\n            let sum = crc::checksum(data);\n            packet.push((0u8).wrapping_sub(sum));\n        }\n        XModemCheckMode::Crc16 => {\n            let crc = crc16_ccitt_zero_pad(data);\n            packet.push((crc >> 8) as u8);\n            packet.push((crc & 0xFF) as u8);\n        }\n    }\n\n    for retry in 0..MAX_RETRIES {\n        if cancel() {\n            return Err("传输已取消".into());\n        }\n        port.write_all(&packet)?;\n        port.flush()?;\n        match read_byte_with_timeout(port, 3000)? {\n            Some(ACK) => return Ok(()),\n            Some(CAN) => return Err("接收方取消了传输".into()),\n            Some(NAK) | None => {\n                if retry == MAX_RETRIES - 1 {\n                    return Err(format!("块 {} 重试次数耗尽（{} 次）", block_num, MAX_RETRIES).into());\n                }\n            }\n            Some(other) => {\n                if retry == MAX_RETRIES - 1 {\n                    return Err(format!("块 {} 收到意外响应 0x{:02X}，重试耗尽", block_num, other).into());\n                }\n            }\n        }\n    }\n    Err(format!("块 {} 发送失败", block_num).into())\n}\n\n/// 发送 EOT'''
x, count = pattern.subn(replacement, x, count=1)
if count != 1:
    raise RuntimeError(f"xmodem getnak/send_block replacement count={count}")
# Receive role argument.
x = x.replace(
    '''    download_dir: &str,\n    on_progress: &dyn Fn(TransferProgress),''',
    '''    download_dir: &str,\n    configured_check_mode: XModemReceiveMode,\n    on_progress: &dyn Fn(TransferProgress),''',
    1,
)
# Replace handshake section through negotiated log.
rx_handshake = re.compile(r'''    // ── 阶段 1: 启动握手.*?    log::info!\("XModem receive: negotiated variant \{:\?\}", variant\);''', re.S)
rx_new = '''    // ── 阶段 1: 接收方选择校验方式；块长由实际 SOH/STX 帧头决定。\n    let check_modes: &[XModemCheckMode] = match configured_check_mode {\n        XModemReceiveMode::Auto => &[XModemCheckMode::Crc16, XModemCheckMode::Checksum],\n        XModemReceiveMode::Crc16 => &[XModemCheckMode::Crc16],\n        XModemReceiveMode::Checksum => &[XModemCheckMode::Checksum],\n    };\n    let attempts_per_mode = if check_modes.len() > 1 {\n        INIT_TIMEOUT_SECS.div_ceil(check_modes.len() as u32)\n    } else {\n        INIT_TIMEOUT_SECS\n    };\n    let mut negotiated_check: Option<XModemCheckMode> = None;\n    let mut first_block_data: Option<(u8, Vec<u8>)> = None;\n\n    'init: for &check_mode in check_modes {\n        for _ in 0..attempts_per_mode {\n            if cancel() {\n                io::send_cancel(port);\n                return Err("传输已取消".into());\n            }\n            port.write_all(&[check_mode.init_byte()])?;\n            port.flush()?;\n            match read_byte_with_timeout(port, 1000)? {\n                Some(header @ (SOH | STX)) => {\n                    let bnum = read_or_fail(port)?;\n                    let bnum_neg = read_or_fail(port)?;\n                    if bnum != !bnum_neg {\n                        port.write_all(&[NAK])?;\n                        port.flush()?;\n                        continue;\n                    }\n                    let block_size = if header == STX { BLOCK_SIZE_1K } else { BLOCK_SIZE_128 };\n                    let mut data = vec![0u8; block_size];\n                    for byte in &mut data {\n                        *byte = read_or_fail(port)?;\n                    }\n                    let valid = verify_block(port, &data, check_mode)?;\n                    if valid {\n                        port.write_all(&[ACK])?;\n                        port.flush()?;\n                        negotiated_check = Some(check_mode);\n                        first_block_data = Some((bnum, data));\n                        break 'init;\n                    }\n                    port.write_all(&[NAK])?;\n                    port.flush()?;\n                }\n                Some(CAN) => return Err("发送方取消了传输".into()),\n                Some(other) => log::debug!(\n                    "XModem RX: received 0x{:02X} while requesting {:?}",\n                    other,\n                    check_mode\n                ),\n                None => {}\n            }\n        }\n    }\n\n    let check_mode = negotiated_check.ok_or(\n        "无法与发送方建立 XModem 连接。请确认发送方已启动 XModem 发送（如 sx --xmodem、sx -X）。",\n    )?;\n    log::info!("XModem receive: negotiated check mode {:?}", check_mode);'''
x, count = rx_handshake.subn(rx_new, x, count=1)
if count != 1:
    raise RuntimeError(f"xmodem receive handshake replacement count={count}")
# Header selection accepts either valid block size and preserves STX.
x = re.sub(
    r'''                Some\(SOH\) if variant\.block_size\(\) == BLOCK_SIZE_128 => break 'read_header SOH,\n                Some\(STX\) if variant\.block_size\(\) == BLOCK_SIZE_1K => break 'read_header STX,\n                Some\(EOT\) => break 'read_header EOT,.*?                Some\(SOH\) \| Some\(STX\) => \{.*?                    break 'read_header SOH;\n                \}''',
    '''                Some(SOH) => break 'read_header SOH,\n                Some(STX) => break 'read_header STX,\n                Some(EOT) => break 'read_header EOT,''',
    x,
    count=1,
    flags=re.S,
)
# Replace main-loop verification match with helper.
x, count = re.subn(
    r'''        // 读取并验证校验和/CRC\n        let valid = match variant \{.*?\n        \};''',
    '''        // 读取并验证接收方在握手阶段选择的校验方式。\n        let valid = verify_block(port, &data, check_mode)?;''',
    x,
    count=1,
    flags=re.S,
)
if count != 1:
    raise RuntimeError(f"xmodem main verify replacement count={count}")
# Add shared verifier before filename helper.
marker = "/// 生成接收文件名（XMODEM 无元数据，使用时间戳命名）\n"
helper = '''fn verify_block(\n    port: &mut Box<dyn crate::transfer::protocol::TransferIo>,\n    data: &[u8],\n    check_mode: XModemCheckMode,\n) -> Result<bool, Box<dyn std::error::Error>> {\n    Ok(match check_mode {\n        XModemCheckMode::Checksum => {\n            let check = read_or_fail(port)?;\n            crc::checksum_verify(data, check)\n        }\n        XModemCheckMode::Crc16 => {\n            let crc_hi = read_or_fail(port)?;\n            let crc_lo = read_or_fail(port)?;\n            crc16_ccitt_feedthrough_verify(data, crc_hi, crc_lo)\n        }\n    })\n}\n\n'''
if x.count(marker) != 1:
    raise RuntimeError("xmodem filename marker missing")
x = x.replace(marker, helper + marker, 1)
if "XModemVariant" in x or "Some(G)" in x:
    raise RuntimeError("xmodem legacy variant semantics remain")
write("src-tauri/src/transfer/xmodem.rs", x)

# 7. ZMODEM: role-aware CRC policy/capability and real CANFC32 negotiation.
z = read("src-tauri/src/transfer/zmodem.rs")
z = z.replace(
    "use crate::transfer::crc::{crc16_ccitt, crc32_zmodem};\n",
    "use crate::transfer::config::{ZModemCrcPolicy, ZModemReceiveCrcCapability};\nuse crate::transfer::crc::{crc16_ccitt, crc32_zmodem};\n",
)
# Replace the public configuration struct + default with a role-bound model.
z, count = re.subn(
    r'''/// ZMODEM 协议处理器\n#\[derive\(Debug, Clone\)\]\npub struct ZModem \{.*?\n\}\n\nimpl Default for ZModem \{.*?\n\}\n''',
    '''/// ZMODEM 实例在创建时绑定本机角色，发送策略与接收能力不会混用。\n#[derive(Debug, Clone, Copy)]\nenum ZModemRole {\n    Sender {\n        crc_policy: ZModemCrcPolicy,\n        max_block_size: usize,\n    },\n    Receiver {\n        crc_capability: ZModemReceiveCrcCapability,\n    },\n}\n\n#[derive(Debug, Clone, Copy)]\npub struct ZModem {\n    role: ZModemRole,\n}\n\nimpl ZModem {\n    pub fn sender(crc_policy: ZModemCrcPolicy, max_block_size: usize) -> Self {\n        Self {\n            role: ZModemRole::Sender {\n                crc_policy,\n                max_block_size,\n            },\n        }\n    }\n\n    pub fn receiver(crc_capability: ZModemReceiveCrcCapability) -> Self {\n        Self {\n            role: ZModemRole::Receiver { crc_capability },\n        }\n    }\n}\n''',
    z,
    count=1,
    flags=re.S,
)
if count != 1:
    raise RuntimeError(f"zmodem model replacement count={count}")
z = z.replace(
    '''        zmodem_send(\n            port,\n            files,\n            self.use_crc32,\n            self.max_block_size,''',
    '''        let ZModemRole::Sender {\n            crc_policy,\n            max_block_size,\n        } = self.role\n        else {\n            return Err("ZModem 接收器不能执行发送".into());\n        };\n        zmodem_send(\n            port,\n            files,\n            crc_policy,\n            max_block_size,''',
    1,
)
z = z.replace(
    '''        zmodem_receive(\n            port,\n            download_dir,\n            self.use_crc32,''',
    '''        let ZModemRole::Receiver { crc_capability } = self.role else {\n            return Err("ZModem 发送器不能执行接收".into());\n        };\n        zmodem_receive(\n            port,\n            download_dir,\n            crc_capability,''',
    1,
)
z = z.replace(
    '''    files: &[FileInfo],\n    use_crc32: bool,\n    max_block_size: usize,''',
    '''    files: &[FileInfo],\n    crc_policy: ZModemCrcPolicy,\n    max_block_size: usize,''',
    1,
)
# Initial control header is conservatively CRC16 until ZRINIT establishes capability.
z = z.replace(
    '''    let total_files = files.len() as u32;\n\n    // ── 阶段 1: 发送 ZRQINIT，等待 ZRINIT ──''',
    '''    let total_files = files.len() as u32;\n    let mut use_crc32 = false;\n\n    // ── 阶段 1: 发送 ZRQINIT，等待 ZRINIT 并协商 CRC 能力 ──''',
    1,
)
z = z.replace(
    '''            Ok(ZFrame::Header {\n                frame_type: ZRINIT, ..\n            }) => {\n                log::info!("ZMODEM send: received ZRINIT");\n                // Negotiate CRC32: if we can and receiver can, use CRC32\n                // rf[ZF0] & CANFC32 tells us if receiver supports it\n                // We'll use use_crc32 for simplicity\n                break;\n            }''',
    '''            Ok(ZFrame::Header {\n                frame_type: ZRINIT,\n                flags,\n            }) => {\n                let receiver_crc32 = flags[ZF0] & CANFC32 != 0;\n                use_crc32 = match crc_policy {\n                    ZModemCrcPolicy::Auto => receiver_crc32,\n                    ZModemCrcPolicy::Crc16 => false,\n                    ZModemCrcPolicy::Crc32Required if receiver_crc32 => true,\n                    ZModemCrcPolicy::Crc32Required => {\n                        return Err("接收方未声明 CANFC32，无法满足 CRC32 Required 策略".into());\n                    }\n                };\n                log::info!(\n                    "ZMODEM send: received ZRINIT, receiver_crc32={}, negotiated_crc32={}",\n                    receiver_crc32,\n                    use_crc32\n                );\n                break;\n            }''',
    1,
)
z = z.replace(
    '''    download_dir: &str,\n    use_crc32: bool,\n    on_progress: &dyn Fn(TransferProgress),''',
    '''    download_dir: &str,\n    crc_capability: ZModemReceiveCrcCapability,\n    on_progress: &dyn Fn(TransferProgress),''',
    1,
)
z = z.replace(
    '''    fs::create_dir_all(download_dir)?;\n\n    let mut batch_results''',
    '''    fs::create_dir_all(download_dir)?;\n    let use_crc32 = matches!(crc_capability, ZModemReceiveCrcCapability::Auto);\n\n    let mut batch_results''',
    1,
)
if "self.use_crc32" in z or "self.max_block_size" in z:
    raise RuntimeError("zmodem legacy shared role fields remain")
write("src-tauri/src/transfer/zmodem.rs", z)

print("transfer role-aware backend migration complete")
