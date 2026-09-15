//! Windows com0com 驱动安装状态探测。
//!
//! 驱动是否安装是 Windows SCM 的事实，不需要启动 `setupc.exe` 或 `sc.exe`。
//! 打包后的 `setupc.exe` 可能因为执行清单/UAC 策略连只读 `list` 也要求提升，
//! 因此普通 GUI 启动和状态查询直接使用 SCM API 判断驱动服务是否存在；端口对
//! 枚举属于特权服务或用户明确触发的提权操作边界。

use std::ptr;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
};

pub fn is_com0com_driver_installed() -> bool {
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return false;
    }

    let service_name: Vec<u16> = "com0com\0".encode_utf16().collect();
    let service = unsafe { OpenServiceW(manager, service_name.as_ptr(), SERVICE_QUERY_STATUS) };
    let installed = !service.is_null();

    if !service.is_null() {
        unsafe {
            CloseServiceHandle(service);
        }
    }
    unsafe {
        CloseServiceHandle(manager);
    }

    installed
}
