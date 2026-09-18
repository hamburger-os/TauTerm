//! Platform/virtual-port Tauri commands.

use crate::virtual_port::backend::VirtualPortBackend;
use crate::AppState;
use tauri::{AppHandle, Emitter, State};

#[cfg(target_os = "windows")]
fn driver_installed(_backend: &dyn VirtualPortBackend) -> bool {
    crate::virtual_port::windows_driver::is_com0com_driver_installed()
}

#[cfg(not(target_os = "windows"))]
fn driver_installed(backend: &dyn VirtualPortBackend) -> bool {
    backend.detect_driver()
}

// ── 虚拟串口驱动管理 ────────────────────────────────

/// 返回虚拟串口后端能力状态。
///
/// `orphan_count` 的唯一语义是：TauTerm 能证明由自己创建、但当前没有活跃 owner
/// 且仍待回收的资源数量。不得把驱动中任意 com0com bus 计入其中。
/// Windows 的驱动安装状态只查询 SCM；普通状态刷新不得启动 `setupc.exe`。
#[tauri::command]
pub async fn check_virtual_port_driver(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "files_present": vpm.are_files_present(),
        "driver_installed": driver_installed(vpm.as_ref()),
        "orphan_count": vpm.pending_orphan_count(),
    }))
}

/// 安装/初始化虚拟串口后端。
#[tauri::command]
pub async fn install_virtual_port_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let mut vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|error| error.to_string())?;

    if driver_installed(vpm.as_ref()) {
        log::info!("虚拟串口驱动已就绪，无需重复安装");
        return Ok("already_installed".into());
    }
    if !vpm.are_files_present() {
        return Err("com0com driver files missing — please reinstall TauTerm".into());
    }

    log::info!("尝试初始化虚拟串口驱动...");
    vpm.install_driver()?;
    if !driver_installed(vpm.as_ref()) {
        return Err("Driver initialization completed but the driver is still unavailable".into());
    }
    let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
    Ok("installed".into())
}

/// 清理确认属于 TauTerm 且当前无活跃 owner 的残留虚拟端口。
///
/// 安全边界：
/// - 不触碰当前 active endpoint；
/// - 不扫描删除第三方/用户自己创建的 com0com bus；
/// - 这是用户显式动作；Windows direct-UAC 后端只在这里启动一次窄类型 helper；
/// - 普通启动与 Session 断开路径不会执行 setupc，也不会弹 UAC。
#[tauri::command]
pub async fn cleanup_virtual_ports(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|error| error.to_string())?;

    let cleaned = vpm.cleanup_orphans()?;
    Ok(serde_json::json!({
        "cleaned": cleaned,
        "message": if cleaned == 0 {
            "没有需要清理的残留端口对".to_string()
        } else {
            format!("已清理 {cleaned} 个残留端口对")
        },
    }))
}
