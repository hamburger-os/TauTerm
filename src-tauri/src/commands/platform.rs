//! Platform/virtual-port Tauri commands.

use crate::AppState;
use tauri::{AppHandle, Emitter, State};

// ── 虚拟串口驱动管理 ────────────────────────────────

/// 返回虚拟串口后端能力状态。
///
/// `orphan_count` 的唯一语义是：TauTerm 能证明由自己创建、但当前没有活跃 owner
/// 且仍待回收的资源数量。不得把驱动中任意 com0com bus 计入其中。
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
        "driver_installed": vpm.detect_driver(),
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

    if vpm.detect_driver() {
        log::info!("虚拟串口驱动已就绪，无需重复安装");
        return Ok("already_installed".into());
    }
    if !vpm.are_files_present() {
        return Err("com0com driver files missing — please reinstall TauTerm".into());
    }

    log::info!("尝试直接初始化虚拟串口驱动...");
    if vpm.install_driver().is_ok() {
        let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
        return Ok("installed".into());
    }

    match vpm.install_driver_elevated() {
        Ok(()) if vpm.detect_driver() => {
            let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
            Ok("installed".into())
        }
        Ok(()) => Err("Driver installed but detection failed — please restart TauTerm".into()),
        Err(error) => Err(format!(
            "Driver installation failed.\n\n{error}\n\nAction: Run TauTerm as administrator once to install the driver."
        )),
    }
}

/// 清理确认属于 TauTerm 且当前无活跃 owner 的残留虚拟端口。
///
/// 安全边界：
/// - 不触碰当前 active endpoint；
/// - 不扫描删除第三方/用户自己创建的 com0com bus；
/// - 先走普通权限清理，仅对仍需权限的已知 orphan 执行一次提权批处理。
#[tauri::command]
pub async fn cleanup_virtual_ports(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|error| error.to_string())?;

    let direct_cleaned = vpm.cleanup_orphans();
    if vpm.pending_orphan_count() == 0 {
        return Ok(serde_json::json!({
            "cleaned": direct_cleaned,
            "message": if direct_cleaned == 0 {
                "没有需要清理的残留端口对".to_string()
            } else {
                format!("已清理 {direct_cleaned} 个残留端口对")
            },
        }));
    }

    log::info!(
        "cleanup_virtual_ports: directly cleaned {}, remaining owned orphans require elevation",
        direct_cleaned
    );
    match vpm.cleanup_endpoints_elevated() {
        Ok(elevated_cleaned) => {
            let total = direct_cleaned + elevated_cleaned;
            Ok(serde_json::json!({
                "cleaned": total,
                "message": format!(
                    "已清理 {total} 个残留端口对（其中 {elevated_cleaned} 个通过提权清理）"
                ),
            }))
        }
        Err(error) if error.to_lowercase().contains("cancel") || error.contains("取消") => {
            Err(format!(
                "用户取消了提权操作（已直接清理 {direct_cleaned} 个，剩余资源保持待清理状态）"
            ))
        }
        Err(error) => Err(format!("提权清理失败: {error}")),
    }
}
