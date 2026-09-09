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
//! // 正常结束后先调用 complete() 释放 SessionStore 传输占用，再显式 emit finished
//! // Drop 时：若未 complete 则自动 cleanup 并 emit 失败事件
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
    client_id: String,
    protocol: String,
    transfer_id: String,
    /// 标记传输已正常完成。若 Drop 时仍为 false，说明异常退出。
    defused: bool,
    cleaned: bool,
}

impl PanicGuard {
    pub(crate) fn new(
        app: AppHandle,
        sid: String,
        client_id: String,
        protocol: String,
        transfer_id: String,
    ) -> Self {
        Self {
            app,
            sid,
            client_id,
            protocol,
            transfer_id,
            defused: false,
            cleaned: false,
        }
    }

    fn cleanup(&mut self) {
        if self.cleaned {
            return;
        }
        if let Some(app_state) = self.app.try_state::<AppState>() {
            if let Ok(mut store) = app_state.session_store.lock() {
                store.transfer_done(&self.sid, Some(&self.transfer_id));
            }
        }
        self.cleaned = true;
    }

    /// 正常完成路径：先释放 SessionStore 的传输占用，再标记守卫为已解除。
    /// 调用方随后才 emit finished，保证用户收到完成事件时下一次传输已经可启动。
    pub(crate) fn complete(&mut self) {
        self.cleanup();
        self.defused = true;
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        self.cleanup();
        // 若未被显式 complete（panic / tokio::spawn abort），发出带完整身份的失败事件，
        // 使前端只结束当前 transfer_id，不会误伤下一次传输。
        if !self.defused {
            let _ = self.app.emit(
                "file-transfer:finished",
                serde_json::json!({
                    "session_id": &self.client_id,
                    "transfer_id": &self.transfer_id,
                    "protocol": &self.protocol,
                    "success": false,
                    "cancelled": false,
                    "error": "传输任务异常终止",
                }),
            );
        }
    }
}
