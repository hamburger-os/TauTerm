//! 串口协议插件。
//!
//! Raw Serial 链路由 transport 层负责；虚拟串口是 Serial 插件自己的可选运行时能力。
//! SessionStore 只持有通用 `SessionService`，不感知 com0com、bridge 或虚拟端口类型。

pub const PLUGIN_ID: &str = "serial";

pub(crate) mod commands;

use crate::kernel::plugin_adapter::{
    EndpointInfo, ProtocolAdapter, ProtocolConnection, SessionAttach, SessionService,
};
use crate::kernel::plugin_runtime::SessionRuntimeRegistry;
use crate::session::SessionError;
use crate::transport::serial::{open_serial, SerialTransportConfig};
use crate::transport::DataPlaneRuntime;
use crate::virtual_port::backend::{
    contains_elevation_indicator, is_internal_endpoint_path, VirtualEndpoint, VirtualPortConfig,
};
use crate::virtual_port::bridge::VirtualPortBridge;
use crate::AppState;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// Serial 私有 Session runtime。
///
/// bridge 与端口对只存在于插件内部；SessionStore 通过 `SessionService::shutdown` 驱动统一
/// 生命周期。需要管理员权限才能删除的残留继续由现有显式“清理残留端口”动作处理，避免
/// I/O 异常断连时突然弹出 UAC。
pub struct SerialRuntime {
    session_id: Mutex<Option<String>>,
    app: Mutex<Option<AppHandle>>,
    bridge: Mutex<Option<VirtualPortBridge>>,
    endpoints: Mutex<Vec<VirtualEndpoint>>,
}

impl SerialRuntime {
    fn new() -> Self {
        Self {
            session_id: Mutex::new(None),
            app: Mutex::new(None),
            bridge: Mutex::new(None),
            endpoints: Mutex::new(Vec::new()),
        }
    }

    fn remember_context(&self, app: &AppHandle, session_id: &str) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(app.clone());
        }
        if let Ok(mut slot) = self.session_id.lock() {
            *slot = Some(session_id.to_string());
        }
    }

    fn take_endpoints(&self) -> Vec<VirtualEndpoint> {
        match self.endpoints.lock() {
            Ok(mut endpoints) => std::mem::take(&mut *endpoints),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    fn destroy_endpoints(&self, endpoints: &[VirtualEndpoint]) {
        if endpoints.is_empty() {
            return;
        }
        let app = self.app.lock().ok().and_then(|slot| slot.clone());
        let Some(app) = app else {
            log::warn!(
                "Serial runtime 缺少 AppHandle，无法销毁 {} 个虚拟端口对",
                endpoints.len()
            );
            return;
        };
        let state = app.state::<AppState>();
        if let Ok(mut manager) = state.virtual_port_manager.lock() {
            for endpoint in endpoints {
                let _ = manager.destroy_endpoint(endpoint);
            }
            if manager.pending_orphan_count() > 0 {
                log::warn!(
                    "Serial 虚拟端口清理仍有 {} 个端口对等待管理员权限；已交给显式清理动作处理",
                    manager.pending_orphan_count()
                );
            }
        }
    }

    /// 幂等关闭 Serial 私有运行时资源。
    pub fn shutdown_runtime(&self) {
        let bridge = match self.bridge.lock() {
            Ok(mut bridge) => bridge.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(bridge) = bridge {
            bridge.shutdown();
        }
        let endpoints = self.take_endpoints();
        self.destroy_endpoints(&endpoints);
    }
    /// 初始化 Serial 可选虚拟串口能力。
    ///
    /// capability 初始化失败不会回滚已经建立的物理串口 Session；失败通过 Serial 私有事件
    /// 报告，保持“物理串口可用、虚拟桥接不可用”这一真实状态。
    pub fn initialize_virtual_ports(
        &self,
        app: &AppHandle,
        session_id: &str,
        params: &Value,
    ) -> Result<Vec<serde_json::Value>, String> {
        self.remember_context(app, session_id);
        self.shutdown_runtime();
        self.remember_context(app, session_id);

        let enabled = params
            .get("virtual_port_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let count = params
            .get("virtual_port_count")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .unwrap_or(0);
        if !enabled || count == 0 {
            return Ok(Vec::new());
        }

        let state = app.state::<AppState>();
        let config = VirtualPortConfig {
            enabled: true,
            count,
        };
        let mut manager = state
            .virtual_port_manager
            .lock()
            .map_err(|error| error.to_string())?;
        let mut create_error = None;
        let endpoints = manager
            .create_endpoints(&config)
            .or_else(|first_error| {
                log::warn!("直接创建端口对失败: {first_error}；尝试先安装驱动...");
                manager
                    .install_driver()
                    .and_then(|_| manager.create_endpoints(&config))
            })
            .unwrap_or_else(|error| {
                if contains_elevation_indicator(&error) && manager.detect_driver() {
                    log::info!("驱动已安装，尝试通过 UAC 提权创建端口对...");
                    match manager.create_endpoints_elevated(&config) {
                        Ok(endpoints) => return endpoints,
                        Err(elevated_error) => {
                            log::warn!("提权创建端口对也失败: {elevated_error}")
                        }
                    }
                }
                create_error = Some(error);
                Vec::new()
            });
        drop(manager);

        if endpoints.is_empty() {
            let detail = create_error.unwrap_or_else(|| {
                "com0com driver not installed. Run TauTerm as administrator once to install the driver."
                    .to_string()
            });
            let lower = detail.to_lowercase();
            let kind = if lower.contains("driver files missing") {
                "files_missing"
            } else if lower.contains("driver not installed") {
                "driver_missing"
            } else if contains_elevation_indicator(&detail) || lower.contains("cancel") {
                "permission"
            } else {
                "create_failed"
            };
            log::warn!("虚拟端口创建失败 (session={session_id}): {detail}");
            let _ = app.emit(
                "virtual-port-failed",
                serde_json::json!({
                    "session_id": session_id,
                    "kind": kind,
                    "reason": detail,
                }),
            );
            return Ok(Vec::new());
        }

        let baud_rate = params
            .get("baud_rate")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .unwrap_or(115_200);
        let io = state
            .session_store
            .lock()
            .map_err(|error| error.to_string())?
            .get_io_for(session_id)
            .ok_or_else(|| "串口会话缺少共享 I/O capability".to_string())?;
        let bridge_paths = endpoints
            .iter()
            .map(|endpoint| endpoint.bridge_path.clone())
            .collect::<Vec<_>>();
        let error_app = app.clone();
        let error_session_id = session_id.to_string();
        let bridge = match VirtualPortBridge::spawn(
            bridge_paths,
            baud_rate,
            io,
            Box::new(move |reason| {
                let _ = error_app.emit(
                    "virtual-port-failed",
                    serde_json::json!({
                        "session_id": error_session_id,
                        "kind": "bridge_failed",
                        "reason": reason,
                    }),
                );
            }),
        ) {
            Ok(bridge) => bridge,
            Err(reason) => {
                self.destroy_endpoints(&endpoints);
                log::warn!("虚拟端口桥接启动失败 (session={session_id}): {reason}");
                let _ = app.emit(
                    "virtual-port-failed",
                    serde_json::json!({
                        "session_id": session_id,
                        "kind": "bridge_failed",
                        "reason": reason,
                    }),
                );
                return Ok(Vec::new());
            }
        };

        if let Ok(mut slot) = self.bridge.lock() {
            *slot = Some(bridge);
        }
        if let Ok(mut slot) = self.endpoints.lock() {
            *slot = endpoints.clone();
        }
        let payload = endpoints
            .iter()
            .map(|endpoint| serde_json::json!({ "external_path": endpoint.external_path }))
            .collect::<Vec<_>>();
        let _ = app.emit(
            "virtual-port-created",
            serde_json::json!({
                "session_id": session_id,
                "endpoints": payload,
            }),
        );
        Ok(payload)
    }
}

impl SessionService for SerialRuntime {
    fn shutdown(&self) {
        self.shutdown_runtime();
    }
}

struct RuntimeAttach {
    runtime: Arc<SerialRuntime>,
    runtimes: SessionRuntimeRegistry<SerialRuntime>,
}

impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        if let Ok(mut slot) = self.runtime.session_id.lock() {
            *slot = Some(session_id.to_string());
        }
        self.runtimes.attach(session_id, &self.runtime);
    }

    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}

pub struct SerialAdapter {
    runtimes: SessionRuntimeRegistry<SerialRuntime>,
}

impl SerialAdapter {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<SerialRuntime>> {
        self.runtimes.get(session_id)
    }

    /// Serial 插件只解析 transport 真正消费的链路字段。
    ///
    /// params 还会携带终端显示、文件传输和虚拟串口等 Session/UI 配置；Serde 默认忽略
    /// 未知字段，因此这里不复制第二套 Serial DTO。已声明链路字段一旦类型错误则明确失败，
    /// 禁止静默退回 115200/8N1 后继续打开设备。
    fn parse_transport_params(params: &Value) -> Result<SerialTransportConfig, SessionError> {
        serde_json::from_value(params.clone()).map_err(|error| {
            SessionError::Config(format!("invalid serial transport configuration: {error}"))
        })
    }
}

impl Default for SerialAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// 驱动友好名经常以 `(COMx)` 重复携带当前系统端口名。
/// 只移除与当前端口完全匹配的末尾标记，保留其它产品文本原样。
fn normalize_device_label(label: &str, port_name: &str) -> String {
    let trimmed = label.trim();
    let suffix = format!(" ({port_name})");
    let start = trimmed.len().saturating_sub(suffix.len());
    let matches = trimmed
        .get(start..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(&suffix));
    if matches {
        trimmed
            .get(..start)
            .unwrap_or(trimmed)
            .trim_end()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for SerialAdapter {
    fn plugin_id(&self) -> Option<&'static str> {
        Some(PLUGIN_ID)
    }

    async fn connect(
        &self,
        endpoint: &str,
        params: &Value,
    ) -> Result<ProtocolConnection, SessionError> {
        let config = Self::parse_transport_params(params)?;
        let driver = open_serial(endpoint, &config)?;
        let runtime = Arc::new(SerialRuntime::new());
        Ok(ProtocolConnection {
            data_plane: Some(DataPlaneRuntime::spawn(Box::new(driver))),
            service: Some(runtime.clone()),
            file_transfer: None,
            channel_factory: None,
            on_attached: Some(Arc::new(RuntimeAttach {
                runtime,
                runtimes: self.runtimes.clone(),
            })),
            teardown_delay: self.teardown_delay(),
        })
    }

    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError> {
        let ports =
            serialport::available_ports().map_err(|error| SessionError::ConnectionFailed {
                reason: error.to_string(),
            })?;

        Ok(ports
            .into_iter()
            .filter(|port| !is_internal_endpoint_path(&port.port_name))
            .map(|port| {
                let port_name = port.port_name.clone();
                let (description, identity) = match &port.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        let raw_label = info
                            .product
                            .as_deref()
                            .or(info.manufacturer.as_deref())
                            .unwrap_or("USB Serial");
                        let normalized = normalize_device_label(raw_label, &port_name);
                        let label = if normalized.is_empty() {
                            "USB Serial".to_string()
                        } else {
                            normalized
                        };
                        let description = format!("{label} [{:04X}:{:04X}]", info.vid, info.pid);
                        let stable_id = info.serial_number.as_ref().map(|serial| {
                            format!("usb:{:04x}:{:04x}:{serial}", info.vid, info.pid)
                        });
                        (
                            description,
                            serde_json::json!({
                                "kind": "usb",
                                "system_port": port_name.clone(),
                                "vid": info.vid,
                                "pid": info.pid,
                                "serial_number": info.serial_number.clone(),
                                "manufacturer": info.manufacturer.clone(),
                                "product": info.product.clone(),
                                "stable_id": stable_id,
                            }),
                        )
                    }
                    serialport::SerialPortType::BluetoothPort => (
                        "Bluetooth Serial".to_string(),
                        serde_json::json!({"kind": "bluetooth", "system_port": port_name.clone()}),
                    ),
                    serialport::SerialPortType::PciPort => (
                        "PCI Serial".to_string(),
                        serde_json::json!({"kind": "pci", "system_port": port_name.clone()}),
                    ),
                    serialport::SerialPortType::Unknown => (
                        port_name.clone(),
                        serde_json::json!({"kind": "unknown", "system_port": port_name.clone()}),
                    ),
                };
                EndpointInfo {
                    name: port_name,
                    description,
                    params: Some(serde_json::json!({"device_identity": identity})),
                }
            })
            .collect())
    }

    fn teardown_delay(&self) -> std::time::Duration {
        #[cfg(target_os = "windows")]
        {
            std::time::Duration::from_millis(100)
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::time::Duration::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::serial::{SerialFlowControl, SerialParity, SerialStopBits};

    #[test]
    fn serial_adapter_parses_transport_fields_without_owning_session_ui_fields() {
        let config = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": 921600,
            "data_bits": 7,
            "parity": "even",
            "stop_bits": "2",
            "flow_control": "rts_cts",
            "data_mode": "hex",
            "virtual_port_enabled": true,
            "virtual_port_count": 2
        }))
        .unwrap();
        assert_eq!(config.baud_rate, 921600);
        assert_eq!(config.data_bits, 7);
        assert_eq!(config.parity, SerialParity::Even);
        assert_eq!(config.stop_bits, SerialStopBits::Two);
        assert_eq!(config.flow_control, SerialFlowControl::RtsCts);
    }

    #[test]
    fn missing_transport_field_is_rejected_instead_of_being_repaired() {
        let error = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": 115200,
            "data_bits": 8,
            "parity": "none",
            "stop_bits": "1"
        }))
        .unwrap_err();
        assert!(matches!(error, SessionError::Config(_)));
    }

    #[test]
    fn malformed_transport_field_is_rejected_instead_of_falling_back_to_defaults() {
        let error = SerialAdapter::parse_transport_params(&serde_json::json!({
            "baud_rate": "921600",
            "data_bits": 8,
            "parity": "none",
            "stop_bits": "1",
            "flow_control": "none"
        }))
        .unwrap_err();
        assert!(matches!(error, SessionError::Config(_)));
    }

    #[test]
    fn device_label_drops_only_matching_port_suffix() {
        assert_eq!(
            normalize_device_label("STLink Virtual COM Port (COM5)", "COM5"),
            "STLink Virtual COM Port"
        );
        assert_eq!(
            normalize_device_label("Adapter (COM6)", "COM5"),
            "Adapter (COM6)"
        );
        assert_eq!(normalize_device_label("设备适配器", "COM5"), "设备适配器");
    }
}
