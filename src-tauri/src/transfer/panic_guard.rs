//! RAII 守卫确保传输任务 panic/abort 时也能释放原连接实例的调度槽。
//!
//! 用于 Auxiliary 传输的 tokio::spawn 块内。即使 task panic 或被 abort，
//! Drop 调用 `transfer_done()` 释放取消标志；它不改写连接健康状态。

use tauri::AppHandle;
use tauri::{Emitter, Manager};

use crate::session::{RuntimeId, RuntimeIdentity};
use crate::AppState;

/// 正常路径先 complete 再发布 finished；未完成的 Drop 清理并发布精确任务失败。
pub(crate) struct PanicGuard {
    app: AppHandle,
    sid: String,
    client_id: String,
    protocol: String,
    transfer_id: String,
    runtime_id: RuntimeId,
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
        runtime_id: RuntimeId,
    ) -> Self {
        Self {
            app,
            sid,
            client_id,
            protocol,
            transfer_id,
            runtime_id,
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
                store.transfer_done(
                    &RuntimeIdentity {
                        session_id: self.sid.clone(),
                        runtime_id: self.runtime_id,
                    },
                    Some(&self.transfer_id),
                );
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
                    "runtime_id": self.runtime_id,
                    "results": serde_json::Value::Null,
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
