//! Local Shell PTY 通道。
//!
//! 将 `portable-pty` 的阻塞 reader 隐藏在专用线程后，通过有界 channel
//! 向通用同步 I/O loop 暴露可超时的 `Read`。这样 PTY 无输出时仍能及时
//! 处理键盘写入、窗口 resize 与 Shutdown。

use crate::transport::{ReadStatus, StreamCloseMetadata, TransportError};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const GRACEFUL_EXIT_TIMEOUT: Duration = Duration::from_millis(350);
const FORCE_EXIT_TIMEOUT: Duration = Duration::from_millis(250);

/// Terminal queries the ConPTY engine sends while it initializes a pseudo
/// console, plus the replies the host must supply before the client can start.
/// Both engines ask for the initial cursor position (`PSEUDOCONSOLE_INHERIT_CURSOR`);
/// only the redistributed engine also probes device attributes before it lets the
/// client finish console initialization, so the reply set is selected per engine.
#[cfg(windows)]
struct ConptyStartupQuery {
    query: &'static [u8],
    reply: &'static [u8],
}

#[cfg(windows)]
const CONPTY_STARTUP_QUERIES: [ConptyStartupQuery; 2] = [
    // CPR: report the inherited cursor position as (1,1).
    ConptyStartupQuery {
        query: b"\x1b[6n",
        reply: b"\x1b[1;1R",
    },
    // DA1: report the same terminal identity xterm.js reports (VT100 with AVO).
    ConptyStartupQuery {
        query: b"\x1b[c",
        reply: b"\x1b[?1;2c",
    },
];

#[cfg(windows)]
const CONPTY_STARTUP_WINDOW: Duration = Duration::from_secs(5);

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

/// Startup handshake interceptor for the ConPTY engine.
///
/// The engine initializes the pseudo console before the client program can run
/// and asks the terminal for state while doing so. Whoever answers first owns the
/// session: if the query reached xterm before the terminal host was ready, the
/// client would wait for a reply that never comes. The driver therefore answers
/// the startup queries locally, strips them from the visible byte stream (a
/// second reply would become stray program input), and stops intercepting once
/// the engine is satisfied so later application DSR/DA queries reach xterm.
#[cfg(windows)]
struct ConptyStartupHandshake {
    /// Indices into `CONPTY_STARTUP_QUERIES` the selected engine still owes us.
    pending: Vec<usize>,
    finished: bool,
    deadline: Instant,
    tail: Vec<u8>,
}

#[cfg(windows)]
impl ConptyStartupHandshake {
    fn new(bundled_engine: bool) -> Self {
        let pending = if bundled_engine {
            (0..CONPTY_STARTUP_QUERIES.len()).collect()
        } else {
            vec![0]
        };
        Self {
            pending,
            finished: false,
            deadline: Instant::now() + CONPTY_STARTUP_WINDOW,
            tail: Vec::new(),
        }
    }

    /// Startup window elapsed: hand any buffered fragment back as normal output.
    fn take_expired_tail(&mut self) -> Option<Vec<u8>> {
        if self.finished || self.tail.is_empty() || Instant::now() <= self.deadline {
            return None;
        }
        self.finished = true;
        Some(std::mem::take(&mut self.tail))
    }

    fn intercept<W: Write + ?Sized>(
        &mut self,
        data: Vec<u8>,
        writer: &mut W,
    ) -> std::io::Result<Vec<u8>> {
        if self.finished {
            return Ok(data);
        }
        if Instant::now() > self.deadline {
            // Startup window elapsed without a complete probe. Stop intercepting so
            // later application/TUI queries are handled normally by xterm.
            self.finished = true;
            let mut visible = std::mem::take(&mut self.tail);
            visible.extend_from_slice(&data);
            return Ok(visible);
        }

        let mut combined = std::mem::take(&mut self.tail);
        combined.extend_from_slice(&data);

        let mut visible = Vec::with_capacity(combined.len());
        let mut cursor = 0;
        while !self.pending.is_empty() {
            let mut earliest: Option<(usize, usize)> = None;
            for &index in &self.pending {
                let query = CONPTY_STARTUP_QUERIES[index].query;
                if let Some(offset) = find_subsequence(&combined[cursor..], query) {
                    let position = cursor + offset;
                    if earliest.is_none_or(|(best, _)| position < best) {
                        earliest = Some((position, index));
                    }
                }
            }
            let Some((position, index)) = earliest else {
                break;
            };
            visible.extend_from_slice(&combined[cursor..position]);
            cursor = position + CONPTY_STARTUP_QUERIES[index].query.len();
            writer.write_all(CONPTY_STARTUP_QUERIES[index].reply)?;
            writer.flush()?;
            self.pending.retain(|pending| *pending != index);
        }

        visible.extend_from_slice(&combined[cursor..]);
        if self.pending.is_empty() {
            self.finished = true;
            log::debug!("ConPTY startup handshake answered");
            return Ok(visible);
        }

        // Keep only the longest suffix that can still become a pending query; emit
        // everything before it immediately.
        let keep = longest_pending_prefix(&visible, &self.pending);
        let emit = visible.len().saturating_sub(keep);
        self.tail = visible[emit..].to_vec();
        visible.truncate(emit);
        Ok(visible)
    }
}

/// Length of the longest suffix of `bytes` that is a proper prefix of any still
/// pending startup query.
#[cfg(windows)]
fn longest_pending_prefix(bytes: &[u8], pending: &[usize]) -> usize {
    pending
        .iter()
        .map(|&index| longest_suffix_prefix(bytes, CONPTY_STARTUP_QUERIES[index].query))
        .max()
        .unwrap_or(0)
}

/// 本地 Shell 的同步 PTY 通道。
pub struct LocalShellDriver {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    reader_rx: mpsc::Receiver<ReaderEvent>,
    pending: VecDeque<u8>,
    running: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<ProcessExit>>>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    shutdown_started: bool,
    // ConPTY initializes before the child can run and queries the terminal while
    // doing so. Answer the startup handshake locally so shell startup never
    // depends on the WebView/xterm event pipeline being ready.
    #[cfg(windows)]
    startup_handshake: ConptyStartupHandshake,
    #[cfg(unix)]
    process_group: Option<libc::pid_t>,
    #[cfg(windows)]
    job: Option<WindowsJob>,
}

impl LocalShellDriver {
    /// 创建 PTY 并在其中启动一个独立 argv 的本地进程。
    pub fn spawn(
        executable: &str,
        args: &[String],
        cwd: &std::path::Path,
        env: &[super::LocalShellEnvVar],
    ) -> std::io::Result<Self> {
        // Windows: pin the redistributed ConPTY engine *before* portable-pty loads
        // its ConPTY functions. `openpty` resolves the engine lazily and caches it
        // for the process, so an earlier call here would permanently fall back to
        // the in-box engine and silently drop the mouse modes this engine exists
        // for. The shell then runs directly as the PTY child, with no helper
        // process, no runtime whitelist and no console-mode polling. See
        // `conpty_engine` for the engine choice and its provenance.
        #[cfg(windows)]
        super::conpty_engine::install();

        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(|e| io_other(format!("PTY creation failed: {e}")))?;

        let mut command = CommandBuilder::new(executable);
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for entry in env {
            command.env(&entry.key, &entry.value);
        }

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
            .spawn(move || read_pty(reader, reader_tx))?;

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
            })?;

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
            #[cfg(windows)]
            startup_handshake: ConptyStartupHandshake::new(
                super::conpty_engine::installed_engine_path().is_some(),
            ),
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

    #[cfg(windows)]
    fn flush_expired_conpty_startup_tail(&mut self) -> bool {
        let Some(tail) = self.startup_handshake.take_expired_tail() else {
            return false;
        };
        // 只可能保留 ESC / ESC[ / ESC[6 这类查询前缀。启动窗口结束仍未补全
        // 查询序列时，它就是普通终端输出，必须交回 xterm，不能永久扣在后端。
        self.pending.extend(tail);
        true
    }

    #[cfg(windows)]
    fn handle_conpty_startup_handshake(&mut self, data: Vec<u8>) -> std::io::Result<Vec<u8>> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "local shell writer is closed during ConPTY startup handshake",
            )
        })?;
        self.startup_handshake.intercept(data, writer.as_mut())
    }
}

impl Read for LocalShellDriver {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.pending.is_empty() {
            return Ok(drain_pending(&mut self.pending, buf));
        }
        #[cfg(windows)]
        if self.flush_expired_conpty_startup_tail() {
            return Ok(drain_pending(&mut self.pending, buf));
        }

        match self.reader_rx.recv_timeout(READ_TIMEOUT) {
            Ok(ReaderEvent::Data(data)) => {
                #[cfg(windows)]
                let data = self.handle_conpty_startup_handshake(data)?;
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
            Err(mpsc::RecvTimeoutError::Timeout) => {
                #[cfg(windows)]
                if self.flush_expired_conpty_startup_tail() {
                    return Ok(drain_pending(&mut self.pending, buf));
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "local shell read timeout",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "local shell reader stopped",
            )),
        }
    }
}

impl Write for LocalShellDriver {
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

impl crate::transport::BlockingByteStream for LocalShellDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadStatus, TransportError> {
        match Read::read(self, buf) {
            Ok(n) if n > 0 => Ok(ReadStatus::Data(n)),
            Ok(_) => Ok(ReadStatus::Idle),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(ReadStatus::Idle)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                Ok(ReadStatus::Eof)
            }
            Err(error) => Err(TransportError::io("local_shell_read", error)),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        Write::write_all(self, data).map_err(|error| TransportError::io("local_shell_write", error))
    }

    fn flush(&mut self) -> Result<(), TransportError> {
        Write::flush(self).map_err(|error| TransportError::io("local_shell_flush", error))
    }

    fn shutdown(&mut self) -> Result<(), TransportError> {
        self.shutdown_process();
        Ok(())
    }

    fn resize_terminal(&mut self, cols: u32, rows: u32) -> Result<(), TransportError> {
        self.master
            .resize(PtySize {
                rows: rows.clamp(1, u16::MAX as u32) as u16,
                cols: cols.clamp(1, u16::MAX as u32) as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                TransportError::io(
                    "local_shell_resize",
                    io_other(format!("PTY resize failed: {error}")),
                )
            })
    }

    fn supports_terminal_control(&self) -> bool {
        true
    }

    fn close_metadata(&self) -> StreamCloseMetadata {
        self.exit_snapshot()
            .map(|exit| StreamCloseMetadata {
                exit_code: Some(exit.code),
                signal: exit.signal,
            })
            .unwrap_or_default()
    }
}

impl Drop for LocalShellDriver {
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

#[cfg(windows)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(windows)]
fn longest_suffix_prefix(data: &[u8], prefix: &[u8]) -> usize {
    let max = data.len().min(prefix.len().saturating_sub(1));
    (1..=max)
        .rev()
        .find(|&len| data[data.len() - len..] == prefix[..len])
        .unwrap_or(0)
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

    #[cfg(windows)]
    #[test]
    fn conpty_startup_handshake_answers_and_strips_queries() {
        let cpr = CONPTY_STARTUP_QUERIES[0].query;
        assert_eq!(longest_suffix_prefix(b"plain\x1b", cpr), 1);
        assert_eq!(longest_suffix_prefix(b"plain\x1b[", cpr), 2);
        assert_eq!(longest_suffix_prefix(b"plain\x1b[6", cpr), 3);
        assert_eq!(longest_suffix_prefix(b"plain text", cpr), 0);

        let mut handshake = ConptyStartupHandshake::new(true);
        let mut replies = Vec::new();

        // Both engine queries arrive split across pipe reads; visible bytes before,
        // between and after them must survive unchanged.
        let first = handshake
            .intercept(b"visible-a\x1b[6".to_vec(), &mut replies)
            .expect("first chunk");
        assert_eq!(first, b"visible-a");
        let second = handshake
            .intercept(b"n\x1b[cvisible-b".to_vec(), &mut replies)
            .expect("second chunk");
        assert_eq!(second, b"visible-b");
        assert_eq!(replies, b"\x1b[1;1R\x1b[?1;2c");

        // Once every expected reply is sent the interceptor stops buffering.
        assert!(handshake.finished);
        assert_eq!(
            handshake
                .intercept(b"\x1b[c".to_vec(), &mut replies)
                .expect("passthrough after handshake"),
            b"\x1b[c"
        );

        // The in-box engine only asks for the cursor position.
        let mut system_engine = ConptyStartupHandshake::new(false);
        let mut replies = Vec::new();
        assert_eq!(
            system_engine
                .intercept(b"\x1b[6n".to_vec(), &mut replies)
                .expect("system engine handshake"),
            b""
        );
        assert_eq!(replies, b"\x1b[1;1R");
        assert!(system_engine.finished);
    }

    // Development builds resolve the engine through the repository vendor directory,
    // which is exactly the payload the Windows bundle installs under `conpty/win-x64/`;
    // installed builds resolve it next to the executable.
    #[cfg(all(windows, debug_assertions))]
    #[test]
    fn bundled_conpty_engine_is_pinned_for_local_shells() {
        let engine = super::super::conpty_engine::install();
        let engine = engine.expect("bundled ConPTY engine must load from the vendored payload");
        assert!(
            engine
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("conpty.dll")),
            "unexpected engine path: {}",
            engine.display()
        );
    }

    #[test]
    fn transports_output_resizes_and_reports_nonzero_exit() {
        #[cfg(windows)]
        let script = "echo TAUTERM_LOCAL_SHELL_TEST & exit /b 7";
        #[cfg(unix)]
        let script = "printf 'TAUTERM_LOCAL_SHELL_TEST\\n'; exit 7";
        let (shell, args) = scripted_shell(script);
        let cwd = std::env::current_dir().expect("current directory");
        let mut channel = LocalShellDriver::spawn(&shell, &args, &cwd, &[]).expect("spawn shell");
        crate::transport::BlockingByteStream::resize_terminal(&mut channel, 132, 43)
            .expect("resize PTY");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        while Instant::now() < deadline {
            match channel.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    output.extend_from_slice(&buffer[..n]);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("TAUTERM_LOCAL_SHELL_TEST"), "output: {text}");
        #[cfg(windows)]
        for query in CONPTY_STARTUP_QUERIES.iter().map(|query| query.query) {
            assert!(
                !output.windows(query.len()).any(|bytes| bytes == query),
                "ConPTY startup query {query:?} should be consumed by LocalShellDriver"
            );
        }

        let info = crate::transport::BlockingByteStream::close_metadata(&channel);
        assert_eq!(info.exit_code, Some(7));
    }

    #[test]
    fn shutdown_stops_a_running_shell() {
        #[cfg(windows)]
        let (shell, args) = scripted_shell("ping -n 30 127.0.0.1 >NUL");
        #[cfg(unix)]
        let (shell, args) = scripted_shell("sleep 30");
        let cwd = std::env::current_dir().expect("current directory");
        let mut channel = LocalShellDriver::spawn(&shell, &args, &cwd, &[]).expect("spawn shell");
        assert!(channel.running.load(Ordering::SeqCst));
        crate::transport::BlockingByteStream::shutdown(&mut channel).expect("shutdown shell");
        assert!(channel.wait_until_stopped(Duration::from_secs(2)));
    }
}
