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
//! 客户端以「连接」为单位记账：`hello` 上报 `client_id`，断开（管道关闭）时
//! 自动清理该客户端创建的全部端口对——即使 App 崩溃，OS 也会关闭管道触发清理。

#[cfg(windows)]
mod service {
    use std::collections::HashMap;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
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
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW};

    use tauterm_lib::virtual_port::backend::{PortPair, VirtualPortConfig};
    use tauterm_lib::virtual_port::manager::VirtualPortManager;

    // ── Win32 常量（硬编码避免依赖 feature 导出名差异） ──
    const PIPE_ACCESS_DUPLEX: u32 = 0x3;
    const PIPE_TYPE_BYTE: u32 = 0x0;
    const PIPE_READMODE_BYTE: u32 = 0x0;
    const PIPE_WAIT: u32 = 0x0;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

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

    fn build_security_descriptor() -> *mut core::ffi::c_void {
        // AU=Authenticated Users(读写) SY=SYSTEM(完全) BA=Administrators(完全)
        let sddl = wide("D:(A;;GRGW;;;AU)(A;;GA;;;SY)(A;;GA;;;BA)");
        let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut size = 0u32;
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1, // SDDL_REVISION_1
                &mut sd,
                &mut size,
            )
        };
        if ok == 0 {
            std::ptr::null_mut()
        } else {
            sd
        }
    }

    fn create_pipe(sd: *mut core::ffi::c_void) -> HANDLE {
        let name = wide(PIPE_NAME);
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                &sa,
            )
        }
    }

    fn verify_client(pipe: HANDLE) -> bool {
        let mut pid = 0u32;
        if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 {
            return false;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let client_path = std::path::Path::new(&path);
        let file_name = client_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if file_name != EXPECTED_CLIENT_EXE {
            return false;
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
            (Some(c), Some(s)) if c == s => true,
            (Some(c), Some(s)) => {
                log::warn!(
                    "pipe client rejected: dir mismatch (client={}, service={})",
                    c,
                    s
                );
                false
            }
            _ => false,
        }
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
        if len == 0 || len > 16 * 1024 * 1024 {
            return None;
        }
        let mut data = vec![0u8; len];
        if !read_exact(pipe, &mut data) {
            return None;
        }
        Some(data)
    }

    fn write_frame(pipe: HANDLE, data: &[u8]) -> bool {
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

    type Clients = HashMap<String, Vec<PortPair>>;

    fn dispatch(vpm: &mut VirtualPortManager, clients: &mut Clients, req: &Request) -> Response {
        let id = req.id;
        let data = match req.op.as_str() {
            "hello" => {
                clients.entry(req.client_id.clone()).or_default();
                Some(serde_json::json!({}))
            }
            "status" => Some(serde_json::json!({
                "files_present": vpm.are_files_present(),
                "driver_installed": vpm.detect_driver(),
                "orphan_count": vpm.pending_orphan_count(),
            })),
            "install_driver" => match vpm.install_driver() {
                Ok(()) => Some(serde_json::json!({})),
                Err(e) => return Response::err(id, e),
            },
            "create_pairs" => {
                let count = req
                    .payload
                    .get("count")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(1) as u32;
                let config = VirtualPortConfig {
                    enabled: true,
                    count,
                };
                match vpm.create_pairs(&config) {
                    Ok(pairs) => {
                        let entry = clients.entry(req.client_id.clone()).or_default();
                        for p in &pairs {
                            entry.push(p.clone());
                        }
                        Some(serde_json::to_value(&pairs).unwrap_or_else(|_| serde_json::json!([])))
                    }
                    Err(e) => return Response::err(id, e),
                }
            }
            "remove_pair" => {
                let bus = req
                    .payload
                    .get("bus")
                    .and_then(|b| b.as_u64())
                    .map(|b| b as u32);
                match bus {
                    Some(bus) => {
                        if let Some(list) = clients.get_mut(&req.client_id) {
                            if let Some(pos) = list.iter().position(|p| p.bus_number == bus) {
                                let pair = list.remove(pos);
                                if let Err(e) = vpm.destroy_pair(&pair) {
                                    log::warn!("remove_pair bus {} failed: {}", bus, e);
                                }
                            }
                        }
                        Some(serde_json::json!({}))
                    }
                    None => return Response::err(id, "missing 'bus'".into()),
                }
            }
            "cleanup_client" => {
                if let Some(list) = clients.remove(&req.client_id) {
                    for p in &list {
                        let _ = vpm.destroy_pair(p);
                    }
                }
                Some(serde_json::json!({}))
            }
            other => return Response::err(id, format!("unknown op: {}", other)),
        };
        Response {
            id,
            ok: true,
            data,
            error: None,
        }
    }

    fn handle_client(pipe: HANDLE, vpm: &Mutex<VirtualPortManager>, clients: &Mutex<Clients>) {
        let mut client_id: Option<String> = None;
        while let Some(frame) = read_frame(pipe) {
            let req: Request = match serde_json::from_slice(&frame) {
                Ok(r) => r,
                Err(_) => break,
            };
            if req.op == "hello" {
                client_id = Some(req.client_id.clone());
            }
            let resp = {
                let mut v = vpm.lock().unwrap();
                let mut c = clients.lock().unwrap();
                dispatch(&mut v, &mut c, &req)
            };
            let body = serde_json::to_vec(&resp).unwrap_or_default();
            if !write_frame(pipe, &body) {
                break;
            }
        }

        // 管道关闭 = 客户端消失 → 自动清理其端口对（崩溃也无孤儿）
        if let Some(cid) = client_id {
            if let Ok(mut v) = vpm.lock() {
                if let Ok(mut c) = clients.lock() {
                    if let Some(list) = c.remove(&cid) {
                        for p in &list {
                            let _ = v.destroy_pair(p);
                        }
                    }
                }
            }
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
        // 无状态模式：服务不落盘 com0com 簿记，也不创建 ProgramData 目录。
        // 各客户端端口对在内存中按 client_id 管理，断开/崩溃时自动清理；
        // 孤儿/总线号以驱动真实状态（setupc list）为准，避免卸载残留目录。
        let vpm = Mutex::new(VirtualPortManager::new_stateless(resource_dir));
        let clients: Mutex<Clients> = Mutex::new(HashMap::new());

        // 启动时清理上次异常退出遗留的孤儿端口对
        if let Ok(mut v) = vpm.lock() {
            let cleaned = v.cleanup_orphans();
            if cleaned > 0 {
                log::info!("startup: cleaned {} orphan port pair(s)", cleaned);
            }
        }

        let sd = build_security_descriptor();

        // 停止监视线程：SHUTDOWN 置位后连接一次管道，使阻塞中的 ConnectNamedPipe
        // 返回（连接进程即服务自身，verify_client 会因文件名不匹配拒绝它），
        // 从而让 `sc stop` / 系统关机 / 卸载不被无客户端时的无限阻塞卡住。
        let unblocker = spawn_shutdown_unblocker();

        loop {
            if SHUTDOWN.load(Ordering::SeqCst) {
                break;
            }
            let pipe = create_pipe(sd);
            if pipe == (-1isize) as HANDLE {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }

            // 阻塞等待客户端连接（STOP 到达时由 unblocker 线程自连接唤醒）
            let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
            if connected == 0 {
                let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                if err != ERROR_PIPE_CONNECTED {
                    unsafe { CloseHandle(pipe) };
                    continue;
                }
            }

            if SHUTDOWN.load(Ordering::SeqCst) {
                unsafe { CloseHandle(pipe) };
                break;
            }

            if !verify_client(pipe) {
                unsafe {
                    DisconnectNamedPipe(pipe);
                    CloseHandle(pipe);
                }
                continue;
            }

            handle_client(pipe, &vpm, &clients);
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
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
            // 未在 SCM 下运行（如手动调试），直接以交互方式启动服务器
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
