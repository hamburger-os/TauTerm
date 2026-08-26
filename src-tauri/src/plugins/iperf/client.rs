//! iperf 客户端测速任务
//!
//! 瞬态任务模式：配置 → 运行 → 实时出结果 → 结束。
//! 在 `tokio::spawn_blocking` 中执行（iperf2 使用同步 std::net，对齐 TFTP client 模式）。
//!
//! 事件流：
//! 1. `iperf-test-started`  — 测试开始（前端清空旧数据、置 test_running）
//! 2. `iperf-interval-report` × N — 每个 -i 区间（实时流 + 图表数据点）
//! 3. `iperf-test-done`     — 测试完成/失败（汇总 + 置 test_running=false）
//!
//! **done 兜底**：引擎通过 `catch_unwind` 包裹，panic 也走统一收尾路径，
//! 保证 `iperf-test-done` 一定发出（前端状态机依赖此事件恢复）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Emitter;

use super::{IperfClientResult, IperfDynamicParams, IperfSummary, IperfVersion};

/// 运行一次客户端测速（瞬态任务）。
///
/// 返回后测试已结束。全程通过事件向前端推送进度。
/// 引擎按 `params.version` 路由（iperf2 / iperf3）。
pub async fn run_iperf_client<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: String,
    target_host: String,
    params: IperfDynamicParams,
    abort_flag: Arc<AtomicBool>,
    client_test_running: Arc<AtomicBool>,
    last_summary: Arc<Mutex<Option<IperfSummary>>>,
) -> Result<(), String> {
    let version = params.version;
    // 运行标志由调用方（iperf_client_run）在重跑守卫通过后同步置位（闭合
    // TOCTOU 窗口）；中止标志亦由调用方在确认上一轮收尾后复位（若在此复位，
    // 10s 等待超时后仍存活的上一轮任务会被静默"解除中止"）

    let _ = app.emit(
        "iperf-test-started",
        serde_json::json!({
            "session_id": session_id,
            "role": "client",
            "direction": "fwd",
            "protocol": params.protocol,
            "target": target_host,
            "params": params,
        }),
    );
    log::info!(
        "[iperf] 客户端测速开始 (session={}, target={}, version={:?})",
        session_id,
        target_host,
        version
    );

    let app2 = app.clone();
    let sid = session_id.clone();
    let target = target_host.clone();
    // catch_unwind 兜底：引擎 panic 也返回 Err，统一走 done 收尾
    let joined = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match version {
            IperfVersion::Iperf2 => {
                super::iperf2::run_client(&app2, &sid, &target, &params, &abort_flag)
            }
            IperfVersion::Iperf3 => {
                super::iperf3::run_client(&app2, &sid, &target, &params, &abort_flag).map(
                    |summary| IperfClientResult {
                        fwd: summary,
                        rev: None,
                        warning: None,
                    },
                )
            }
        }))
    })
    .await;

    // 注意：运行标志在 done 事件发出后才复位（见下方两分支）——上一轮的
    // done 与下一轮的 started 必须保持先后顺序（客户端事件无 seq，重跑守卫
    // 依赖该顺序防止两轮事件在前端错配）
    let result: Result<IperfClientResult, String> = match joined {
        Ok(Ok(Ok(result))) => Ok(result),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(_)) => Err("客户端测速内部错误（已中止）".into()),
        Err(e) => Err(format!("测速任务异常: {}", e)),
    };

    match result {
        Ok(result) => {
            // 锁中毒走统一恢复出口（此前 if let Ok 静默跳过：引擎 panic 后
            // 摘要永久丢失、get_status 永远返回 null）
            let mut last = super::lock_or_recover(&last_summary, "last_summary");
            *last = Some(result.fwd.clone());
            // fwd done（警告字段用于 UDP 未收到服务器回报等非致命提示）
            let mut done = serde_json::json!({
                "session_id": session_id,
                "success": true,
                "role": "client",
                "direction": "fwd",
                "protocol": result.fwd.protocol,
                "summary": result.fwd,
            });
            if let Some(w) = &result.warning {
                done["warning"] = serde_json::json!(w);
            }
            let _ = app.emit("iperf-test-done", done);
            // rev done（-d/-r 反向相，紧随 fwd done 发出）
            if let Some(rev) = result.rev {
                let _ = app.emit(
                    "iperf-test-done",
                    serde_json::json!({
                        "session_id": session_id,
                        "success": true,
                        "role": "client",
                        "direction": "rev",
                        "protocol": rev.protocol,
                        "summary": rev,
                    }),
                );
            }
            // done 全部发出后才复位：重跑守卫的等待循环只在本轮事件
            // 完整送达后才放行下一轮
            client_test_running.store(false, Ordering::Relaxed);
            log::info!("[iperf] 客户端测速完成 (session={})", session_id);
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                "iperf-test-done",
                serde_json::json!({
                    "session_id": session_id,
                    "success": false,
                    "error": e,
                    "role": "client",
                    "direction": "fwd",
                    "summary": null,
                }),
            );
            client_test_running.store(false, Ordering::Relaxed);
            log::warn!("[iperf] 客户端测速失败 (session={}): {}", session_id, e);
            Err(e)
        }
    }
}
