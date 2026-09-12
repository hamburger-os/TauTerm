from pathlib import Path
import re


def replace(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing: {path}: {old[:100]!r}")
    p.write_text(text.replace(old, new, count))

# Exclusive leases must preserve serial-style timeout semantics so protocol loops remain cancellable.
p = Path("src-tauri/src/transport/runtime.rs")
text = p.read_text()
text = text.replace(
    "const EXCLUSIVE_RX_CAPACITY: usize = 256;\n",
    "const EXCLUSIVE_RX_CAPACITY: usize = 256;\nconst EXCLUSIVE_READ_SLICE: std::time::Duration = std::time::Duration::from_millis(20);\n",
    1,
)
old = """        while self.read_buf.is_empty() {\n            match self.data_rx.recv() {\n                Ok(chunk) => self.read_buf.extend(chunk),\n                Err(_) => return Ok(0),\n            }\n        }\n"""
new = """        while self.read_buf.is_empty() {\n            match self.data_rx.recv_timeout(EXCLUSIVE_READ_SLICE) {\n                Ok(chunk) => self.read_buf.extend(chunk),\n                Err(mpsc::RecvTimeoutError::Timeout) => {\n                    return Err(std::io::Error::new(\n                        std::io::ErrorKind::TimedOut,\n                        \"exclusive data-plane read timed out\",\n                    ));\n                }\n                Err(mpsc::RecvTimeoutError::Disconnected) => {\n                    return Err(std::io::Error::new(\n                        std::io::ErrorKind::UnexpectedEof,\n                        \"transport closed while exclusive lease was active\",\n                    ));\n                }\n            }\n        }\n"""
if old not in text:
    raise SystemExit("runtime exclusive read anchor missing")
text = text.replace(old, new, 1)
# Regression test for non-blocking/cancellable lease reads.
anchor = """    #[test]\n    fn queued_commands_are_not_consumed_by_disconnect_probe() {\n"""
test = """    #[test]\n    fn exclusive_read_times_out_while_transport_is_idle() {\n        let writes = Arc::new(Mutex::new(Vec::new()));\n        let runtime = DataPlaneRuntime::spawn(Box::new(MockStream {\n            reads: VecDeque::new(),\n            writes,\n        }));\n        let mut lease = runtime.handle.acquire_exclusive(\"test-timeout\", false).unwrap();\n        let mut buf = [0u8; 8];\n        let error = lease.read(&mut buf).unwrap_err();\n        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);\n        drop(lease);\n        runtime.join();\n    }\n\n    #[test]\n    fn queued_commands_are_not_consumed_by_disconnect_probe() {\n"""
if anchor not in text:
    raise SystemExit("runtime test anchor missing")
text = text.replace(anchor, test, 1)
p.write_text(text)

# Serial transfer algorithms depend only on byte I/O, never on serialport-specific APIs.
p = Path("src-tauri/src/transfer/protocol.rs")
text = p.read_text()
text = text.replace(
    "use crate::kernel::plugin_adapter::TransferProtocolType;\n",
    "use std::io::{Read, Write};\n\nuse crate::kernel::plugin_adapter::TransferProtocolType;\n",
    1,
)
text = text.replace(
    "/// XModem / YModem / ZModem 的串口协议算法接口。\n",
    """/// Byte-oriented exclusive I/O used by inline transfer protocols.\n/// Keeping this contract at Read + Write + Send prevents X/Y/ZModem from depending on the\n/// concrete serialport handle or transport ownership details.\npub trait TransferIo: Read + Write + Send {}\nimpl<T: Read + Write + Send> TransferIo for T {}\n\n/// XModem / YModem / ZModem 的串口协议算法接口。\n""",
    1,
)
text = text.replace("Box<dyn serialport::SerialPort>", "Box<dyn TransferIo>")
p.write_text(text)

for path in [
    "src-tauri/src/transfer/io.rs",
    "src-tauri/src/transfer/xmodem.rs",
    "src-tauri/src/transfer/ymodem.rs",
    "src-tauri/src/transfer/zmodem.rs",
    "src-tauri/src/transfer/serial_transfer.rs",
]:
    p = Path(path)
    text = p.read_text().replace(
        "Box<dyn serialport::SerialPort>",
        "Box<dyn crate::transfer::protocol::TransferIo>",
    )
    p.write_text(text)

# SerialFileTransfer no longer returns a physical port; dropping it releases ExclusiveIo by RAII.
p = Path("src-tauri/src/transfer/serial_transfer.rs")
text = p.read_text()
text = re.sub(
    r"\n    /// 取出端口（传输完成后归还 I/O 循环）\n    pub fn take_port\(self\).*?\n    }\n",
    "\n",
    text,
    count=1,
    flags=re.S,
)
p.write_text(text)

# Replace old Channel handoff orchestration with a DataPlane exclusive lease.
p = Path("src-tauri/src/transfer/orchestrator.rs")
text = p.read_text()
text = text.replace("use crate::channel::io_loop::IoLoopCmd;\nuse crate::channel::Channel;\n", "", 1)
text = text.replace("                handle.channel_return_tx = None;\n", "", 1)
text = text.replace(
    "/// 启动阶段同步完成端口 handoff，确保返回 ack 时任务真实可执行；协议算法则放入\n/// 后台 task，内部的同步 I/O 继续由 SerialFileTransfer::spawn_blocking 隔离。\n",
    """/// 启动阶段同步获取 Session DataPlane 的 exclusive lease，确保返回 ack 时任务已拥有\n/// 唯一字节流访问权；协议算法放入后台 task，完成后通过 RAII 释放 lease。\n""",
    1,
)
pattern = re.compile(r"\n    fn handoff_port\(.*?\n    fn spawn_cancel_bridge", re.S)
replacement = r'''
    fn acquire_exclusive_io(
        &self,
        app: &AppHandle,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<(
        Box<dyn crate::transfer::protocol::TransferIo>,
        tokio::sync::oneshot::Receiver<()>,
    ), String> {
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let io = {
            let app_state = app.try_state::<AppState>().ok_or("无法获取应用状态")?;
            let mut store = app_state.session_store.lock().map_err(|e| e.to_string())?;
            let not_found = store.session_not_found(session_id);
            let io = {
                let handle = store.get_session(session_id).ok_or(not_found)?;
                if handle.state != SessionState::Connected {
                    return Err("会话未连接".into());
                }
                handle
                    .io
                    .as_ref()
                    .cloned()
                    .ok_or("当前会话没有可独占的数据面")?
            };
            store.reserve_inline_transfer(session_id, transfer_id, cancel_tx)?;
            let not_found = store.session_not_found(session_id);
            let handle = store.get_session_mut(session_id).ok_or(not_found)?;
            handle.state = SessionState::Transferring;
            io
        };

        match io.acquire_exclusive(format!("file-transfer:{transfer_id}"), true) {
            Ok(lease) => Ok((Box::new(lease), cancel_rx)),
            Err(error) => {
                restore_session_state(app, session_id, transfer_id);
                Err(format!("无法获取文件传输独占 I/O: {error}"))
            }
        }
    }

    fn spawn_cancel_bridge'''
text, n = pattern.subn(replacement, text, count=1)
if n != 1:
    raise SystemExit("orchestrator handoff block anchor missing")

# Both send and receive acquire the lease before constructing the protocol adapter.
text = text.replace(
    "let (port, cancel_rx) = self.handoff_port(&app, &ctx.session_id, &transfer_id)?;",
    "let (io, cancel_rx) = self.acquire_exclusive_io(&app, &ctx.session_id, &transfer_id)?;",
)
text = text.replace(
    """            Err(error) => {\n                self.return_port(&app, &ctx.session_id, &transfer_id, port);\n                return Err(error);\n            }\n        };\n\n        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, port);\n""",
    """            Err(error) => {\n                drop(io);\n                restore_session_state(&app, &ctx.session_id, &transfer_id);\n                return Err(error);\n            }\n        };\n\n        let transfer = SerialFileTransfer::new(self.pt.clone(), protocol_handler, io);\n""",
)
# Remove no-longer-needed worker copies.
text = text.replace(
    """        let worker = InlineTransferOrchestrator {\n            pt: self.pt.clone(),\n        };\n\n""",
    "",
)
rollback = """                match transfer.take_port() {\n                    Ok(port) => worker.return_port(&task_app, &task_sid, &task_transfer_id, port),\n                    Err(error) => {\n                        log::error!(\"启动回滚时无法归还端口: {}\", error);\n                        worker.release_after_port_loss(&task_app, &task_sid, &task_transfer_id);\n                    }\n                }\n                return;\n"""
rollback_new = """                drop(transfer);\n                restore_session_state(&task_app, &task_sid, &task_transfer_id);\n                return;\n"""
if rollback not in text:
    raise SystemExit("orchestrator rollback anchor missing")
text = text.replace(rollback, rollback_new)
finish = """            match transfer.take_port() {\n                Ok(port) => worker.return_port(&task_app, &task_sid, &task_transfer_id, port),\n                Err(error) => {\n                    log::error!(\"无法归还端口: {}\", error);\n                    worker.release_after_port_loss(&task_app, &task_sid, &task_transfer_id);\n                }\n            }\n\n            emit_transfer_finished(\n"""
finish_new = """            drop(transfer);\n            restore_session_state(&task_app, &task_sid, &task_transfer_id);\n\n            emit_transfer_finished(\n"""
if finish not in text:
    raise SystemExit("orchestrator finish anchor missing")
text = text.replace(finish, finish_new)

# No legacy handoff/resource-return terms may remain in the inline orchestrator implementation.
for forbidden in ["HandoffPort", "channel_return_tx", "try_handoff", "return_port", "release_after_port_loss", "IoLoopCmd"]:
    if forbidden in text:
        raise SystemExit(f"legacy transfer token still present in orchestrator: {forbidden}")
p.write_text(text)

# Inline transfer algorithms should no longer depend on serialport::SerialPort.
for path in Path("src-tauri/src/transfer").glob("*.rs"):
    if "serialport::SerialPort" in path.read_text():
        raise SystemExit(f"serialport dependency remains in transfer algorithm: {path}")
