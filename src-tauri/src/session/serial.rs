//! 串口会话实现
//!
//! 使用专用 I/O 线程独占串口端口。
//! I/O 线程使用缓冲通道（sync_channel(32)）和公平读写调度。

use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use crate::session::{ConnectionType, EndpointInfo, SessionState, TermSession};
use crate::session::manager::{IoCmd, spawn_io_thread};

/// 最小串口会话（兼容 TermSession trait）
pub struct SerialSession {
    state: SessionState,
}

impl SerialSession {
    pub fn new() -> Self {
        Self {
            state: SessionState::Disconnected,
        }
    }

    /// 创建串口会话
    ///
    /// 打开端口，启动 I/O 线程，返回 (session, write_tx, io_thread, cancel_tx, actual_params)
    pub fn create_session(
        session_id: &str,
        endpoint: &str,
        params: &serde_json::Value,
        on_data: Box<dyn Fn(String, Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn(String) + Send>,
    ) -> Result<(
        SerialSession,
        mpsc::SyncSender<IoCmd>,
        std::thread::JoinHandle<()>,
        tokio::sync::oneshot::Sender<()>,
        serde_json::Value,
    ), String> {
        let actual_params = Self::build_params(params);
        let port = Self::open_port(endpoint, &actual_params)?;

        let (write_tx, write_rx) = mpsc::sync_channel::<IoCmd>(32);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let sid = session_id.to_string();
        let io_handle = spawn_io_thread(port, sid, on_data, on_disconnect, write_rx, cancel_rx);

        Ok((
            SerialSession { state: SessionState::Connected },
            write_tx,
            io_handle,
            cancel_tx,
            actual_params,
        ))
    }

    /// 构建串口参数（从 JSON Value 提取，使用默认值填充）
    fn build_params(params: &serde_json::Value) -> serde_json::Value {
        let baud_rate = params.get("baud_rate").and_then(|v| v.as_u64()).unwrap_or(115200);
        let data_bits = params.get("data_bits").and_then(|v| v.as_u64()).unwrap_or(8);
        let parity = params.get("parity").and_then(|v| v.as_str()).unwrap_or("none");
        let stop_bits = params.get("stop_bits").and_then(|v| v.as_str()).unwrap_or("1");
        let flow_control = params.get("flow_control").and_then(|v| v.as_str()).unwrap_or("none");

        serde_json::json!({
            "baud_rate": baud_rate,
            "data_bits": data_bits,
            "parity": parity,
            "stop_bits": stop_bits,
            "flow_control": flow_control,
        })
    }

    /// 打开串口端口
    ///
    /// 通过最多 3 次重试（每次间隔 100ms）处理 Windows COM 端口句柄释放时机问题。
    /// 断开后立即重连时，操作系统可能尚未完全释放端口句柄，短暂延迟后重试可解决此问题。
    /// 打开成功后清空缓冲区以确保端口处于可用状态。
    fn open_port(endpoint: &str, params: &serde_json::Value) -> Result<Box<dyn serialport::SerialPort>, String> {
        let baud_rate = params.get("baud_rate").and_then(|v| v.as_u64()).unwrap_or(115200) as u32;
        let dbv = params.get("data_bits").and_then(|v| v.as_u64()).unwrap_or(8) as u8;
        let ps = params.get("parity").and_then(|v| v.as_str()).unwrap_or("none");
        let sbs = params.get("stop_bits").and_then(|v| v.as_str()).unwrap_or("1");
        let fcs = params.get("flow_control").and_then(|v| v.as_str()).unwrap_or("none");

        let db = match dbv { 5=>serialport::DataBits::Five,6=>serialport::DataBits::Six,7=>serialport::DataBits::Seven,_=>serialport::DataBits::Eight };
        let pa = match ps { "even"=>serialport::Parity::Even,"odd"=>serialport::Parity::Odd,_=>serialport::Parity::None };
        let sb = match sbs { "2"=>serialport::StopBits::Two,_=>serialport::StopBits::One };
        let fc = match fcs { "rts_cts"=>serialport::FlowControl::Hardware,"xon_xoff"=>serialport::FlowControl::Software,_=>serialport::FlowControl::None };

        let mut last_err = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match serialport::new(endpoint, baud_rate)
                .data_bits(db).parity(pa).stop_bits(sb).flow_control(fc)
                .timeout(Duration::from_millis(50))
                .open()
            {
                Ok(port) => {
                    // 清空缓冲区，丢弃上次连接残留的数据
                    let _ = port.clear(serialport::ClearBuffer::All);
                    // 短暂等待设备稳定
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    return Ok(port);
                }
                Err(e) => {
                    last_err = format!("无法打开端口 {}: {}", endpoint, e);
                }
            }
        }
        Err(last_err)
    }

    // ── YModem 文件传输 ────────────────────────────

    /// 临时打开专用端口用于 YModem 传输
    pub fn open_port_for_transfer(endpoint: &str, params: &serde_json::Value) -> Result<Box<dyn serialport::SerialPort>, String> {
        Self::open_port(endpoint, params)
    }

    /// 清空端口接收缓冲区
    ///
    /// 丢弃设备残留输出，避免干扰 YModem 握手协议。
    /// 连续读取直到连续 3 次超时（50ms 每次），确保缓冲区清空。
    pub fn flush_port_buffer(port: &mut Box<dyn serialport::SerialPort>) {
        let mut buf = [0u8; 256];
        let mut empty_count = 0;
        for _ in 0..20 {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => { empty_count = 0; }
                _ => {
                    empty_count += 1;
                    if empty_count >= 3 { break; }
                }
            }
        }
    }

    /// YModem 发送文件
    pub fn ymodem_send(
        port: &mut Box<dyn serialport::SerialPort>,
        app: AppHandle,
        file_paths: Vec<String>,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        use crate::transfer::ymodem::YModemSender;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let cancelled = Arc::new(AtomicBool::new(false));
        let c = cancelled.clone();
        std::thread::spawn(move || { let _ = cancel_rx.blocking_recv(); c.store(true, Ordering::SeqCst); });
        let cancel_fn = &mut || cancelled.load(Ordering::SeqCst);
        let ac = app.clone();
        YModemSender::send(port, &file_paths,
            move |p| { let _ = ac.emit("transfer-progress", serde_json::json!({"file_name":p.file_name,"bytes_transferred":p.bytes_transferred,"total_bytes":p.total_bytes,"direction":"send"})); },
            cancel_fn,
        ).map_err(|e| e.to_string())?;
        let _ = app.emit("transfer-complete", serde_json::json!({"success":true,"files":file_paths.len()}));
        Ok(())
    }

    /// YModem 接收文件
    pub fn ymodem_receive(
        port: &mut Box<dyn serialport::SerialPort>,
        app: AppHandle,
        download_dir: String,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        use crate::transfer::ymodem::YModemReceiver;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let cancelled = Arc::new(AtomicBool::new(false));
        let c = cancelled.clone();
        std::thread::spawn(move || { let _ = cancel_rx.blocking_recv(); c.store(true, Ordering::SeqCst); });
        let cancel_fn = &mut || cancelled.load(Ordering::SeqCst);
        let ac = app.clone();
        YModemReceiver::receive(port, &download_dir,
            move |p| { let _ = ac.emit("transfer-progress", serde_json::json!({"file_name":p.file_name,"bytes_transferred":p.bytes_transferred,"total_bytes":p.total_bytes,"direction":"receive"})); },
            cancel_fn,
        ).map_err(|e| e.to_string())?;
        let _ = app.emit("transfer-complete", serde_json::json!({"success":true,"message":"接收完成"}));
        Ok(())
    }
}

// ── TermSession trait 实现 ────────────────────────
// 注意: 多会话架构中，SerialSession 的 TermSession trait 实现保留用于兼容性，
// 实际会话管理通过 SessionManager 完成。

impl TermSession for SerialSession {
    fn enumerate_endpoints(&self) -> Result<Vec<EndpointInfo>, String> {
        serialport::available_ports().map_err(|e| e.to_string()).map(|ports|
            ports.into_iter().map(|p| EndpointInfo {
                name: p.port_name.clone(),
                description: p.port_name,
                connection_type: ConnectionType::Serial,
            }).collect()
        )
    }

    fn connect(
        &mut self,
        _endpoint: &str,
        _params: serde_json::Value,
        _on_data: Box<dyn Fn(Vec<u8>) + Send>,
        _on_disconnect: Box<dyn Fn() + Send>,
    ) -> Result<(), String> {
        Err("请使用 SessionManager::create_session() 创建会话".into())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        self.state = SessionState::Disconnected;
        Ok(())
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("请使用 SessionManager::write() 写入数据".into())
    }

    fn state(&self) -> SessionState { self.state.clone() }
    fn connection_type(&self) -> ConnectionType { ConnectionType::Serial }
}

impl Drop for SerialSession {
    fn drop(&mut self) { let _ = self.disconnect(); }
}
