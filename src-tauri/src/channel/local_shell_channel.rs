//! Local Shell PTY 通道。
//!
//! 将 `portable-pty` 的阻塞 reader 隐藏在专用线程后，通过有界 channel
//! 向通用同步 I/O loop 暴露可超时的 `Read`。这样 PTY 无输出时仍能及时
//! 处理键盘写入、窗口 resize 与 Shutdown。

use crate::channel::error::ChannelError;
use crate::channel::{Channel, DisconnectInfo};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const GRACEFUL_EXIT_TIMEOUT: Duration = Duration::from_millis(350);
const FORCE_EXIT_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
struct ProcessExit {
    code: u32,
    signal: Option<String>,
}

enum ReaderEvent {
    Data(Vec<u8>),
    Closed,
    Error(String),
}

/// 本地 Shell 的同步 PTY 通道。
pub struct LocalShellChannel {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    reader_rx: mpsc::Receiver<ReaderEvent>,
    pending: VecDeque<u8>,
    running: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<ProcessExit>>>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    shutdown_started: bool,
    #[cfg(unix)]
    process_group: Option<libc::pid_t>,
    #[cfg(windows)]
    job: Option<WindowsJob>,
}

impl LocalShellChannel {
    /// 创建 PTY 并在其中启动一个独立 argv 的本地进程。
    pub fn spawn(
        executable: &str,
        args: &[String],
        cwd: &std::path::Path,
    ) -> Result<Self, ChannelError> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(|e| io_other(format!("PTY creation failed: {e}")))?;

        let mut command = CommandBuilder::new(executable);
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| io_other(format!("Shell process failed to start: {e}")))?;
        let process_id = child.process_id();
        let killer = Arc::new(Mutex::new(child.clone_killer()));

        #[cfg(windows)]
        let job = child
            .as_raw_handle()
            .and_then(|handle| match WindowsJob::attach(handle) {
                Ok(job) => Some(job),
                Err(error) => {
                    log::warn!("Failed to attach local shell to Windows Job Object: {error}");
                    None
                }
            });

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| io_other(format!("PTY reader creation failed: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| io_other(format!("PTY writer creation failed: {e}")))?;

        let (reader_tx, reader_rx) = mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("local-shell-reader".into())
            .spawn(move || read_pty(reader, reader_tx))
            .map_err(ChannelError::Io)?;

        let running = Arc::new(AtomicBool::new(true));
        let running_wait = running.clone();
        let exit = Arc::new(Mutex::new(None));
        let exit_wait = exit.clone();
        std::thread::Builder::new()
            .name("local-shell-waiter".into())
            .spawn(move || {
                let snapshot = match child.wait() {
                    Ok(status) => ProcessExit {
                        code: status.exit_code(),
                        signal: status.signal().map(str::to_owned),
                    },
                    Err(error) => {
                        log::warn!(
                            "Failed waiting for local shell process {process_id:?}: {error}"
                        );
                        ProcessExit {
                            code: 1,
                            signal: Some("wait failed".into()),
                        }
                    }
                };
                if let Ok(mut slot) = exit_wait.lock() {
                    *slot = Some(snapshot);
                }
                running_wait.store(false, Ordering::SeqCst);
            })
            .map_err(ChannelError::Io)?;

        #[cfg(unix)]
        let process_group = pair.master.process_group_leader();

        // 子进程已经持有 slave，父进程不再保留 slave 端。
        drop(pair.slave);

        Ok(Self {
            master: pair.master,
            writer: Some(writer),
            reader_rx,
            pending: VecDeque::new(),
            running,
            exit,
            killer,
            shutdown_started: false,
            #[cfg(unix)]
            process_group,
            #[cfg(windows)]
            job,
        })
    }

    fn wait_until_stopped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.running.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        !self.running.load(Ordering::SeqCst)
    }

    fn shutdown_process(&mut self) {
        if self.shutdown_started {
            return;
        }
        self.shutdown_started = true;

        // Dropping the PTY writer requests EOF first, giving the shell a chance
        // to flush history and terminate normally.
        self.writer.take();
        if self.wait_until_stopped(GRACEFUL_EXIT_TIMEOUT) {
            return;
        }

        #[cfg(unix)]
        {
            if let Some(group) = self.process_group {
                // Negative pid targets the entire process group created for the PTY.
                unsafe {
                    libc::kill(-group, libc::SIGHUP);
                }
            } else if let Ok(mut killer) = self.killer.lock() {
                let _ = killer.kill();
            }
        }

        #[cfg(windows)]
        {
            if let Some(job) = self.job.take() {
                // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE terminates the full tree.
                drop(job);
            } else if let Ok(mut killer) = self.killer.lock() {
                let _ = killer.kill();
            }
        }

        if self.wait_until_stopped(FORCE_EXIT_TIMEOUT) {
            return;
        }

        #[cfg(unix)]
        if let Some(group) = self.process_group {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }

        // Final fallback for platforms/configurations where process-tree control
        // could not be established.
        if self.running.load(Ordering::SeqCst) {
            if let Ok(mut killer) = self.killer.lock() {
                let _ = killer.kill();
            }
        }
    }

    fn exit_snapshot(&self) -> Option<ProcessExit> {
        self.exit.lock().ok().and_then(|slot| slot.clone())
    }

    fn await_exit_snapshot(&self) -> Option<ProcessExit> {
        for _ in 0..10 {
            if let Some(exit) = self.exit_snapshot() {
                return Some(exit);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.exit_snapshot()
    }
}

impl Read for LocalShellChannel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.pending.is_empty() {
            return Ok(drain_pending(&mut self.pending, buf));
        }

        match self.reader_rx.recv_timeout(READ_TIMEOUT) {
            Ok(ReaderEvent::Data(data)) => {
                self.pending.extend(data);
                Ok(drain_pending(&mut self.pending, buf))
            }
            Ok(ReaderEvent::Closed) => {
                let _ = self.await_exit_snapshot();
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "local shell PTY closed",
                ))
            }
            Ok(ReaderEvent::Error(error)) => Err(io_other(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "local shell read timeout",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "local shell reader stopped",
            )),
        }
    }
}

impl Write for LocalShellChannel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.writer.as_mut() {
            Some(writer) => writer.write(buf),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "local shell writer is closed",
            )),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.writer.as_mut() {
            Some(writer) => writer.flush(),
            None => Ok(()),
        }
    }
}

impl Channel for LocalShellChannel {
    fn is_connected(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.writer.is_some()
    }

    fn set_timeout(&mut self, _dur: Duration) -> Result<(), ChannelError> {
        Ok(())
    }

    fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<(), ChannelError> {
        self.master
            .resize(PtySize {
                rows: rows.clamp(1, u16::MAX as u32) as u16,
                cols: cols.clamp(1, u16::MAX as u32) as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| ChannelError::Io(io_other(format!("PTY resize failed: {e}"))))
    }

    fn shutdown(&mut self) -> Result<(), ChannelError> {
        self.shutdown_process();
        Ok(())
    }

    fn disconnect_info(&self, fallback: DisconnectInfo) -> DisconnectInfo {
        self.exit_snapshot()
            .map(|exit| DisconnectInfo::process_exited(exit.code, exit.signal.as_deref()))
            .unwrap_or(fallback)
    }
}

impl Drop for LocalShellChannel {
    fn drop(&mut self) {
        self.shutdown_process();
    }
}

fn read_pty(mut reader: Box<dyn Read + Send>, tx: mpsc::SyncSender<ReaderEvent>) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(ReaderEvent::Closed);
                break;
            }
            Ok(n) => {
                if tx.send(ReaderEvent::Data(buf[..n].to_vec())).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = tx.send(ReaderEvent::Error(error.to_string()));
                break;
            }
        }
    }
}

fn drain_pending(pending: &mut VecDeque<u8>, buf: &mut [u8]) -> usize {
    let count = pending.len().min(buf.len());
    for slot in &mut buf[..count] {
        *slot = pending.pop_front().expect("pending length checked");
    }
    count
}

fn io_other(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn attach(process: std::os::windows::io::RawHandle) -> std::io::Result<Self> {
        use std::mem::{size_of, zeroed};
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        let assigned =
            configured != 0 && unsafe { AssignProcessToJobObject(job, process as HANDLE) } != 0;
        if !assigned {
            let error = std::io::Error::last_os_error();
            unsafe { CloseHandle(job) };
            return Err(error);
        }

        Ok(Self { handle: job })
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn scripted_shell(script: &str) -> (String, Vec<String>) {
        (
            "cmd.exe".into(),
            vec!["/D".into(), "/Q".into(), "/C".into(), script.into()],
        )
    }

    #[cfg(unix)]
    fn scripted_shell(script: &str) -> (String, Vec<String>) {
        ("/bin/sh".into(), vec!["-c".into(), script.into()])
    }

    #[test]
    fn transports_output_resizes_and_reports_nonzero_exit() {
        #[cfg(windows)]
        let script = "echo TAUTERM_LOCAL_SHELL_TEST & exit /b 7";
        #[cfg(unix)]
        let script = "printf 'TAUTERM_LOCAL_SHELL_TEST\\n'; exit 7";
        let (shell, args) = scripted_shell(script);
        let cwd = std::env::current_dir().expect("current directory");
        let mut channel = LocalShellChannel::spawn(&shell, &args, &cwd).expect("spawn shell");
        channel.resize_pty(132, 43).expect("resize PTY");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut answered_cursor_query = false;
        while Instant::now() < deadline {
            match channel.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    output.extend_from_slice(&buffer[..n]);
                    if !answered_cursor_query && output.windows(4).any(|bytes| bytes == b"\x1b[6n")
                    {
                        channel
                            .write_all(b"\x1b[1;1R")
                            .expect("answer cursor query");
                        channel.flush().expect("flush cursor response");
                        answered_cursor_query = true;
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("TAUTERM_LOCAL_SHELL_TEST"), "output: {text}");

        let info = channel.disconnect_info(DisconnectInfo::io_error("fallback"));
        assert_eq!(info.exit_code, Some(7));
        assert!(info.retain_terminal);
    }

    #[test]
    fn shutdown_stops_a_running_shell() {
        #[cfg(windows)]
        let (shell, args) = scripted_shell("ping -n 30 127.0.0.1 >NUL");
        #[cfg(unix)]
        let (shell, args) = scripted_shell("sleep 30");
        let cwd = std::env::current_dir().expect("current directory");
        let mut channel = LocalShellChannel::spawn(&shell, &args, &cwd).expect("spawn shell");
        assert!(channel.is_connected());
        channel.shutdown().expect("shutdown shell");
        assert!(channel.wait_until_stopped(Duration::from_secs(2)));
    }
}
