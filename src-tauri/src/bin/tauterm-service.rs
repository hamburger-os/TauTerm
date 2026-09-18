//! TauTerm Windows 特权服务（com0com 后端）。
//!
//! 以 `LocalSystem` 运行，通过命名管道 `\\.\pipe\TauTermService` 接收本应用
//! `tauterm.exe` 的窄类型化请求，代理执行 com0com 驱动安装与虚拟端口对的
//! 创建/删除/清理等需要管理员权限的操作。
//!
//! 安全边界：
//! - 管道安全描述符仅授予「Authenticated Users 读写 + SYSTEM/Administrators 完全控制」；
//! - 连接后通过 `GetNamedPipeClientProcessId` + `QueryFullProcessImageNameW`
//!   校验调用方进程镜像名必须为 `tauterm.exe`；
//! - 仅接受固定的窄操作集，绝不透传任意 `setupc` 参数。
//!
//! 客户端以「连接」为单位记账：`hello` 上报 `client_id` 与内部协议版本，断开
//! （管道关闭）时自动清理该客户端创建的全部端口对。服务同时在 ProgramData
//! 持久化自己的 ownership；若服务自身崩溃或系统异常掉电，重启后只恢复/清理
//! 有 ownership 证据的 TauTerm 资源，不扫描删除第三方 com0com bus。

#[cfg(windows)]
mod service {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::{Arc, Mutex};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    };
    use windows_sys::Win32::System::Services::{
        RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
        SERVICE_STATUS, SERVICE_STATUS_HANDLE, SERVICE_TABLE_ENTRYW,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    };

    use tauterm_lib::virtual_port::backend::{VirtualEndpoint, VirtualPortConfig};
    use tauterm_lib::virtual_port::manager::VirtualPortManager;

    // ── Win32 常量（硬编码避免依赖 feature 导出名差异） ──
    const PIPE_ACCESS_DUPLEX: u32 = 0x3;
    const PIPE_TYPE_BYTE: u32 = 0x0;
    const PIPE_READMODE_BYTE: u32 = 0x0;
    const PIPE_WAIT: u32 = 0x0;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const INFINITE: u32 = 0xFFFF_FFFF;

    const GENERIC_READ: u32 = 0x80000000;
    const GENERIC_WRITE: u32 = 0x40000000;
    const OPEN_EXISTING: u32 = 3;

    const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
    const SERVICE_RUNNING: u32 = 0x4;
    const SERVICE_STOPPED: u32 = 0x1;
    const SERVICE_STOP_PENDING: u32 = 0x3;
    const SERVICE_ACCEPT_STOP: u32 = 0x1;
    const SERVICE_ACCEPT_SHUTDOWN: u32 = 0x4;
    const SERVICE_CONTROL_STOP: u32 = 0x1;
    const SERVICE_CONTROL_SHUTDOWN: u32 = 0x5;
    const SERVICE_CONTROL_INTERROGATE: u32 = 0x4;

    const PIPE_NAME: &str = r"\\.\pipe\TauTermService";
    const SERVICE_NAME: &str = "TauTermService";
    const EXPECTED_CLIENT_EXE: &str = "tauterm.exe";
    const SERVICE_PROTOCOL_VERSION: u64 = 2;
    const MAX_FRAME: usize = 1024 * 1024;

    /// 收到 STOP/SHUTDOWN 时置位，主循环据此退出。
    static SHUTDOWN: AtomicBool = AtomicBool::new(false);
    /// 服务状态句柄（由 service_main 注册后写入，供 handler 汇报状态）。
    static STATUS_HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // ── 命名管道 + 帧协议 ──────────────────────────────

    fn create_pipe() -> Result<HANDLE, String> {
        // AU=Authenticated Users(read/write), SY=SYSTEM(full), BA=Administrators(full).
        // The pipe rejects remote clients before process-identity verification.
        let sddl = wide("D:P(A;;GRGW;;;AU)(A;;GA;;;SY)(A;;GA;;;BA)");
        let mut descriptor: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut descriptor_size = 0u32;
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                &mut descriptor_size,
            )
        };
        if converted == 0 || descriptor.is_null() {
            return Err(format!(
                "failed to build TauTermService pipe DACL (Win32 {})",
                unsafe { GetLastError() }
            ));
        }

        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(PIPE_NAME);
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                &attributes,
            )
        };
        let create_error = (pipe == INVALID_HANDLE_VALUE).then(|| unsafe { GetLastError() });
        unsafe {
            let _ = LocalFree(descriptor);
        }

        if let Some(error) = create_error {
            Err(format!(
                "failed to create TauTermService pipe (Win32 {error})"
            ))
        } else {
            Ok(pipe)
        }
    }

    fn verify_client(pipe: HANDLE) -> Option<u32> {
        let mut pid = 0u32;
        if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 {
            return None;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let client_path = std::path::Path::new(&path);
        let file_name = client_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if file_name != EXPECTED_CLIENT_EXE {
            return None;
        }
        // 仅校验文件名可被轻易绕过（把任意程序改名成 tauterm.exe 即可冒充）。
        // 客户端 exe 必须与服务自身位于同一目录（安装目录）：安装目录普通用户
        // 无写权限，无法在其中放置伪造的 tauterm.exe。
        let client_dir = client_path
            .parent()
            .map(|d| d.to_string_lossy().to_lowercase());
        let service_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_lowercase()));
        match (client_dir, service_dir) {
            (Some(c), Some(s)) if c == s => Some(pid),
            (Some(c), Some(s)) => {
                log::warn!(
                    "pipe client rejected: dir mismatch (client={}, service={})",
                    c,
                    s
                );
                None
            }
            _ => None,
        }
    }

    fn schedule_disconnect_cleanup(
        vpm: Arc<Mutex<VirtualPortManager>>,
        client_pid: u32,
        endpoints: Vec<VirtualEndpoint>,
    ) {
        if endpoints.is_empty() {
            return;
        }

        let process = unsafe { OpenProcess(SYNCHRONIZE, 0, client_pid) };
        if process.is_null() {
            // Fail closed: an unverifiable PID must not authorize endpoint deletion. Protected
            // ownership remains available for service restart/manual reconciliation.
            log::warn!(
                "cannot monitor GUI PID {} after service disconnect; preserving {} virtual endpoint(s)",
                client_pid,
                endpoints.len()
            );
            return;
        }

        let _ = std::thread::spawn(move || {
            let wait = unsafe { WaitForSingleObject(process, INFINITE) };
            unsafe { CloseHandle(process) };
            if wait != WAIT_OBJECT_0 {
                log::warn!(
                    "GUI PID {} exit watcher failed; preserving {} virtual endpoint(s)",
                    client_pid,
                    endpoints.len()
                );
                return;
            }

            match vpm.lock() {
                Ok(mut manager) => {
                    for endpoint in &endpoints {
                        if let Err(error) = manager.destroy_endpoint(endpoint) {
                            log::warn!(
                                "post-exit cleanup bus {} deferred after error: {}",
                                endpoint.resource_id,
                                error
                            );
                        }
                    }
                }
                Err(error) => {
                    log::warn!(
                        "virtual-port manager lock poisoned during post-exit cleanup: {error}"
                    );
                }
            }
        });
    }

    fn read_exact(pipe: HANDLE, buf: &mut [u8]) -> bool {
        let mut total = 0usize;
        while total < buf.len() {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    pipe,
                    buf.as_mut_ptr().add(total),
                    (buf.len() - total) as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                return false;
            }
            total += read as usize;
        }
        true
    }

    fn write_exact(pipe: HANDLE, buf: &[u8]) -> bool {
        let mut total = 0usize;
        while total < buf.len() {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    pipe,
                    buf.as_ptr().add(total),
                    (buf.len() - total) as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                return false;
            }
            total += written as usize;
        }
        true
    }

    fn read_frame(pipe: HANDLE) -> Option<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        if !read_exact(pipe, &mut len_buf) {
            return None;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > MAX_FRAME {
            return None;
        }
        let mut data = vec![0u8; len];
        if !read_exact(pipe, &mut data) {
            return None;
        }
        Some(data)
    }

    fn write_frame(pipe: HANDLE, data: &[u8]) -> bool {
        if data.is_empty() || data.len() > MAX_FRAME {
            return false;
        }
        let header = (data.len() as u32).to_le_bytes();
        write_exact(pipe, &header) && write_exact(pipe, data)
    }

    // ── 请求/响应与分发 ────────────────────────────────

    #[derive(serde::Deserialize)]
    struct Request {
        id: u64,
        op: String,
        client_id: String,
        #[serde(default)]
        payload: serde_json::Value,
    }

    #[derive(serde::Serialize)]
    struct Response {
        id: u64,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    impl Response {
        fn err(id: u64, e: String) -> Self {
            Response {
                id,
                ok: false,
                data: None,
                error: Some(e),
            }
        }
    }

    fn dispatch(
        vpm: &mut VirtualPortManager,
        endpoints: &mut Vec<VirtualEndpoint>,
        bound_client_id: &mut Option<String>,
        req: &Request,
        client_pid: u32,
    ) -> Response {
        let id = req.id;

        if req.op != "hello" && bound_client_id.as_deref() != Some(req.client_id.as_str()) {
            return Response::err(
                id,
                "virtual-port service request rejected: hello is required and client_id must match the connection".into(),
            );
        }

        let data = match req.op.as_str() {
            "hello" => {
                if bound_client_id.is_some() {
                    return Response::err(
                        id,
                        "virtual-port service hello may only be sent once per connection".into(),
                    );
                }
                let version = req
                    .payload
                    .get("protocol_version")
                    .and_then(serde_json::Value::as_u64);
                if version != Some(SERVICE_PROTOCOL_VERSION) {
                    return Response::err(
                        id,
                        format!(
                            "service protocol mismatch (expected {SERVICE_PROTOCOL_VERSION}, got {})",
                            version
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "missing".into())
                        ),
                    );
                }

                let adopted = match vpm.adopt_owned_endpoints_for_owner(client_pid) {
                    Ok(adopted) => adopted,
                    Err(error) => return Response::err(id, error),
                };
                *endpoints = adopted.clone();
                *bound_client_id = Some(req.client_id.clone());
                Some(serde_json::json!({
                    "protocol_version": SERVICE_PROTOCOL_VERSION,
                    "adopted_endpoints": adopted,
                }))
            }
            "status" => Some(serde_json::json!({
                "files_present": vpm.are_files_present(),
                "driver_installed": vpm.detect_driver(),
                "orphan_count": vpm.pending_orphan_count(),
            })),
            "install_driver" => match vpm.install_driver() {
                Ok(()) => Some(serde_json::json!({})),
                Err(error) => return Response::err(id, error),
            },
            "create_endpoints" => {
                let Some(count) = req.payload.get("count").and_then(serde_json::Value::as_u64)
                else {
                    return Response::err(id, "missing 'count'".into());
                };
                if !(1..=4).contains(&count) {
                    return Response::err(
                        id,
                        format!("virtual endpoint count must be in 1..=4, got {count}"),
                    );
                }
                let config = VirtualPortConfig {
                    enabled: true,
                    count: count as u32,
                };
                match vpm.ensure_endpoints_for_owner(&config, client_pid) {
                    Ok(pairs) => {
                        for pair in &pairs {
                            endpoints.retain(|existing| existing.resource_id != pair.resource_id);
                            endpoints.push(pair.clone());
                        }
                        match serde_json::to_value(&pairs) {
                            Ok(value) => Some(value),
                            Err(error) => {
                                return Response::err(
                                    id,
                                    format!(
                                        "failed to serialize created virtual endpoints: {error}"
                                    ),
                                );
                            }
                        }
                    }
                    Err(error) => return Response::err(id, error.to_string()),
                }
            }
            "remove_pair" => {
                let Some(bus) = req
                    .payload
                    .get("bus")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    return Response::err(id, "missing or invalid 'bus'".into());
                };
                let Some(position) = endpoints
                    .iter()
                    .position(|endpoint| endpoint.resource_id == bus)
                else {
                    return Response::err(
                        id,
                        format!("virtual endpoint bus {bus} is not owned by this connection"),
                    );
                };
                let endpoint = endpoints[position].clone();
                if let Err(error) = vpm.destroy_endpoint(&endpoint) {
                    return Response::err(id, error);
                }
                endpoints.remove(position);
                Some(serde_json::json!({}))
            }
            "cleanup_orphans" => match vpm.cleanup_orphans() {
                Ok(cleaned) => Some(serde_json::json!({ "cleaned": cleaned })),
                Err(error) => return Response::err(id, error),
            },
            other => return Response::err(id, format!("unknown op: {other}")),
        };
        Response {
            id,
            ok: true,
            data,
            error: None,
        }
    }

    fn handle_client(pipe: HANDLE, vpm: &Arc<Mutex<VirtualPortManager>>, client_pid: u32) {
        let mut client_id = None;
        let mut endpoints = Vec::new();

        while let Some(frame) = read_frame(pipe) {
            let req: Request = match serde_json::from_slice(&frame) {
                Ok(request) => request,
                Err(error) => {
                    log::warn!("invalid TauTermService request frame: {error}");
                    break;
                }
            };
            let response = match vpm.lock() {
                Ok(mut manager) => dispatch(
                    &mut manager,
                    &mut endpoints,
                    &mut client_id,
                    &req,
                    client_pid,
                ),
                Err(error) => Response::err(
                    req.id,
                    format!("virtual-port manager lock poisoned: {error}"),
                ),
            };
            let body = match serde_json::to_vec(&response) {
                Ok(body) => body,
                Err(error) => {
                    log::warn!("failed to serialize TauTermService response: {error}");
                    break;
                }
            };
            if !write_frame(pipe, &body) {
                break;
            }
        }

        // A broken pipe is not proof that the GUI process is gone: ServiceBackend may reconnect
        // while this server thread is unwinding. Keep the endpoint active and attach cleanup to
        // the authenticated process handle. Reconnects can safely re-adopt the same identity;
        // cleanup happens only after that exact process exits.
        if !endpoints.is_empty() {
            log::info!(
                "service connection closed for GUI PID {}; preserving {} virtual endpoint(s) until process exit",
                client_pid,
                endpoints.len()
            );
            schedule_disconnect_cleanup(Arc::clone(vpm), client_pid, endpoints);
        }
    }

    /// 停止监视线程：SHUTDOWN 置位后自连接管道，唤醒阻塞中的 ConnectNamedPipe。
    ///
    /// 连接进程即服务自身，会被 `verify_client` 以文件名不匹配拒绝，随后主循环
    /// 回到顶部检查 SHUTDOWN 并退出，服务正常进入 STOPPED。
    fn spawn_shutdown_unblocker() -> std::thread::JoinHandle<()> {
        std::thread::spawn(|| {
            while !SHUTDOWN.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            // SHUTDOWN 已置位：连接一次管道使挂起的 ConnectNamedPipe 返回。
            // 若管道正被占用（服务在处理客户端），CreateFileW 返回
            // ERROR_PIPE_BUSY，稍后重试直到成功。
            loop {
                let name = wide(PIPE_NAME);
                let handle = unsafe {
                    CreateFileW(
                        name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        std::ptr::null_mut(),
                        OPEN_EXISTING,
                        0,
                        std::ptr::null_mut(),
                    )
                };
                if handle != INVALID_HANDLE_VALUE {
                    unsafe { CloseHandle(handle) };
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
    }

    fn run_server(resource_dir: PathBuf) {
        // 服务的 ownership 簿记属于机器级特权状态，存放在 ProgramData，而不是
        // 安装目录或某个交互用户的 AppData。它只记录 TauTerm 自己创建的 endpoint，
        // 因而服务崩溃/掉电后仍能安全恢复，且不会把第三方 com0com bus 当作孤儿。
        let state_dir = match tauterm_lib::virtual_port::windows_state::ensure_ownership_state_dir()
        {
            Ok(state_dir) => state_dir,
            Err(error) => {
                log::error!("cannot secure privileged virtual-port ownership state: {error}");
                return;
            }
        };
        let vpm = Arc::new(Mutex::new(VirtualPortManager::new_privileged(
            resource_dir,
            state_dir,
        )));

        // 启动时只清理有 TauTerm ownership 证据、且当前无 active owner 的资源。
        if let Ok(mut v) = vpm.lock() {
            match v.cleanup_orphans() {
                Ok(cleaned) if cleaned > 0 => {
                    log::info!("startup: cleaned {} orphan port pair(s)", cleaned);
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("startup orphan cleanup failed: {error}");
                }
            }
        }

        // 停止监视线程：SHUTDOWN 置位后连接一次管道，使阻塞中的 ConnectNamedPipe
        // 返回（连接进程即服务自身，verify_client 会因文件名不匹配拒绝它），
        // 从而让 `sc stop` / 系统关机 / 卸载不被无客户端时的无限阻塞卡住。
        let unblocker = spawn_shutdown_unblocker();

        loop {
            if SHUTDOWN.load(Ordering::SeqCst) {
                break;
            }
            let pipe = match create_pipe() {
                Ok(pipe) => pipe,
                Err(error) => {
                    log::error!("TauTermService pipe creation failed: {error}");
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }
            };

            // Block only the accept loop. Each verified GUI connection is served on its own
            // thread, while com0com mutations remain serialized by VirtualPortManager + the global
            // driver mutation mutex. This prevents one long-lived TauTerm instance from forcing
            // every other instance into the direct-UAC fallback.
            let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
            if connected == 0 {
                let error = unsafe { GetLastError() };
                if error != ERROR_PIPE_CONNECTED {
                    unsafe { CloseHandle(pipe) };
                    continue;
                }
            }

            if SHUTDOWN.load(Ordering::SeqCst) {
                unsafe { CloseHandle(pipe) };
                break;
            }

            let Some(client_pid) = verify_client(pipe) else {
                unsafe {
                    DisconnectNamedPipe(pipe);
                    CloseHandle(pipe);
                }
                continue;
            };

            let manager = Arc::clone(&vpm);
            let pipe_value = pipe as usize;
            let _ = std::thread::spawn(move || {
                let pipe = pipe_value as HANDLE;
                handle_client(pipe, &manager, client_pid);
                unsafe {
                    DisconnectNamedPipe(pipe);
                    CloseHandle(pipe);
                }
            });
        }
        let _ = unblocker.join();
    }

    // ── 服务生命周期 ───────────────────────────────────

    fn report_status(handle: SERVICE_STATUS_HANDLE, state: u32, wait_hint: u32) {
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: wait_hint,
        };
        unsafe { SetServiceStatus(handle, &status) };
    }

    unsafe extern "system" fn service_handler(
        ctrl: u32,
        _event: u32,
        _data: *mut core::ffi::c_void,
        _ctx: *mut core::ffi::c_void,
    ) -> u32 {
        match ctrl {
            SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
                SHUTDOWN.store(true, Ordering::SeqCst);
                let handle = STATUS_HANDLE.load(Ordering::SeqCst);
                report_status(handle, SERVICE_STOP_PENDING, 1000);
                0 // NO_ERROR
            }
            SERVICE_CONTROL_INTERROGATE => 0,
            _ => 1, // ERROR_CALL_NOT_IMPLEMENTED
        }
    }

    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows_sys::core::PWSTR) {
        let name = wide(SERVICE_NAME);
        let handle = RegisterServiceCtrlHandlerExW(
            name.as_ptr(),
            Some(service_handler),
            std::ptr::null_mut(),
        );
        STATUS_HANDLE.store(handle, Ordering::SeqCst);
        report_status(handle, SERVICE_RUNNING, 0);

        let resource_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        run_server(resource_dir);

        report_status(handle, SERVICE_STOPPED, 0);
    }

    pub fn entry() {
        let _ = env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init();

        let name = wide(SERVICE_NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_ptr() as *mut u16,
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];

        let ok = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
        if ok == 0 {
            // 未在 SCM 下运行（如手工调试），直接以交互方式启动服务器
            eprintln!("Not running as a service; starting interactively");
            let resource_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."));
            run_server(resource_dir);
        }
    }
}

#[cfg(windows)]
fn main() {
    service::entry();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("tauterm-service is only supported on Windows");
}
