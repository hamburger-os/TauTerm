//! Windows com0com bridge I/O.
//!
//! This module deliberately does not use `serialport::try_clone`. A bridge endpoint is opened once
//! with FILE_FLAG_OVERLAPPED, moved to one Endpoint Actor, and all read/write/modem-status activity
//! is serialized by that actor. WaitCommEvent provides event-driven peer/RX/error notification;
//! ReadFile and WriteFile use independent OVERLAPPED records on the same actor-owned handle.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::time::Duration;

use windows_sys::Win32::Devices::Communication::{
    BuildCommDCBA, ClearCommError, GetCommModemStatus, GetCommState, SetCommMask, SetCommState,
    WaitCommEvent, COMSTAT, DCB, EV_DSR, EV_ERR, EV_RXCHAR, MS_DSR_ON,
};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE,
    INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, ResetEvent, WaitForMultipleObjects, WaitForSingleObject,
};

const READ_BUFFER_BYTES: usize = 4096;

pub struct WindowsBridgeHandle {
    handle: OwnedHandle,
}

impl WindowsBridgeHandle {
    pub fn open(name: &str, baud_rate: u32) -> Result<Self, String> {
        let path = format!(r"\\.\{name}");
        let wide = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_NONE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "failed to open virtual endpoint {name} for overlapped I/O (Win32 {})",
                unsafe { GetLastError() }
            ));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle as _) };

        configure_serial(handle.as_raw_handle() as _, baud_rate)
            .map_err(|error| format!("failed to configure virtual endpoint {name}: {error}"))?;

        Ok(Self { handle })
    }

    pub fn into_io(self) -> Result<Box<WindowsBridgeIo>, String> {
        WindowsBridgeIo::new(self.handle)
    }
}

fn configure_serial(
    handle: windows_sys::Win32::Foundation::HANDLE,
    baud_rate: u32,
) -> Result<(), String> {
    let mut dcb: DCB = unsafe { std::mem::zeroed() };
    dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
    if unsafe { GetCommState(handle, &mut dcb) } == 0 {
        return Err(format!("GetCommState failed (Win32 {})", unsafe {
            GetLastError()
        }));
    }

    let definition = std::ffi::CString::new(format!("baud={baud_rate} parity=N data=8 stop=1"))
        .map_err(|error| error.to_string())?;
    if unsafe { BuildCommDCBA(definition.as_ptr() as _, &mut dcb) } == 0 {
        return Err(format!("BuildCommDCBA failed (Win32 {})", unsafe {
            GetLastError()
        }));
    }
    if unsafe { SetCommState(handle, &dcb) } == 0 {
        return Err(format!("SetCommState failed (Win32 {})", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}

pub struct PollResult {
    pub peer_open: Option<bool>,
    pub peer_status_error: Option<String>,
    pub rx_ready: bool,
    pub line_error: Option<u32>,
    pub read_data: Option<Vec<u8>>,
    pub write_completed: Option<usize>,
}

impl PollResult {
    fn empty() -> Self {
        Self {
            peer_open: None,
            peer_status_error: None,
            rx_ready: false,
            line_error: None,
            read_data: None,
            write_completed: None,
        }
    }
}

pub struct WindowsBridgeIo {
    handle: OwnedHandle,
    comm_event: OwnedHandle,
    read_event: OwnedHandle,
    write_event: OwnedHandle,
    comm_overlapped: OVERLAPPED,
    read_overlapped: OVERLAPPED,
    write_overlapped: OVERLAPPED,
    comm_mask: u32,
    comm_pending: bool,
    read_buffer: Box<[u8; READ_BUFFER_BYTES]>,
    read_pending: bool,
    write_pending: bool,
    write_buffer: Option<Arc<[u8]>>,
    write_offset: usize,
}

impl WindowsBridgeIo {
    fn new(handle: OwnedHandle) -> Result<Box<Self>, String> {
        let comm_event = create_manual_event("comm")?;
        let read_event = create_manual_event("read")?;
        let write_event = create_manual_event("write")?;
        let mut this = Box::new(Self {
            handle,
            comm_overlapped: overlapped_for(&comm_event),
            read_overlapped: overlapped_for(&read_event),
            write_overlapped: overlapped_for(&write_event),
            comm_event,
            read_event,
            write_event,
            comm_mask: 0,
            comm_pending: false,
            read_buffer: Box::new([0u8; READ_BUFFER_BYTES]),
            read_pending: false,
            write_pending: false,
            write_buffer: None,
            write_offset: 0,
        });
        if unsafe { SetCommMask(this.raw_handle(), EV_DSR | EV_ERR | EV_RXCHAR) } == 0 {
            return Err(format!("SetCommMask failed (Win32 {})", unsafe {
                GetLastError()
            }));
        }
        this.arm_comm_wait()?;
        Ok(this)
    }

    fn raw_handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.handle.as_raw_handle() as _
    }

    pub fn peer_is_open(&self) -> Result<bool, String> {
        let mut status = 0u32;
        if unsafe { GetCommModemStatus(self.raw_handle(), &mut status) } == 0 {
            return Err(format!("GetCommModemStatus failed (Win32 {})", unsafe {
                GetLastError()
            }));
        }
        Ok((status & MS_DSR_ON) != 0)
    }

    pub fn write_pending(&self) -> bool {
        self.write_pending
    }

    pub fn read_pending(&self) -> bool {
        self.read_pending
    }

    pub fn start_write(&mut self, data: Arc<[u8]>, offset: usize) -> Result<(), String> {
        if self.write_pending {
            return Err("overlapped virtual-port write already pending".into());
        }
        if offset >= data.len() {
            return Ok(());
        }

        self.write_buffer = Some(data);
        self.write_offset = offset;
        reset_overlapped(&mut self.write_overlapped, &self.write_event)?;
        let buffer = self
            .write_buffer
            .as_ref()
            .expect("write buffer stored before overlapped write");
        let remaining = &buffer[self.write_offset..];
        let result = unsafe {
            WriteFile(
                self.raw_handle(),
                remaining.as_ptr(),
                remaining.len().min(u32::MAX as usize) as u32,
                std::ptr::null_mut(),
                &mut self.write_overlapped,
            )
        };
        self.write_pending = true;
        if result == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                self.write_pending = false;
                self.write_buffer = None;
                self.write_offset = 0;
                return Err(format!("WriteFile failed (Win32 {error})"));
            }
        }
        Ok(())
    }

    pub fn start_read(&mut self, max_bytes: usize) -> Result<(), String> {
        if self.read_pending || max_bytes == 0 {
            return Ok(());
        }

        reset_overlapped(&mut self.read_overlapped, &self.read_event)?;
        let requested = max_bytes.min(self.read_buffer.len()).min(u32::MAX as usize) as u32;
        let result = unsafe {
            ReadFile(
                self.raw_handle(),
                self.read_buffer.as_mut_ptr(),
                requested,
                std::ptr::null_mut(),
                &mut self.read_overlapped,
            )
        };
        self.read_pending = true;
        if result == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                self.read_pending = false;
                return Err(format!("ReadFile failed (Win32 {error})"));
            }
        }
        Ok(())
    }

    pub fn input_available(&self) -> Result<(usize, u32), String> {
        let mut errors = 0u32;
        let mut status: COMSTAT = unsafe { std::mem::zeroed() };
        if unsafe { ClearCommError(self.raw_handle(), &mut errors, &mut status) } == 0 {
            return Err(format!("ClearCommError failed (Win32 {})", unsafe {
                GetLastError()
            }));
        }
        Ok((status.cbInQue as usize, errors))
    }

    pub fn poll(&mut self, timeout: Duration) -> Result<PollResult, String> {
        self.arm_comm_wait()?;

        let mut handles = Vec::with_capacity(3);
        handles.push(self.comm_event.as_raw_handle() as _);
        if self.read_pending {
            handles.push(self.read_event.as_raw_handle() as _);
        }
        if self.write_pending {
            handles.push(self.write_event.as_raw_handle() as _);
        }

        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait = unsafe {
            WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout_ms)
        };
        if wait == WAIT_FAILED {
            return Err(format!(
                "WaitForMultipleObjects failed (Win32 {})",
                unsafe { GetLastError() }
            ));
        }
        if wait != WAIT_TIMEOUT
            && (wait < WAIT_OBJECT_0 || wait >= WAIT_OBJECT_0 + handles.len() as u32)
        {
            return Err(format!("unexpected wait result {wait}"));
        }

        let mut result = PollResult::empty();
        if is_signaled(&self.comm_event)? {
            self.complete_comm(&mut result)?;
        }
        if self.read_pending && is_signaled(&self.read_event)? {
            result.read_data = self.complete_read()?;
        }
        if self.write_pending && is_signaled(&self.write_event)? {
            result.write_completed = self.complete_write()?;
        }
        Ok(result)
    }

    pub fn cancel_read(&mut self) -> Result<(), String> {
        if !self.read_pending {
            return Ok(());
        }
        cancel_and_drain(self.raw_handle(), &self.read_overlapped, "read")?;
        self.read_pending = false;
        Ok(())
    }

    pub fn cancel_write(&mut self) -> Result<(), String> {
        if !self.write_pending {
            return Ok(());
        }
        cancel_and_drain(self.raw_handle(), &self.write_overlapped, "write")?;
        self.write_pending = false;
        self.write_buffer = None;
        self.write_offset = 0;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        unsafe {
            let _ = CancelIoEx(self.raw_handle(), std::ptr::null());
        }
        let _ = self.cancel_read();
        let _ = self.cancel_write();
        if self.comm_pending {
            let _ = cancel_and_drain(self.raw_handle(), &self.comm_overlapped, "comm");
            self.comm_pending = false;
        }
        self.comm_mask = 0;
    }

    fn arm_comm_wait(&mut self) -> Result<(), String> {
        if self.comm_pending {
            return Ok(());
        }
        reset_overlapped(&mut self.comm_overlapped, &self.comm_event)?;
        let result = unsafe {
            WaitCommEvent(
                self.raw_handle(),
                &mut self.comm_mask,
                &mut self.comm_overlapped,
            )
        };
        if result == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                self.comm_mask = 0;
                return Err(format!("WaitCommEvent failed (Win32 {error})"));
            }
        }
        self.comm_pending = true;
        Ok(())
    }

    fn complete_comm(&mut self, result: &mut PollResult) -> Result<(), String> {
        let mut transferred = 0u32;
        let ok = unsafe {
            GetOverlappedResult(
                self.raw_handle(),
                &self.comm_overlapped,
                &mut transferred,
                0,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            self.comm_pending = false;
            self.comm_mask = 0;
            if error == ERROR_OPERATION_ABORTED {
                return Ok(());
            }
            return Err(format!("WaitCommEvent completion failed (Win32 {error})"));
        }

        let mask = self.comm_mask;
        self.comm_pending = false;
        self.comm_mask = 0;
        result.rx_ready = (mask & EV_RXCHAR) != 0;
        if (mask & EV_ERR) != 0 {
            let (_, errors) = self.input_available()?;
            if errors != 0 {
                result.line_error = Some(errors);
            }
        }
        if (mask & EV_DSR) != 0 {
            match self.peer_is_open() {
                Ok(open) => result.peer_open = Some(open),
                Err(error) => result.peer_status_error = Some(error),
            }
        }
        self.arm_comm_wait()?;
        Ok(())
    }

    fn complete_read(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut transferred = 0u32;
        let ok = unsafe {
            GetOverlappedResult(
                self.raw_handle(),
                &self.read_overlapped,
                &mut transferred,
                0,
            )
        };
        self.read_pending = false;
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_OPERATION_ABORTED {
                return Ok(None);
            }
            return Err(format!("ReadFile completion failed (Win32 {error})"));
        }
        if transferred == 0 {
            return Ok(None);
        }
        Ok(Some(self.read_buffer[..transferred as usize].to_vec()))
    }

    fn complete_write(&mut self) -> Result<Option<usize>, String> {
        let mut transferred = 0u32;
        let ok = unsafe {
            GetOverlappedResult(
                self.raw_handle(),
                &self.write_overlapped,
                &mut transferred,
                0,
            )
        };
        self.write_pending = false;
        self.write_buffer = None;
        self.write_offset = 0;
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_OPERATION_ABORTED {
                return Ok(None);
            }
            return Err(format!("WriteFile completion failed (Win32 {error})"));
        }
        Ok(Some(transferred as usize))
    }
}

impl Drop for WindowsBridgeIo {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn create_manual_event(kind: &str) -> Result<OwnedHandle, String> {
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if handle.is_null() {
        return Err(format!("CreateEventW({kind}) failed (Win32 {})", unsafe {
            GetLastError()
        }));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

fn overlapped_for(event: &OwnedHandle) -> OVERLAPPED {
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.as_raw_handle() as _;
    overlapped
}

fn reset_overlapped(overlapped: &mut OVERLAPPED, event: &OwnedHandle) -> Result<(), String> {
    if unsafe { ResetEvent(event.as_raw_handle() as _) } == 0 {
        return Err(format!("ResetEvent failed (Win32 {})", unsafe {
            GetLastError()
        }));
    }
    *overlapped = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.as_raw_handle() as _;
    Ok(())
}

fn is_signaled(event: &OwnedHandle) -> Result<bool, String> {
    let wait = unsafe { WaitForSingleObject(event.as_raw_handle() as _, 0) };
    if wait == WAIT_OBJECT_0 {
        Ok(true)
    } else if wait == WAIT_TIMEOUT {
        Ok(false)
    } else {
        Err(format!(
            "WaitForSingleObject failed/result={wait} (Win32 {})",
            unsafe { GetLastError() }
        ))
    }
}

fn cancel_and_drain(
    handle: windows_sys::Win32::Foundation::HANDLE,
    overlapped: &OVERLAPPED,
    kind: &str,
) -> Result<(), String> {
    unsafe {
        let _ = CancelIoEx(handle, overlapped);
    }
    let mut transferred = 0u32;
    let ok = unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 1) };
    if ok != 0 {
        return Ok(());
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_OPERATION_ABORTED {
        Ok(())
    } else {
        Err(format!(
            "{kind} cancellation completion failed (Win32 {error})"
        ))
    }
}

