//! One owner for the per-session script command queue, cancellation flag and worker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::session::AutomationIo;

use super::{spawn_script_thread, ScriptCmd};

pub struct ScriptRuntime {
    commands: mpsc::SyncSender<ScriptCmd>,
    shutdown: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

pub struct ScriptCleanup {
    worker: std::thread::JoinHandle<()>,
}

impl ScriptCleanup {
    pub fn join(self) {
        if let Err(error) = self.worker.join() {
            log::error!("脚本线程异常退出: {error:?}");
        }
    }
}

impl ScriptRuntime {
    pub fn start(
        io: Arc<dyn AutomationIo>,
        app: tauri::AppHandle,
        session_id: &str,
        code: &str,
    ) -> Result<Self, String> {
        let subscription = io
            .subscribe(&format!("script:{session_id}"))
            .map_err(|error| error.to_string())?;
        let (commands, receiver) = mpsc::sync_channel(4096);
        commands
            .try_send(ScriptCmd::LoadScript(code.to_string()))
            .map_err(|error| format!("发送脚本失败: {error}"))?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker = spawn_script_thread(
            io,
            Some(subscription),
            app,
            receiver,
            session_id.to_string(),
            shutdown.clone(),
        );
        Ok(Self {
            commands,
            shutdown,
            worker: Some(worker),
        })
    }

    pub fn load(&self, code: &str) -> Result<(), String> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err("脚本引擎正在停止".into());
        }
        self.commands
            .try_send(ScriptCmd::LoadScript(code.to_string()))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "脚本命令队列已满，请稍后重试".into(),
                mpsc::TrySendError::Disconnected(_) => {
                    "脚本引擎已停止，请先停止旧任务再重新启动".into()
                }
            })
    }

    pub fn cancel(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    pub fn into_cleanup(mut self) -> ScriptCleanup {
        self.cancel();
        ScriptCleanup {
            worker: self.worker.take().expect("ScriptRuntime owns its worker"),
        }
    }
}

impl Drop for ScriptRuntime {
    fn drop(&mut self) {
        // Fallback cancellation never sends into a potentially full queue or joins under a lock.
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cancellation_does_not_need_space_in_the_command_queue() {
        let (commands, _receiver) = mpsc::sync_channel(1);
        commands
            .try_send(ScriptCmd::LoadScript("pending".into()))
            .expect("fill queue");
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            while !worker_shutdown.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            done_tx.send(()).expect("done");
        });
        let runtime = ScriptRuntime {
            commands,
            shutdown,
            worker: Some(worker),
        };
        assert!(runtime.load("another").is_err());
        let cleanup = runtime.into_cleanup();
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("cancel bypasses full queue");
        cleanup.join();
    }
}
