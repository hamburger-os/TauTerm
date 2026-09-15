//! Windows com0com 驱动安装状态探测。
//!
//! 驱动是否安装是 Windows SCM 的事实，不需要启动 `setupc.exe`。打包后的
//! `setupc.exe` 可能因为执行清单/UAC 策略连只读 `list` 也要求提升，因此普通
//! GUI 启动和状态查询只能通过 SCM 判断驱动存在性；端口对枚举属于特权服务或
//! 用户明确触发的提权操作边界。

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn is_com0com_driver_installed() -> bool {
    let mut command = Command::new("sc");
    command.args(["query", "com0com"]);
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
