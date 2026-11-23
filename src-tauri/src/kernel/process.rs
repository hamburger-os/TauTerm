//! 子进程可见性：桌面构建上唯一允许的后台进程启动入口。
//!
//! Release 构建是 GUI 子系统（`main.rs` 的 `windows_subsystem = "windows"`），
//! 进程自身没有控制台；任何 console 子系统子进程（`wsl.exe`、`setupc.exe`、
//! TRDP 原生 bridge …）都会由 Windows 新分配一个控制台窗口，用户看到的就是
//! “黑框一闪而过”。`CREATE_NO_WINDOW` 让子进程在没有窗口的控制台里运行，
//! 同时 stdio 仍可正常重定向。
//!
//! 契约（由 `npm run check:windows-spawn` 锁定）：
//!
//! - 通过 `Command` 启动的后台探针、探测与助手进程一律经 [`hidden_command`]；
//! - 需要用户真正看见的程序（系统文件管理器等）继续使用 `Command::new`，
//!   但必须在同一行标注 `// visible-launcher:` 并说明理由。
//!
//! 例外（不属于本契约、也不适合塞进 `Command`）：UAC 提权 helper 必须用
//! `ShellExecuteExW("runas")` 启动，它们以 `nShow = 0`（SW_HIDE）自身隐藏窗口，
//! 见 `plugins/local_shell/elevated.rs` 与 `virtual_port/elevated.rs`。

use std::ffi::OsStr;
use std::process::Command;

#[cfg(target_os = "windows")]
fn apply_hidden_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn apply_hidden_console(_command: &mut Command) {}

/// 构建一个不会创建可见控制台窗口的 [`Command`]。
///
/// 返回的 `Command` 可以照常链式配置参数与 stdio。
pub fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    apply_hidden_console(&mut command);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_command_still_spawns_and_exits_normally() {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = hidden_command("cmd.exe");
            command.args(["/D", "/Q", "/C", "exit 0"]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = hidden_command("/bin/sh");
            command.args(["-c", "exit 0"]);
            command
        };

        let status = command
            .status()
            .expect("hidden console child must still be spawnable");
        assert!(status.success(), "hidden console child exited: {status:?}");
    }
}
