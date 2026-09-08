//! Platform/virtual-port Tauri commands.

use crate::AppState;
use tauri::{AppHandle, Emitter, State};

// ── 虚拟串口驱动管理 ────────────────────────────────

/// 查询 com0com 驱动状态（前端主动拉取，解决事件在组件挂载前发射的竞态）
#[tauri::command]
pub async fn check_virtual_port_driver(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "files_present": vpm.are_files_present(),
        "driver_installed": vpm.detect_driver(),
        "orphan_count": vpm.pending_orphan_count(),
    }))
}

/// 尝试安装 com0com 虚拟串口驱动
///
/// 优先直接安装（当前进程已提权时成功）；普通权限下则在 Windows 上
/// 通过 PowerShell Start-Process -Verb RunAs 触发 UAC 提权安装。
#[tauri::command]
pub async fn install_virtual_port_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let mut vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|e| e.to_string())?;

    // 先检测是否已安装
    if vpm.detect_driver() {
        log::info!("com0com 驱动已安装，无需重复操作");
        return Ok("already_installed".into());
    }

    // 检查驱动文件是否存在
    if !vpm.are_files_present() {
        return Err("com0com driver files missing — please reinstall TauTerm".into());
    }

    // 第 1 层: 尝试直接安装（当前进程已提权时成功）
    log::info!("尝试直接安装 com0com 驱动...");
    match vpm.install_driver() {
        Ok(()) => {
            let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
            return Ok("installed".into());
        }
        Err(direct_err) => {
            log::info!("直接安装失败: {}；尝试提权安装...", direct_err);
        }
    }

    // 第 2 层: 通过提权安装（UAC / sudo），逻辑下沉到 VirtualPortManager
    //      避免 commands 层直接依赖 com0com 的 setupc_path/resource_dir
    match vpm.install_driver_elevated() {
        Ok(()) => {
            log::info!("com0com 驱动提权安装成功");
            // 重新检测确认安装成功
            if vpm.detect_driver() {
                let _ = app.emit("virtual-port-driver-ready", serde_json::json!({}));
                return Ok("installed".into());
            }
            Err("Driver installed but detection failed — please restart TauTerm".into())
        }
        Err(elevated_err) => Err(format!(
            "Driver installation failed.\n\n{}\n\n\
                 Action: Run TauTerm as administrator once to install the driver.",
            elevated_err
        )),
    }
}

/// 手动触发虚拟端口残留清理（通过 UAC 提权，单次弹窗）。
///
/// 收集所有已知的残留 bus 号（active_endpoints + com0com_state.json + 驱动真实状态），
/// 通过单个提权的 PowerShell 脚本批量清理。
///
/// 返回 `{ cleaned: N, message: "..." }`。
#[tauri::command]
pub async fn cleanup_virtual_ports(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut vpm = state
        .virtual_port_manager
        .lock()
        .map_err(|e| e.to_string())?;

    // 先尝试直接清理孤儿端口（无需管理员权限的场景）
    let direct_cleaned = vpm.cleanup_orphans();

    // 检查是否还有残留需要 UAC 提权（pending_orphan_count > 0）
    let has_more_work = vpm.pending_orphan_count() > 0;

    if !has_more_work && direct_cleaned > 0 {
        return Ok(serde_json::json!({
            "cleaned": direct_cleaned,
            "message": format!("已清理 {} 个遗留端口对", direct_cleaned),
        }));
    }

    if !has_more_work && direct_cleaned == 0 {
        return Ok(serde_json::json!({
            "cleaned": 0,
            "message": "没有需要清理的端口对",
        }));
    }

    // 有残留且需要 UAC 提权
    log::info!(
        "cleanup_virtual_ports: 直接清理完成 {} 个，剩余端口对需要 UAC 提权",
        direct_cleaned
    );
    match vpm.cleanup_endpoints_elevated() {
        Ok(uac_cleaned) => {
            let total = direct_cleaned + uac_cleaned;
            Ok(serde_json::json!({
                "cleaned": total,
                "message": format!("已清理 {} 个端口对（含 UAC 提权清理 {} 个）", total, uac_cleaned),
            }))
        }
        Err(e) => {
            if e.contains("取消") || e.contains("cancel") {
                Err(format!(
                    "用户取消了 UAC 提权弹窗（已直接清理 {} 个，余下将保留至下次操作）",
                    direct_cleaned
                ))
            } else {
                Err(format!("UAC 提权清理失败: {}", e))
            }
        }
    }
}
