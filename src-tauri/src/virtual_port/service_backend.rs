//! ServiceBackend — 通过命名管道对接 TauTerm Windows 特权服务。
//!
//! 作为 `VirtualPortBackend` 的一个实现，把虚拟串口的特权操作委托给
//! `tauterm-service`（LocalSystem），使主程序可以 `asInvoker` 运行而无需
//! 每次操作弹 UAC。
//!
//! 连接生命周期：首次调用时建立到 `\\.\pipe\TauTermService` 的持久管道连接，
//! 并上报 `client_id`；连接关闭（App 退出/崩溃）时服务端自动清理该客户端
//! 创建的全部端口对。

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

use super::backend::{VirtualEndpoint, VirtualPortBackend, VirtualPortConfig};

const PIPE_NAME: &str = r"\\.\pipe\TauTermService";

const GENERIC_READ: u32 = 0x80000000;
const GENERIC_WRITE: u32 = 0x40000000;
const OPEN_EXISTING: u32 = 3;
/// 打开句柄即声明后续 I/O 走 OVERLAPPED，才能对阻塞读写施加超时。
const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;
const WAIT_OBJECT_0: u32 = 0;
/// 单次管道读写的最长等待时间。服务进程卡死时避免应用永久阻塞；
/// 服务崩溃时管道会被 OS 关闭，ReadFile 立即返回，不会拖满该超时。
const PIPE_IO_TIMEOUT_MS: u32 = 60_000;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 连接失败 / 服务不可用时的错误。
fn pipe_error() -> String {
    let err = unsafe { GetLastError() };
    format!("virtual port service unavailable (win32 error {})", err)
}

fn open_pipe() -> Result<OwnedHandle, String> {
    let name = wide(PIPE_NAME);
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0, // 独占
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(pipe_error());
    }
    // SAFETY: 刚由 CreateFileW 返回的有效句柄，所有权移交 OwnedHandle
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

/// 对一次 OVERLAPPED 读写执行等待；超时则取消挂起操作并返回 false。
fn wait_io(handle: RawHandle, overlapped: &OVERLAPPED) -> bool {
    let wait = unsafe { WaitForSingleObject(overlapped.hEvent, PIPE_IO_TIMEOUT_MS) };
    if wait != WAIT_OBJECT_0 {
        // 超时或等待失败：取消挂起 I/O（管道随后会被上层关闭）
        unsafe { CancelIo(handle as HANDLE) };
        return false;
    }
    true
}

fn read_exact(handle: RawHandle, buf: &mut [u8]) -> bool {
    let mut total = 0usize;
    while total < buf.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return false;
        }
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle as HANDLE,
                buf.as_mut_ptr().add(total),
                (buf.len() - total) as u32,
                &mut read,
                &mut overlapped,
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_IO_PENDING {
                if !wait_io(handle, &overlapped) {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return false;
                }
                let mut transferred = 0u32;
                let ok2 = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                unsafe { CloseHandle(overlapped.hEvent) };
                if ok2 == 0 {
                    return false;
                }
                read = transferred;
            } else {
                unsafe { CloseHandle(overlapped.hEvent) };
                return false;
            }
        } else {
            unsafe { CloseHandle(overlapped.hEvent) };
        }
        if read == 0 {
            return false;
        }
        total += read as usize;
    }
    true
}

fn write_exact(handle: RawHandle, buf: &[u8]) -> bool {
    let mut total = 0usize;
    while total < buf.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return false;
        }
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle as HANDLE,
                buf.as_ptr().add(total),
                (buf.len() - total) as u32,
                &mut written,
                &mut overlapped,
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_IO_PENDING {
                if !wait_io(handle, &overlapped) {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return false;
                }
                let mut transferred = 0u32;
                let ok2 = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                unsafe { CloseHandle(overlapped.hEvent) };
                if ok2 == 0 {
                    return false;
                }
                written = transferred;
            } else {
                unsafe { CloseHandle(overlapped.hEvent) };
                return false;
            }
        } else {
            unsafe { CloseHandle(overlapped.hEvent) };
        }
        if written == 0 {
            return false;
        }
        total += written as usize;
    }
    true
}

fn write_frame(handle: RawHandle, data: &[u8]) -> bool {
    let header = (data.len() as u32).to_le_bytes();
    write_exact(handle, &header) && write_exact(handle, data)
}

fn read_frame(handle: RawHandle) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    if !read_exact(handle, &mut len_buf) {
        return None;
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 16 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0u8; len];
    if !read_exact(handle, &mut data) {
        return None;
    }
    Some(data)
}

#[derive(serde::Deserialize)]
struct Response {
    #[allow(dead_code)]
    id: u64,
    ok: bool,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

struct ServiceInner {
    pipe: Option<OwnedHandle>,
    client_id: String,
    next_id: u64,
}

/// Windows 特权服务客户端后端。
pub struct ServiceBackend {
    inner: Mutex<ServiceInner>,
}

impl ServiceBackend {
    /// 构造一个尚未连接的后端（`client_id` 在构造时生成）。
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ServiceInner {
                pipe: None,
                client_id: uuid::Uuid::new_v4().to_string(),
                next_id: 0,
            }),
        }
    }

    /// 建立到服务的持久连接并完成握手。失败时返回错误（表示服务不可用）。
    pub fn connect(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let pipe = open_pipe()?;
        let id = inner.next_id;
        inner.next_id += 1;
        let hello = serde_json::json!({
            "id": id,
            "op": "hello",
            "client_id": inner.client_id,
            "payload": {},
        });
        let raw = pipe.as_raw_handle();
        let body = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
        if !write_frame(raw, &body) {
            return Err("virtual port service handshake write failed".into());
        }
        let frame = read_frame(raw)
            .ok_or_else(|| "virtual port service handshake read failed".to_string())?;
        let resp: Response = serde_json::from_slice(&frame).map_err(|e| e.to_string())?;
        if !resp.ok {
            return Err(resp.error.unwrap_or_else(|| "handshake rejected".into()));
        }
        inner.pipe = Some(pipe);
        Ok(())
    }

    /// 发送一次请求并等待响应。断线后置空连接，下次调用会重连。
    fn call(&self, op: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;

        if inner.pipe.is_none() {
            let pipe = open_pipe()?;
            let id = inner.next_id;
            inner.next_id += 1;
            let hello = serde_json::json!({
                "id": id, "op": "hello", "client_id": inner.client_id, "payload": {},
            });
            let raw = pipe.as_raw_handle();
            if !write_frame(raw, &serde_json::to_vec(&hello).map_err(|e| e.to_string())?) {
                return Err("virtual port service handshake write failed".into());
            }
            let frame = read_frame(raw)
                .ok_or_else(|| "virtual port service handshake read failed".to_string())?;
            let resp: Response = serde_json::from_slice(&frame).map_err(|e| e.to_string())?;
            if !resp.ok {
                return Err(resp.error.unwrap_or_else(|| "handshake rejected".into()));
            }
            inner.pipe = Some(pipe);
        }

        let raw = inner.pipe.as_ref().unwrap().as_raw_handle();
        let id = inner.next_id;
        inner.next_id += 1;
        let req = serde_json::json!({
            "id": id, "op": op, "client_id": inner.client_id, "payload": payload,
        });
        let body = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
        if !write_frame(raw, &body) {
            inner.pipe = None;
            return Err("virtual port service write failed".into());
        }
        let frame = match read_frame(raw) {
            Some(f) => f,
            None => {
                inner.pipe = None;
                return Err("virtual port service read failed (connection closed)".into());
            }
        };
        let resp: Response = serde_json::from_slice(&frame).map_err(|e| e.to_string())?;
        if resp.ok {
            Ok(resp.data.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(resp.error.unwrap_or_else(|| "unknown service error".into()))
        }
    }

    fn status(&self) -> Result<serde_json::Value, String> {
        self.call("status", serde_json::json!({}))
    }
}

impl VirtualPortBackend for ServiceBackend {
    fn are_files_present(&self) -> bool {
        self.status()
            .map(|d| d["files_present"].as_bool().unwrap_or(false))
            .unwrap_or(false)
    }

    fn detect_driver(&self) -> bool {
        self.status()
            .map(|d| d["driver_installed"].as_bool().unwrap_or(false))
            .unwrap_or(false)
    }

    fn install_driver(&mut self) -> Result<(), String> {
        self.call("install_driver", serde_json::json!({}))
            .map(|_| ())
    }

    fn install_driver_elevated(&mut self) -> Result<(), String> {
        // 服务以 LocalSystem 运行，本身即具备管理员权限，无需单独提权
        self.install_driver()
    }

    fn create_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        let data = self.call(
            "create_endpoints",
            serde_json::json!({ "count": config.count }),
        )?;
        serde_json::from_value(data)
            .map_err(|e| format!("invalid create_endpoints response: {}", e))
    }

    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        self.create_endpoints(config)
    }

    fn destroy_endpoint(&mut self, pair: &VirtualEndpoint) -> Result<(), String> {
        self.call(
            "remove_pair",
            serde_json::json!({ "bus": pair.resource_id }),
        )
        .map(|_| ())
    }

    fn cleanup_all(&mut self) {
        let _ = self.call("cleanup_client", serde_json::json!({}));
    }

    fn cleanup_orphans(&mut self) -> u32 {
        // 孤儿清理由服务在自身启动时完成，客户端侧无需再清理
        0
    }

    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        self.call("cleanup_client", serde_json::json!({}))
            .map(|_| 0)
    }

    fn pending_orphan_count(&self) -> u32 {
        self.status()
            .map(|d| d["orphan_count"].as_u64().unwrap_or(0) as u32)
            .unwrap_or(0)
    }
}

impl Default for ServiceBackend {
    fn default() -> Self {
        Self::new()
    }
}
