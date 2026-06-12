//! 串口管理器
//!
//! 管理串口的枚举、打开、关闭、读取和写入操作。

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use crate::serial::config::SerialConfig;

/// 串口连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// 共享的串口句柄
type SharedPort = Arc<Mutex<Option<Box<dyn serialport::SerialPort>>>>;

/// 串口管理器
pub struct SerialPortManager {
    /// 共享的串口句柄
    port: SharedPort,
    /// 连接状态
    state: ConnectionState,
    /// 取消读取循环的信号
    read_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// 取消传输的信号
    transfer_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl SerialPortManager {
    /// 创建新的串口管理器实例
    pub fn new() -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            state: ConnectionState::Disconnected,
            read_cancel_tx: None,
            transfer_cancel_tx: None,
        }
    }

    /// 枚举系统中所有可用的串口（返回端口名称列表）
    pub fn enumerate_ports() -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let ports = serialport::available_ports()?;
        Ok(ports.into_iter().map(|p| p.port_name).collect())
    }

    /// 获取当前连接状态
    pub fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    /// 打开串口连接
    pub fn open(
        &mut self,
        app: AppHandle,
        config: SerialConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 先关闭已有连接
        if self.is_connected() {
            self.close_internal()?;
        }

        self.state = ConnectionState::Connecting;

        // 创建串口构建器
        let port = serialport::new(&config.port_name, config.baud_rate)
            .data_bits(config.data_bits_value())
            .parity(config.parity)
            .stop_bits(config.stop_bits)
            .flow_control(config.flow_control)
            .timeout(Duration::from_millis(50))
            .open()?;

        let port_name = config.port_name.clone();
        let port_arc = self.port.clone();
        port_arc.lock().map_err(|e| format!("锁错误: {}", e))?.replace(port);

        self.state = ConnectionState::Connected;

        // 启动后台读取线程
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        self.read_cancel_tx = Some(cancel_tx);

        let app_for_thread = app.clone();
        let port_for_thread = self.port.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                // 检查取消信号
                if cancel_rx.try_recv().is_ok() {
                    break;
                }

                let read_result = {
                    let mut guard = match port_for_thread.lock() {
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
                        let data = buf[..n].to_vec();
                        let _ = app_for_thread.emit("serial-data", data);
                    }
                    Ok(_) => {
                        // 超时，无数据
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => {
                        // 读取错误（设备可能已拔出）
                        let _ = app_for_thread.emit("serial-disconnected", ());
                        break;
                    }
                }
            }
        });

        let _ = app.emit("serial-connected", port_name);
        Ok(())
    }

    /// 内部关闭（不发送事件）
    fn close_internal(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 发送取消信号停止读取循环
        if let Some(tx) = self.read_cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.transfer_cancel_tx.take() {
            let _ = tx.send(());
        }

        // 关闭并清除端口
        if let Ok(mut guard) = self.port.lock() {
            *guard = None;
        }
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    /// 关闭串口连接
    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.close_internal()
    }

    /// 向串口写入数据
    pub fn write(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut guard = self.port.lock().map_err(|e| format!("锁错误: {}", e))?;
        match guard.as_mut() {
            Some(port) => {
                port.write_all(data)?;
                Ok(())
            }
            None => Err("串口未打开".into()),
        }
    }

    // YModem 方法已迁移至 session/serial.rs
    // 此文件保留用于向后兼容，未来版本移除

    /// 检查串口是否已连接
    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }
}

impl Drop for SerialPortManager {
    fn drop(&mut self) {
        let _ = self.close_internal();
    }
}
