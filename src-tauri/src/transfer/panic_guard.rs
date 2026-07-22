//! PanicGuard — RAII 守卫确保传输任务 panic 时也能清理会话状态
//!
//! 用于 SideChannel 传输的 tokio::spawn 块内。即使 task panic，
//! Drop 实现也会调用 `transfer_done()` 释放取消标志，防止会话
//! 永久卡在 "传输中" 状态。

use tauri::AppHandle;
use tauri::{Emitter, Manager};

use crate::AppState;

/// RAII 守卫：Drop 时自动调用 `SessionStore::transfer_done()`
///
/// # 使用方式
/// ```ignore
/// let _guard = PanicGuard::new(app.clone(), session_id.clone());
/// // ... 执行传输 ...
/// // Drop 时自动清理（含 panic 场景）
/// ```
pub(crate) struct PanicGuard {
    app: AppHandle,
    sid: String,
}

impl PanicGuard {
    pub(crate) fn new(app: AppHandle, sid: String) -> Self {
        Self { app, sid }
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        if let Some(app_state) = self.app.try_state::<AppState>() {
            if let Ok(mut store) = app_state.session_store.lock() {
                store.transfer_done(&self.sid);
            }
        }
        // 在 panic 展开路径上发出完成事件，否则前端进度条永久卡住。
        // 正常路径上 std::thread::panicking() 返回 false，不会重复 emit
        // （orchestrator 的显式清理已在 Drop 前发出事件）。
        if std::thread::panicking() {
            let _ = self.app.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &self.sid,
                    "success": false,
                    "error": "传输任务异常终止",
                }),
            );
        }
    }
}
