//! Windows 管理员 Local Shell 桥接。
//!
//! GUI 保持普通权限；当前已签名可执行文件以隐藏 helper 模式通过 `runas`
//! 启动，只承载一个 ConPTY。配置/命令与终端事件通过一对随机、逻辑单向命名管道的
//! 结构化帧传输，避免同步管道句柄上的阻塞读写互相等待。

use crate::channel::error::ChannelError;
use crate::channel::local_shell_channel::LocalShellChannel;
use crate::channel::{Channel, DisconnectInfo};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, SetNamedPipeHandleState,
};
use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
const PIPE_TYPE_BYTE: u32 = 0;
const PIPE_READMODE_BYTE: u32 = 0;
const PIPE_WAIT: u32 = 0;
const PIPE_NOWAIT: u32 = 1;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
const ERROR_PIPE_LISTENING: u32 = 536;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const WAIT_OBJECT_0: u32 = 0;
const FRAME_CONFIG: u8 = 1;
const FRAME_DATA: u8 = 2;
const FRAME_EXIT: u8 = 3;
const FRAME_ERROR: u8 = 4;
const FRAME_WRITE: u8 = 10;
const FRAME_RESIZE: u8 = 11;
const FRAME_SHUTDOWN: u8 = 12;
const MAX_FRAME: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_millis(50);
const HELPER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize, Deserialize)]
struct HelperConfig {
    executable: String,
    args: Vec<String>,
    cwd: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResizeRequest {
    cols: u32,
    rows: u32,
}

enum ReaderEvent {
    Data(Vec<u8>),
    Exit(DisconnectInfo),
    Error(String),
}

pub struct ElevatedShellChannel {
    writer: Arc<Mutex<std::fs::File>>,
    reader_rx: mpsc::Receiver<ReaderEvent>,
    pending: VecDeque<u8>,
    running: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<DisconnectInfo>>>,
    helper_process: HANDLE,
    shutdown_started: bool,
}

unsafe impl Send for ElevatedShellChannel {}

impl ElevatedShellChannel {
    pub fn spawn(executable: &str, args: &[String], cwd: &Path) -> Result<Self, ChannelError> {
        let pipe_base = format!(
            r"\\.\pipe\TauTermElevatedShell-{}",
            uuid::Uuid::new_v4().simple()
        );
        // The handles are deliberately duplex-capable even though traffic is
        // logically one-way. SetNamedPipeHandleState needs write-attributes
        // access when switching PIPE_NOWAIT back to PIPE_WAIT after connect.
        let command_pipe = create_server_pipe(&format!("{pipe_base}-commands"), PIPE_ACCESS_DUPLEX)
            .map_err(ChannelError::Io)?;
        let event_pipe =
            match create_server_pipe(&format!("{pipe_base}-events"), PIPE_ACCESS_DUPLEX) {
                Ok(pipe) => pipe,
                Err(error) => {
                    unsafe { CloseHandle(command_pipe) };
                    return Err(ChannelError::Io(error));
                }
            };

        let helper_process = match launch_helper(&pipe_base) {
            Ok(process) => process,
            Err(error) => {
                unsafe {
                    CloseHandle(command_pipe);
                    CloseHandle(event_pipe);
                }
                return Err(ChannelError::Io(error));
            }
        };

        if let Err(error) = wait_for_helper_connection(command_pipe, helper_process)
            .and_then(|_| wait_for_helper_connection(event_pipe, helper_process))
        {
            unsafe {
                TerminateProcess(helper_process, 1);
                CloseHandle(helper_process);
                CloseHandle(command_pipe);
                CloseHandle(event_pipe);
            }
            return Err(ChannelError::Io(error));
        }

        let mut writer = unsafe { std::fs::File::from_raw_handle(command_pipe as RawHandle) };
        let reader = unsafe { std::fs::File::from_raw_handle(event_pipe as RawHandle) };
        let config = HelperConfig {
            executable: executable.to_string(),
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
        };
        let payload = match serde_json::to_vec(&config) {
            Ok(payload) => payload,
            Err(error) => {
                unsafe {
                    TerminateProcess(helper_process, 1);
                    CloseHandle(helper_process);
                }
                return Err(ChannelError::Io(io_other(format!(
                    "管理员 Shell 配置序列化失败: {error}"
                ))));
            }
        };
        if let Err(error) = write_frame(&mut writer, FRAME_CONFIG, &payload) {
            unsafe {
                TerminateProcess(helper_process, 1);
                CloseHandle(helper_process);
            }
            return Err(ChannelError::Io(error));
        }

        let writer = Arc::new(Mutex::new(writer));
        let (reader_tx, reader_rx) = mpsc::sync_channel(64);
        let running = Arc::new(AtomicBool::new(true));
        let running_reader = running.clone();
        let exit = Arc::new(Mutex::new(None));
        let exit_reader = exit.clone();
        std::thread::Builder::new()
            .name("elevated-shell-pipe-reader".into())
            .spawn(move || read_helper_frames(reader, reader_tx, running_reader, exit_reader))?;

        Ok(Self {
            writer,
            reader_rx,
            pending: VecDeque::new(),
            running,
            exit,
            helper_process,
            shutdown_started: false,
        })
    }

    fn send(&self, kind: u8, payload: &[u8]) -> std::io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io_other("管理员 Shell 管道锁已损坏"))?;
        write_frame(&mut *writer, kind, payload)
    }

    fn shutdown_helper(&mut self) {
        if self.shutdown_started {
            return;
        }
        self.shutdown_started = true;
        let _ = self.send(FRAME_SHUTDOWN, &[]);
        if !self.helper_process.is_null() {
            let wait = unsafe { WaitForSingleObject(self.helper_process, 1000) };
            if wait != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(self.helper_process, 1) };
            }
        }
        self.running.store(false, Ordering::SeqCst);
    }
}

fn create_server_pipe(name: &str, access: u32) -> std::io::Result<HANDLE> {
    let name = wide(name);
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            access,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            std::ptr::null(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(pipe)
    }
}

fn wait_for_helper_connection(pipe: HANDLE, helper_process: HANDLE) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + HELPER_CONNECT_TIMEOUT;
    loop {
        let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
        if connected != 0 {
            break;
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_PIPE_CONNECTED {
            break;
        }
        if error != ERROR_PIPE_LISTENING {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        if unsafe { WaitForSingleObject(helper_process, 0) } == WAIT_OBJECT_0 {
            return Err(io_other("管理员 Shell helper 在连接管道前退出"));
        }
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "管理员 Shell helper 连接超时",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
    if unsafe { SetNamedPipeHandleState(pipe, &mode, std::ptr::null_mut(), std::ptr::null_mut()) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

impl Read for ElevatedShellChannel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.pending.is_empty() {
            return Ok(drain_pending(&mut self.pending, buf));
        }
        match self.reader_rx.recv_timeout(READ_TIMEOUT) {
            Ok(ReaderEvent::Data(data)) => {
                self.pending.extend(data);
                Ok(drain_pending(&mut self.pending, buf))
            }
            Ok(ReaderEvent::Exit(info)) => {
                if let Ok(mut slot) = self.exit.lock() {
                    *slot = Some(info);
                }
                self.running.store(false, Ordering::SeqCst);
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "管理员 Shell 已退出",
                ))
            }
            Ok(ReaderEvent::Error(error)) => Err(io_other(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "管理员 Shell 读取超时",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "管理员 Shell 管道已关闭",
            )),
        }
    }
}

impl Write for ElevatedShellChannel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.send(FRAME_WRITE, buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer
            .lock()
            .map_err(|_| io_other("管理员 Shell 管道锁已损坏"))?
            .flush()
    }
}

impl Channel for ElevatedShellChannel {
    fn is_connected(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn set_timeout(&mut self, _dur: Duration) -> Result<(), ChannelError> {
        Ok(())
    }

    fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<(), ChannelError> {
        let payload = serde_json::to_vec(&ResizeRequest { cols, rows })
            .map_err(|error| ChannelError::Io(io_other(error.to_string())))?;
        self.send(FRAME_RESIZE, &payload).map_err(ChannelError::Io)
    }

    fn shutdown(&mut self) -> Result<(), ChannelError> {
        self.shutdown_helper();
        Ok(())
    }

    fn disconnect_info(&self, fallback: DisconnectInfo) -> DisconnectInfo {
        self.exit
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .unwrap_or(fallback)
    }
}

impl Drop for ElevatedShellChannel {
    fn drop(&mut self) {
        self.shutdown_helper();
        if !self.helper_process.is_null() {
            unsafe { CloseHandle(self.helper_process) };
            self.helper_process = std::ptr::null_mut();
        }
    }
}

/// 在 `main` 启动 Tauri 前调用。返回 true 表示当前进程已作为 helper 处理完毕。
pub fn maybe_run_helper() -> bool {
    let mut args = std::env::args_os();
    let _exe = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--tauterm-elevated-shell-helper")) {
        return false;
    }
    let result = args
        .next()
        .ok_or_else(|| "缺少管理员 Shell 管道名".to_string())
        .and_then(|pipe| run_helper(&pipe.to_string_lossy()).map_err(|error| error.to_string()));
    if let Err(error) = result {
        log::error!("管理员 Shell helper 失败: {error}");
    }
    true
}

fn run_helper(pipe_base: &str) -> std::io::Result<()> {
    let command_name = wide(&format!("{pipe_base}-commands"));
    let command_handle = unsafe {
        CreateFileW(
            command_name.as_ptr(),
            GENERIC_READ,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if command_handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let event_name = wide(&format!("{pipe_base}-events"));
    let event_handle = unsafe {
        CreateFileW(
            event_name.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if event_handle == INVALID_HANDLE_VALUE {
        unsafe { CloseHandle(command_handle) };
        return Err(std::io::Error::last_os_error());
    }
    let mut command_pipe = unsafe { std::fs::File::from_raw_handle(command_handle as RawHandle) };
    let mut event_pipe = unsafe { std::fs::File::from_raw_handle(event_handle as RawHandle) };
    let (kind, payload) = read_frame(&mut command_pipe)?;
    if kind != FRAME_CONFIG {
        return Err(io_other("管理员 Shell helper 收到无效握手"));
    }
    let config: HelperConfig = serde_json::from_slice(&payload)
        .map_err(|error| io_other(format!("管理员 Shell 配置无效: {error}")))?;
    validate_helper_config(&config)?;

    let mut shell = LocalShellChannel::spawn(&config.executable, &config.args, &config.cwd)
        .map_err(|error| io_other(error.to_string()))?;
    let (command_tx, command_rx) = mpsc::sync_channel(64);
    std::thread::Builder::new()
        .name("elevated-shell-command-reader".into())
        .spawn(move || {
            while let Ok(frame) = read_frame(&mut command_pipe) {
                if command_tx.send(frame).is_err() {
                    break;
                }
            }
        })?;

    let mut buffer = [0u8; 8192];
    loop {
        while let Ok((kind, payload)) = command_rx.try_recv() {
            match kind {
                FRAME_WRITE => {
                    shell.write_all(&payload)?;
                    shell.flush()?;
                }
                FRAME_RESIZE => {
                    let resize: ResizeRequest = serde_json::from_slice(&payload)
                        .map_err(|error| io_other(error.to_string()))?;
                    shell
                        .resize_pty(resize.cols, resize.rows)
                        .map_err(|error| io_other(error.to_string()))?;
                }
                FRAME_SHUTDOWN => {
                    let _ = shell.shutdown();
                    return Ok(());
                }
                _ => return Err(io_other("管理员 Shell helper 收到未知命令")),
            }
        }

        match shell.read(&mut buffer) {
            Ok(0) => {}
            Ok(count) => write_frame(&mut event_pipe, FRAME_DATA, &buffer[..count])?,
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => {
                let info = shell.disconnect_info(DisconnectInfo::io_error(error.to_string()));
                let payload = serde_json::to_vec(&info)
                    .map_err(|serialize_error| io_other(serialize_error.to_string()))?;
                write_frame(&mut event_pipe, FRAME_EXIT, &payload)?;
                return Ok(());
            }
        }
    }
}

fn validate_helper_config(config: &HelperConfig) -> std::io::Result<()> {
    if config.executable.contains('\0')
        || config.args.len() > 64
        || config
            .args
            .iter()
            .any(|arg| arg.contains('\0') || arg.len() > 4096)
        || !config.cwd.is_dir()
    {
        return Err(io_other("管理员 Shell helper 配置校验失败"));
    }
    Ok(())
}

fn launch_helper(pipe_name: &str) -> std::io::Result<HANDLE> {
    let executable = std::env::current_exe()?;
    let verb = wide("runas");
    let file = wide(&executable.to_string_lossy());
    let params = wide(&format!(
        "--tauterm-elevated-shell-helper \"{}\"",
        pipe_name
    ));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_CANCELLED {
            return Err(io_other("User cancelled the UAC elevation prompt"));
        }
        return Err(io_other(format!("管理员 helper 启动失败 (Win32 {error})")));
    }
    if info.hProcess.is_null() {
        return Err(io_other("管理员 helper 未返回进程句柄"));
    }
    Ok(info.hProcess)
}

fn read_helper_frames(
    mut reader: std::fs::File,
    tx: mpsc::SyncSender<ReaderEvent>,
    running: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<DisconnectInfo>>>,
) {
    loop {
        match read_frame(&mut reader) {
            Ok((FRAME_DATA, payload)) => {
                if tx.send(ReaderEvent::Data(payload)).is_err() {
                    break;
                }
            }
            Ok((FRAME_EXIT, payload)) => {
                let info = serde_json::from_slice::<DisconnectInfo>(&payload)
                    .unwrap_or_else(|error| DisconnectInfo::io_error(error.to_string()));
                if let Ok(mut slot) = exit.lock() {
                    *slot = Some(info.clone());
                }
                let _ = tx.send(ReaderEvent::Exit(info));
                break;
            }
            Ok((FRAME_ERROR, payload)) => {
                let _ = tx.send(ReaderEvent::Error(String::from_utf8_lossy(&payload).into()));
                break;
            }
            Ok(_) => {
                let _ = tx.send(ReaderEvent::Error("管理员 Shell 管道帧无效".into()));
                break;
            }
            Err(error) => {
                let _ = tx.send(ReaderEvent::Error(error.to_string()));
                break;
            }
        }
    }
    running.store(false, Ordering::SeqCst);
}

fn write_frame(writer: &mut impl Write, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io_other("管理员 Shell 管道帧过大"));
    }
    writer.write_all(&[kind])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn read_frame(reader: &mut impl Read) -> std::io::Result<(u8, Vec<u8>)> {
    let mut kind = [0u8; 1];
    let mut len = [0u8; 4];
    reader.read_exact(&mut kind)?;
    reader.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io_other("管理员 Shell 管道帧过大"));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok((kind[0], payload))
}

fn drain_pending(pending: &mut VecDeque<u8>, buf: &mut [u8]) -> usize {
    let count = pending.len().min(buf.len());
    for slot in &mut buf[..count] {
        *slot = pending.pop_front().expect("pending length checked");
    }
    count
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn io_other(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_pipe_protocol_round_trips_binary_payloads() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, FRAME_WRITE, &[0, 1, 2, 255]).unwrap();
        let (kind, payload) = read_frame(&mut std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(kind, FRAME_WRITE);
        assert_eq!(payload, [0, 1, 2, 255]);
    }

    #[test]
    fn helper_rejects_unbounded_or_nul_arguments() {
        let config = HelperConfig {
            executable: "pwsh.exe".into(),
            args: vec!["bad\0argument".into()],
            cwd: std::env::current_dir().unwrap(),
        };
        assert!(validate_helper_config(&config).is_err());
    }

    #[test]
    fn production_one_way_pipes_switch_back_to_blocking_after_connect() {
        for (suffix, server_access, client_access) in [
            ("commands", PIPE_ACCESS_DUPLEX, GENERIC_READ),
            ("events", PIPE_ACCESS_DUPLEX, GENERIC_WRITE),
        ] {
            let pipe_name = format!(
                r"\\.\pipe\TauTermElevatedShellModeTest-{}-{suffix}",
                uuid::Uuid::new_v4().simple()
            );
            let server = create_server_pipe(&pipe_name, server_access).unwrap();
            let client_name = pipe_name.clone();
            let (client_tx, client_rx) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let name = wide(&client_name);
                let client = unsafe {
                    CreateFileW(
                        name.as_ptr(),
                        client_access,
                        0,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        std::ptr::null_mut(),
                    )
                };
                let _ = client_tx.send(client as usize);
            });

            let result = wait_for_helper_connection(server, unsafe {
                windows_sys::Win32::System::Threading::GetCurrentProcess()
            });
            let client = client_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("test client did not connect") as HANDLE;
            assert_ne!(client, INVALID_HANDLE_VALUE);
            unsafe {
                CloseHandle(client);
                CloseHandle(server);
            }
            result.expect("production pipe must become blocking without access denied");
        }
    }

    #[test]
    fn helper_streams_prompt_input_and_stops_on_first_shutdown() {
        let pipe_base = format!(
            r"\\.\pipe\TauTermElevatedShellTest-{}",
            uuid::Uuid::new_v4().simple()
        );
        let create_test_pipe = |name: &str, access: u32| {
            let pipe_wide = wide(name);
            unsafe {
                CreateNamedPipeW(
                    pipe_wide.as_ptr(),
                    access,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    64 * 1024,
                    64 * 1024,
                    0,
                    std::ptr::null(),
                )
            }
        };
        let command_pipe = create_test_pipe(&format!("{pipe_base}-commands"), PIPE_ACCESS_DUPLEX);
        let event_pipe = create_test_pipe(&format!("{pipe_base}-events"), PIPE_ACCESS_DUPLEX);
        assert_ne!(command_pipe, INVALID_HANDLE_VALUE);
        assert_ne!(event_pipe, INVALID_HANDLE_VALUE);

        let (helper_done_tx, helper_done_rx) = mpsc::sync_channel(1);
        let helper_pipe_name = pipe_base.clone();
        std::thread::spawn(move || {
            let result = run_helper(&helper_pipe_name);
            let _ = helper_done_tx.send(result.map_err(|error| error.to_string()));
        });

        for pipe in [command_pipe, event_pipe] {
            let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
            assert!(connected != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED);
        }
        let mut command_server =
            unsafe { std::fs::File::from_raw_handle(command_pipe as RawHandle) };
        let mut event_server = unsafe { std::fs::File::from_raw_handle(event_pipe as RawHandle) };

        let config = HelperConfig {
            executable: "cmd.exe".into(),
            args: vec!["/D".into(), "/Q".into(), "/K".into()],
            cwd: std::env::current_dir().unwrap(),
        };
        write_frame(
            &mut command_server,
            FRAME_CONFIG,
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();

        let (frame_tx, frame_rx) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            while let Ok(frame) = read_frame(&mut event_server) {
                if frame_tx.send(frame).is_err() {
                    break;
                }
            }
        });

        let wait_for_text = |needle: &[u8]| {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut output = Vec::new();
            let mut terminal_frame = None;
            while std::time::Instant::now() < deadline {
                if let Ok((kind, payload)) = frame_rx.recv_timeout(Duration::from_millis(100)) {
                    match kind {
                        FRAME_DATA => {
                            output.extend(payload);
                            if output.windows(needle.len()).any(|window| window == needle) {
                                return;
                            }
                        }
                        FRAME_EXIT | FRAME_ERROR => {
                            terminal_frame = Some((kind, payload));
                            break;
                        }
                        _ => {}
                    }
                }
            }
            panic!(
                "helper output did not contain {}; output={:?}; terminal_frame={:?}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(&output),
                terminal_frame
            );
        };

        write_frame(&mut command_server, FRAME_WRITE, b"echo TAUTERM_READY\r\n").unwrap();
        // ConPTY asks the terminal for its cursor position before presenting the
        // first prompt. xterm answers this automatically in production.
        write_frame(&mut command_server, FRAME_WRITE, b"\x1b[1;1R").unwrap();
        wait_for_text(b"TAUTERM_READY");
        write_frame(
            &mut command_server,
            FRAME_WRITE,
            b"echo TAUTERM_ROUNDTRIP\r\n",
        )
        .unwrap();
        wait_for_text(b"TAUTERM_ROUNDTRIP");
        write_frame(&mut command_server, FRAME_SHUTDOWN, &[]).unwrap();
        assert!(helper_done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("helper did not stop after the first shutdown frame")
            .is_ok());
    }
}
