//! ServiceBackend — 通过命名管道对接 TauTerm Windows 特权服务。
//!
//! 服务以 LocalSystem 执行 com0com 特权操作；客户端只发送窄类型化请求。
//! 客户端同时维护本进程创建端点的可见性注册表：bridge path 永远属于内部实现，
//! external path 才允许出现在普通串口发现结果中。

use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

use super::backend::{
    register_internal_endpoint_path, unregister_internal_endpoint_path, VirtualEndpoint,
    VirtualPortBackend, VirtualPortConfig,
};

const PIPE_NAME: &str = r"\\.\pipe\TauTermService";
const GENERIC_READ: u32 = 0x80000000;
const GENERIC_WRITE: u32 = 0x40000000;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;
const WAIT_OBJECT_0: u32 = 0;
const PIPE_IO_TIMEOUT_MS: u32 = 60_000;

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn pipe_error() -> String {
    let error = unsafe { GetLastError() };
    format!("virtual port service unavailable (win32 error {error})")
}

fn open_pipe() -> Result<OwnedHandle, String> {
    let name = wide(PIPE_NAME);
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(pipe_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn wait_io(handle: RawHandle, overlapped: &OVERLAPPED) -> bool {
    let wait = unsafe { WaitForSingleObject(overlapped.hEvent, PIPE_IO_TIMEOUT_MS) };
    if wait != WAIT_OBJECT_0 {
        unsafe { CancelIo(handle as HANDLE) };
        return false;
    }
    true
}

fn read_exact(handle: RawHandle, buffer: &mut [u8]) -> bool {
    let mut total = 0usize;
    while total < buffer.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return false;
        }
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle as HANDLE,
                buffer.as_mut_ptr().add(total),
                (buffer.len() - total) as u32,
                &mut read,
                &mut overlapped,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                if !wait_io(handle, &overlapped) {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return false;
                }
                let mut transferred = 0u32;
                let completed = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                unsafe { CloseHandle(overlapped.hEvent) };
                if completed == 0 {
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

fn write_exact(handle: RawHandle, buffer: &[u8]) -> bool {
    let mut total = 0usize;
    while total < buffer.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return false;
        }
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle as HANDLE,
                buffer.as_ptr().add(total),
                (buffer.len() - total) as u32,
                &mut written,
                &mut overlapped,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                if !wait_io(handle, &overlapped) {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return false;
                }
                let mut transferred = 0u32;
                let completed = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                unsafe { CloseHandle(overlapped.hEvent) };
                if completed == 0 {
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
    let mut length = [0u8; 4];
    if !read_exact(handle, &mut length) {
        return None;
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > 16 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0u8; length];
    read_exact(handle, &mut data).then_some(data)
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
    endpoints: HashMap<u32, VirtualEndpoint>,
}

pub struct ServiceBackend {
    inner: Mutex<ServiceInner>,
}

impl ServiceBackend {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ServiceInner {
                pipe: None,
                client_id: uuid::Uuid::new_v4().to_string(),
                next_id: 0,
                endpoints: HashMap::new(),
            }),
        }
    }

    pub fn connect(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
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
        let body = serde_json::to_vec(&hello).map_err(|error| error.to_string())?;
        if !write_frame(raw, &body) {
            return Err("virtual port service handshake write failed".into());
        }
        let frame = read_frame(raw)
            .ok_or_else(|| "virtual port service handshake read failed".to_string())?;
        let response: Response =
            serde_json::from_slice(&frame).map_err(|error| error.to_string())?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "handshake rejected".into()));
        }
        inner.pipe = Some(pipe);
        Ok(())
    }

    fn clear_local_endpoints(inner: &mut ServiceInner) {
        for endpoint in inner.endpoints.values() {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        }
        inner.endpoints.clear();
    }

    fn call(&self, op: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;

        if inner.pipe.is_none() {
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
            if !write_frame(
                raw,
                &serde_json::to_vec(&hello).map_err(|error| error.to_string())?,
            ) {
                return Err("virtual port service handshake write failed".into());
            }
            let frame = read_frame(raw)
                .ok_or_else(|| "virtual port service handshake read failed".to_string())?;
            let response: Response =
                serde_json::from_slice(&frame).map_err(|error| error.to_string())?;
            if !response.ok {
                return Err(response
                    .error
                    .unwrap_or_else(|| "handshake rejected".into()));
            }
            inner.pipe = Some(pipe);
        }

        let raw = inner
            .pipe
            .as_ref()
            .expect("service pipe initialized")
            .as_raw_handle();
        let id = inner.next_id;
        inner.next_id += 1;
        let request = serde_json::json!({
            "id": id,
            "op": op,
            "client_id": inner.client_id,
            "payload": payload,
        });
        let body = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        if !write_frame(raw, &body) {
            inner.pipe = None;
            Self::clear_local_endpoints(&mut inner);
            return Err("virtual port service write failed".into());
        }
        let frame = match read_frame(raw) {
            Some(frame) => frame,
            None => {
                inner.pipe = None;
                Self::clear_local_endpoints(&mut inner);
                return Err("virtual port service read failed (connection closed)".into());
            }
        };
        let response: Response =
            serde_json::from_slice(&frame).map_err(|error| error.to_string())?;
        if response.ok {
            Ok(response.data.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "unknown service error".into()))
        }
    }

    fn status(&self) -> Result<serde_json::Value, String> {
        self.call("status", serde_json::json!({}))
    }

    fn remember_endpoints(&self, endpoints: &[VirtualEndpoint]) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
        for endpoint in endpoints {
            register_internal_endpoint_path(&endpoint.bridge_path);
            inner
                .endpoints
                .insert(endpoint.resource_id, endpoint.clone());
        }
        Ok(())
    }

    fn forget_endpoint(&self, resource_id: u32) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
        if let Some(endpoint) = inner.endpoints.remove(&resource_id) {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        }
        Ok(())
    }

    fn forget_all_endpoints(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
        Self::clear_local_endpoints(&mut inner);
        Ok(())
    }
}

impl VirtualPortBackend for ServiceBackend {
    fn are_files_present(&self) -> bool {
        self.status()
            .map(|data| data["files_present"].as_bool().unwrap_or(false))
            .unwrap_or(false)
    }

    fn detect_driver(&self) -> bool {
        self.status()
            .map(|data| data["driver_installed"].as_bool().unwrap_or(false))
            .unwrap_or(false)
    }

    fn install_driver(&mut self) -> Result<(), String> {
        self.call("install_driver", serde_json::json!({}))
            .map(|_| ())
    }

    fn install_driver_elevated(&mut self) -> Result<(), String> {
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
        let endpoints: Vec<VirtualEndpoint> = serde_json::from_value(data)
            .map_err(|error| format!("invalid create_endpoints response: {error}"))?;
        self.remember_endpoints(&endpoints)?;
        Ok(endpoints)
    }

    fn create_endpoints_elevated(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, String> {
        self.create_endpoints(config)
    }

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        self.call(
            "remove_pair",
            serde_json::json!({ "bus": endpoint.resource_id }),
        )?;
        self.forget_endpoint(endpoint.resource_id)
    }

    fn cleanup_all(&mut self) {
        if self.call("cleanup_client", serde_json::json!({})).is_ok() {
            let _ = self.forget_all_endpoints();
        }
    }

    fn cleanup_orphans(&mut self) -> u32 {
        // 服务端通过 client_id 管理自己创建的端口；客户端不存在“扫描驱动找孤儿”的权限。
        0
    }

    fn cleanup_endpoints_elevated(&mut self) -> Result<u32, String> {
        self.call("cleanup_client", serde_json::json!({}))?;
        self.forget_all_endpoints()?;
        Ok(0)
    }

    fn pending_orphan_count(&self) -> u32 {
        self.status()
            .map(|data| data["orphan_count"].as_u64().unwrap_or(0) as u32)
            .unwrap_or(0)
    }
}

impl Drop for ServiceBackend {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut() {
            for endpoint in inner.endpoints.values() {
                unregister_internal_endpoint_path(&endpoint.bridge_path);
            }
        }
    }
}

impl Default for ServiceBackend {
    fn default() -> Self {
        Self::new()
    }
}
