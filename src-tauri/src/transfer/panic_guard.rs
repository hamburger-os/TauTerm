//! PanicGuard — RAII 守卫确保传输任务 panic/abort 时也能清理会话状态
//!
//! 用于 SideChannel 传输的 tokio::spawn 块内。即使 task panic 或被 abort，
//! Drop 实现也会调用 `transfer_done()` 释放取消标志，防止会话
//! 永久卡在 "传输中" 状态。
//!
//! ## 使用方式
//! ```ignore
//! let mut guard = PanicGuard::new(app.clone(), session_id.clone());
//! // ... 执行传输 ...
//! // 传输成功后调用 defuse() 并显式 emit 成功事件
//! // Drop 时：若未 defused 则自动 emit 失败事件
//! ```

use tauri::AppHandle;
use tauri::{Emitter, Manager};

use crate::AppState;

/// RAII 守卫：Drop 时自动调用 `SessionStore::transfer_done()`
///
/// # 使用方式
/// ```ignore
/// let mut guard = PanicGuard::new(app.clone(), session_id.clone());
/// // ... 执行传输 ...
/// // 成功路径：显式 emit 成功事件后 defuse
/// guard.defuse();
/// // Drop 时：若未 defuse（panic / abort / 错误路径）自动 emit 失败事件
/// ```
pub(crate) struct PanicGuard {
    app: AppHandle,
    sid: String,
    /// 标记传输已正常完成。若 Drop 时仍为 false，说明异常退出。
    defused: bool,
}

impl PanicGuard {
    pub(crate) fn new(app: AppHandle, sid: String) -> Self {
        Self {
            app,
            sid,
            defused: false,
        }
    }

    /// 标记守卫为"已解除"。成功路径上调用此方法后，
    /// Drop 时不会自动 emit 失败事件。
    pub(crate) fn defuse(&mut self) {
        self.defused = true;
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        // 总是调用 transfer_done 清理会话传输状态
        if let Some(app_state) = self.app.try_state::<AppState>() {
            if let Ok(mut store) = app_state.session_store.lock() {
                store.transfer_done(&self.sid);
            }
        }
        // 若未被显式 defuse（panic / tokio::spawn abort / 传输失败），
        // 发出失败事件避免前端进度条永久卡住。
        // 成功路径上 orchestrator 已显式 emit 成功事件后调用 defuse()，
        // 此处跳过以防止重复 emit。
        if !self.defused {
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
