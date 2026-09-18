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
    VirtualPortBackend, VirtualPortConfig, VirtualPortError,
};

const PIPE_NAME: &str = r"\\.\pipe\TauTermService";
const SERVICE_PROTOCOL_VERSION: u64 = 2;
const GENERIC_READ: u32 = 0x80000000;
const GENERIC_WRITE: u32 = 0x40000000;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const WAIT_FAILED: u32 = 0xFFFF_FFFF;
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

fn cancel_and_drain_io(handle: RawHandle, overlapped: &OVERLAPPED) {
    // CancelIo only requests cancellation. Keep OVERLAPPED/event storage alive until the
    // cancellation completion is observed before the caller closes the event and returns.
    unsafe { CancelIo(handle as HANDLE) };
    let mut transferred = 0u32;
    let _ = unsafe { GetOverlappedResult(handle as HANDLE, overlapped, &mut transferred, 1) };
}

fn wait_io(handle: RawHandle, overlapped: &OVERLAPPED, operation: &str) -> Result<(), String> {
    let wait = unsafe { WaitForSingleObject(overlapped.hEvent, PIPE_IO_TIMEOUT_MS) };
    match wait {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => {
            cancel_and_drain_io(handle, overlapped);
            Err(format!(
                "virtual port service {operation} timed out after {PIPE_IO_TIMEOUT_MS} ms"
            ))
        }
        WAIT_FAILED => {
            let error = unsafe { GetLastError() };
            cancel_and_drain_io(handle, overlapped);
            Err(format!(
                "virtual port service {operation} wait failed (win32 error {error})"
            ))
        }
        other => {
            cancel_and_drain_io(handle, overlapped);
            Err(format!(
                "virtual port service {operation} wait returned unexpected status {other}"
            ))
        }
    }
}
fn read_exact(handle: RawHandle, buffer: &mut [u8]) -> Result<(), String> {
    let mut total = 0usize;
    while total < buffer.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return Err(format!(
                "virtual port service read event creation failed (win32 error {})",
                unsafe { GetLastError() }
            ));
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
                if let Err(wait_error) = wait_io(handle, &overlapped, "read") {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return Err(wait_error);
                }
                let mut transferred = 0u32;
                let completed = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                if completed == 0 {
                    let error = unsafe { GetLastError() };
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return Err(format!(
                        "virtual port service read completion failed (win32 error {error})"
                    ));
                }
                read = transferred;
            } else {
                unsafe { CloseHandle(overlapped.hEvent) };
                return Err(format!(
                    "virtual port service read failed (win32 error {error})"
                ));
            }
        }
        unsafe { CloseHandle(overlapped.hEvent) };

        if read == 0 {
            return Err("virtual port service closed the pipe while reading".into());
        }
        total += read as usize;
    }
    Ok(())
}

fn write_exact(handle: RawHandle, buffer: &[u8]) -> Result<(), String> {
    let mut total = 0usize;
    while total < buffer.len() {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if overlapped.hEvent.is_null() {
            return Err(format!(
                "virtual port service write event creation failed (win32 error {})",
                unsafe { GetLastError() }
            ));
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
                if let Err(wait_error) = wait_io(handle, &overlapped, "write") {
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return Err(wait_error);
                }
                let mut transferred = 0u32;
                let completed = unsafe {
                    GetOverlappedResult(handle as HANDLE, &overlapped, &mut transferred, 0)
                };
                if completed == 0 {
                    let error = unsafe { GetLastError() };
                    unsafe { CloseHandle(overlapped.hEvent) };
                    return Err(format!(
                        "virtual port service write completion failed (win32 error {error})"
                    ));
                }
                written = transferred;
            } else {
                unsafe { CloseHandle(overlapped.hEvent) };
                return Err(format!(
                    "virtual port service write failed (win32 error {error})"
                ));
            }
        }
        unsafe { CloseHandle(overlapped.hEvent) };

        if written == 0 {
            return Err("virtual port service closed the pipe while writing".into());
        }
        total += written as usize;
    }
    Ok(())
}

fn write_frame(handle: RawHandle, data: &[u8]) -> Result<(), String> {
    let header = (data.len() as u32).to_le_bytes();
    write_exact(handle, &header)?;
    write_exact(handle, data)
}

fn read_frame(handle: RawHandle) -> Result<Vec<u8>, String> {
    let mut length = [0u8; 4];
    read_exact(handle, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > 16 * 1024 * 1024 {
        return Err(format!(
            "invalid virtual port service frame length: {length}"
        ));
    }
    let mut data = vec![0u8; length];
    read_exact(handle, &mut data)?;
    Ok(data)
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
    /// 仅属于当前 pipe generation。重连必须换新 ID，避免旧连接迟到的断开清理
    /// 误删新连接刚创建的资源。
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
                client_id: String::new(),
                next_id: 0,
                endpoints: HashMap::new(),
            }),
        }
    }

    fn connect_inner(inner: &mut ServiceInner) -> Result<(), String> {
        if inner.pipe.is_some() {
            return Ok(());
        }

        let pipe = open_pipe()?;
        let client_id = uuid::Uuid::new_v4().to_string();
        let id = inner.next_id;
        inner.next_id += 1;
        let hello = serde_json::json!({
            "id": id,
            "op": "hello",
            "client_id": client_id,
            "payload": { "protocol_version": SERVICE_PROTOCOL_VERSION },
        });
        let raw = pipe.as_raw_handle();
        let body = serde_json::to_vec(&hello).map_err(|error| error.to_string())?;
        write_frame(raw, &body)
            .map_err(|error| format!("virtual port service handshake write failed: {error}"))?;
        let frame = read_frame(raw).map_err(|error| {
            format!(
                "virtual port service handshake read failed: {error}; the service may have rejected this executable (development builds use direct UAC) or be incompatible"
            )
        })?;
        let response: Response = serde_json::from_slice(&frame)
            .map_err(|error| format!("invalid virtual port service handshake response: {error}"))?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "virtual port service handshake rejected".into()));
        }
        let protocol_version = response
            .data
            .as_ref()
            .and_then(|data| data.get("protocol_version"))
            .and_then(serde_json::Value::as_u64);
        if protocol_version != Some(SERVICE_PROTOCOL_VERSION) {
            return Err(format!(
                "virtual port service protocol mismatch (expected {SERVICE_PROTOCOL_VERSION}, got {})",
                protocol_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "missing".into())
            ));
        }

        let adopted: Vec<VirtualEndpoint> = response
            .data
            .as_ref()
            .and_then(|data| data.get("adopted_endpoints"))
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("invalid adopted endpoint list from service: {error}"))?
            .unwrap_or_default();

        Self::clear_local_endpoints(inner);
        for endpoint in adopted {
            register_internal_endpoint_path(&endpoint.bridge_path);
            inner.endpoints.insert(endpoint.resource_id, endpoint);
        }

        inner.client_id = client_id;
        inner.pipe = Some(pipe);
        Ok(())
    }

    pub fn connect(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
        Self::connect_inner(&mut inner)
    }

    fn clear_local_endpoints(inner: &mut ServiceInner) {
        for endpoint in inner.endpoints.values() {
            unregister_internal_endpoint_path(&endpoint.bridge_path);
        }
        inner.endpoints.clear();
    }

    fn reset_connection(inner: &mut ServiceInner) {
        inner.pipe = None;
        inner.client_id.clear();
        // Keep endpoint identity and internal-port hiding across a transient service restart.
        // The next hello re-adopts protected records for this GUI PID in TauTermService.
    }

    fn call(&self, op: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let mut inner = self.inner.lock().map_err(|error| error.to_string())?;
        Self::connect_inner(&mut inner)?;

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
        if let Err(error) = write_frame(raw, &body) {
            Self::reset_connection(&mut inner);
            return Err(error);
        }
        let frame = match read_frame(raw) {
            Ok(frame) => frame,
            Err(error) => {
                Self::reset_connection(&mut inner);
                return Err(error);
            }
        };
        let response: Response = serde_json::from_slice(&frame)
            .map_err(|error| format!("invalid virtual port service response: {error}"))?;
        if response.ok {
            Ok(response.data.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "unknown virtual port service error".into()))
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

    fn ensure_endpoints(
        &mut self,
        config: &VirtualPortConfig,
    ) -> Result<Vec<VirtualEndpoint>, VirtualPortError> {
        if !config.enabled || config.count == 0 {
            return Ok(Vec::new());
        }
        let status = self.status().map_err(VirtualPortError::from_backend)?;
        if !status["files_present"].as_bool().unwrap_or(false) {
            return Err(VirtualPortError::FilesMissing);
        }
        if !status["driver_installed"].as_bool().unwrap_or(false) {
            self.install_driver()
                .map_err(VirtualPortError::from_backend)?;
            let status = self.status().map_err(VirtualPortError::from_backend)?;
            if !status["driver_installed"].as_bool().unwrap_or(false) {
                return Err(VirtualPortError::DriverMissing);
            }
        }

        let data = self
            .call(
                "create_endpoints",
                serde_json::json!({ "count": config.count }),
            )
            .map_err(VirtualPortError::from_backend)?;
        let endpoints: Vec<VirtualEndpoint> = serde_json::from_value(data).map_err(|error| {
            VirtualPortError::Backend(format!("invalid create_endpoints response: {error}"))
        })?;
        self.remember_endpoints(&endpoints)
            .map_err(VirtualPortError::from_backend)?;
        Ok(endpoints)
    }

    fn destroy_endpoint(&mut self, endpoint: &VirtualEndpoint) -> Result<(), String> {
        self.call(
            "remove_pair",
            serde_json::json!({ "bus": endpoint.resource_id }),
        )?;
        self.forget_endpoint(endpoint.resource_id)
    }

    fn cleanup_all(&mut self) {
        let endpoints = match self.inner.lock() {
            Ok(inner) => inner.endpoints.values().cloned().collect::<Vec<_>>(),
            Err(error) => {
                log::warn!("failed to lock virtual-port service backend for cleanup: {error}");
                return;
            }
        };

        for endpoint in endpoints {
            if let Err(error) = self.destroy_endpoint(&endpoint) {
                log::warn!(
                    "virtual-port service cleanup deferred for {} (bus {}): {}",
                    endpoint.external_path,
                    endpoint.resource_id,
                    error
                );
            }
        }
    }

    fn cleanup_orphans(&mut self) -> Result<u32, String> {
        let data = self.call("cleanup_orphans", serde_json::json!({}))?;
        Ok(data["cleaned"].as_u64().unwrap_or(0) as u32)
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
