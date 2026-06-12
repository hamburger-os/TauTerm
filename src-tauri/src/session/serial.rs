//! 串口会话实现
//!
//! 将串口连接封装为 `TermSession` trait 实现。

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use crate::session::{ConnectionType, EndpointInfo, SessionState, TermSession};
use crate::transfer::ymodem::{YModemSender, YModemReceiver};

/// 共享串口句柄
type SharedPort = Arc<Mutex<Option<Box<dyn serialport::SerialPort>>>>;

/// 串口会话
///
/// 实现 `TermSession` trait，管理串口的枚举、连接和读写。
pub struct SerialSession {
    port: SharedPort,
    state: SessionState,
    read_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    transfer_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl SerialSession {
    pub fn new() -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            state: SessionState::Disconnected,
            read_cancel_tx: None,
            transfer_cancel_tx: None,
        }
    }

    /// 启动后台读取任务
    fn start_read_loop(
        port: SharedPort,
        on_data: Box<dyn Fn(Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn() + Send>,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if cancel_rx.try_recv().is_ok() {
                    break;
                }

                let read_result = {
                    let mut guard = match port.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    match guard.as_mut() {
                        Some(p) => p.read(&mut buf),
                        None => break,
                    }
                };

                match read_result {
                    Ok(n) if n > 0 => {
                        on_data(buf[..n].to_vec());
                    }
                    Ok(_) => {
                        // 超时或无数据
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => {
                        on_disconnect();
                        break;
                    }
                }
            }
        });
    }

    /// YModem 发送文件
    pub fn ymodem_send(
        port: SharedPort,
        app: AppHandle,
        file_paths: Vec<String>,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let mut guard = port.lock().map_err(|e| format!("锁错误: {}", e))?;
        let serial_port = guard.as_mut().ok_or("串口未打开")?;

        let mut cancelled = false;
        let cancel_fn = &mut || {
            if cancelled { return true; }
            cancelled = cancel_rx.try_recv().is_ok();
            cancelled
        };

        let app_clone = app.clone();
        let on_progress = move |p: crate::transfer::protocol::TransferProgress| {
            let _ = app_clone.emit("transfer-progress", serde_json::json!({
                "file_name": p.file_name,
                "bytes_transferred": p.bytes_transferred,
                "total_bytes": p.total_bytes,
                "direction": "send",
            }));
        };

        YModemSender::send(serial_port, &file_paths, on_progress, cancel_fn)
            .map_err(|e| e.to_string())?;

        let _ = app.emit("transfer-complete", serde_json::json!({
            "success": true,
            "files": file_paths.len(),
        }));
        Ok(())
    }

    /// YModem 接收文件
    pub fn ymodem_receive(
        port: SharedPort,
        app: AppHandle,
        download_dir: String,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let mut guard = port.lock().map_err(|e| format!("锁错误: {}", e))?;
        let serial_port = guard.as_mut().ok_or("串口未打开")?;

        let mut cancelled = false;
        let cancel_fn = &mut || {
            if cancelled { return true; }
            cancelled = cancel_rx.try_recv().is_ok();
            cancelled
        };

        let app_clone = app.clone();
        let on_progress = move |p: crate::transfer::protocol::TransferProgress| {
            let _ = app_clone.emit("transfer-progress", serde_json::json!({
                "file_name": p.file_name,
                "bytes_transferred": p.bytes_transferred,
                "total_bytes": p.total_bytes,
                "direction": "receive",
            }));
        };

        YModemReceiver::receive(serial_port, &download_dir, on_progress, cancel_fn)
            .map_err(|e| e.to_string())?;

        let _ = app.emit("transfer-complete", serde_json::json!({
            "success": true,
            "message": "接收完成",
        }));
        Ok(())
    }

    /// 获取端口共享引用（供 YModem 命令使用）
    pub fn port_handle(&self) -> SharedPort {
        self.port.clone()
    }

    /// 获取取消信号发送端
    pub fn take_cancel_tx(&mut self) -> Option<tokio::sync::oneshot::Sender<()>> {
        self.transfer_cancel_tx.take()
    }

    /// 设置取消信号发送端
    pub fn set_cancel_tx(&mut self, tx: tokio::sync::oneshot::Sender<()>) {
        self.transfer_cancel_tx = Some(tx);
    }
}

impl TermSession for SerialSession {
    fn enumerate_endpoints(&self) -> Result<Vec<EndpointInfo>, String> {
        let ports = serialport::available_ports().map_err(|e| e.to_string())?;
        Ok(ports
            .into_iter()
            .map(|p| EndpointInfo {
                name: p.port_name.clone(),
                description: p.port_name,
                connection_type: ConnectionType::Serial,
            })
            .collect())
    }

    fn connect(
        &mut self,
        endpoint: &str,
        params: serde_json::Value,
        on_data: Box<dyn Fn(Vec<u8>) + Send>,
        on_disconnect: Box<dyn Fn() + Send>,
    ) -> Result<(), String> {
        // 解析参数
        let baud_rate = params.get("baud_rate").and_then(|v| v.as_u64()).unwrap_or(115200) as u32;
        let data_bits_val = params.get("data_bits").and_then(|v| v.as_u64()).unwrap_or(8) as u8;
        let parity_str = params.get("parity").and_then(|v| v.as_str()).unwrap_or("none");
        let stop_bits_str = params.get("stop_bits").and_then(|v| v.as_str()).unwrap_or("1");
        let flow_control_str = params.get("flow_control").and_then(|v| v.as_str()).unwrap_or("none");

        let data_bits = match data_bits_val {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let parity = match parity_str {
            "even" => serialport::Parity::Even,
            "odd" => serialport::Parity::Odd,
            _ => serialport::Parity::None,
        };
        let stop_bits = match stop_bits_str {
            "2" => serialport::StopBits::Two,
            _ => serialport::StopBits::One,
        };
        let flow_control = match flow_control_str {
            "rts_cts" => serialport::FlowControl::Hardware,
            "xon_xoff" => serialport::FlowControl::Software,
            _ => serialport::FlowControl::None,
        };

        self.state = SessionState::Connecting;

        let port = serialport::new(endpoint, baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .flow_control(flow_control)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("无法打开端口 {}: {}", endpoint, e))?;

        {
            let mut guard = self.port.lock().map_err(|e| format!("锁错误: {}", e))?;
            *guard = Some(port);
        }

        self.state = SessionState::Connected;

        // 启动后台读取
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        self.read_cancel_tx = Some(cancel_tx);

        Self::start_read_loop(
            self.port.clone(),
            on_data,
            on_disconnect,
            cancel_rx,
        );

        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(tx) = self.read_cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.transfer_cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Ok(mut guard) = self.port.lock() {
            *guard = None;
        }
        self.state = SessionState::Disconnected;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut guard = self.port.lock().map_err(|e| format!("锁错误: {}", e))?;
        match guard.as_mut() {
            Some(port) => {
                port.write_all(data).map_err(|e| e.to_string())?;
                Ok(())
            }
            None => Err("串口未打开".into()),
        }
    }

    fn state(&self) -> SessionState {
        self.state.clone()
    }

    fn connection_type(&self) -> ConnectionType {
        ConnectionType::Serial
    }
}

impl Drop for SerialSession {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
