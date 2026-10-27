//! 脚本引擎模块
//!
//! 基于 mlua (Lua 5.4) 的嵌入式脚本运行时，每个会话独立的 Lua VM。
//! 发送与接收只依赖协议无关的 AutomationIo/AutomationRx 能力；普通流式 Session 由
//! SessionIo/DataPlane 适配，RTT 等多流容器可提供自己的有界自动化流而无需伪造主 DataPlane。

pub mod codegen;
mod execution;
pub mod lua_api;
mod runtime;
pub mod sandbox;

pub use runtime::{ScriptCleanup, ScriptRuntime};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use tauri::Emitter;

use crate::session::{AutomationIo, AutomationRx, AutomationRxEvent};

use self::execution::ScriptExecution;
use self::lua_api::inject_lua_api;
use self::sandbox::create_sandboxed_lua;

pub enum ScriptCmd {
    LoadScript(String),
}

struct ScriptEngine {
    lua: mlua::Lua,
    app_handle: tauri::AppHandle,
    session_id: String,
    execution: ScriptExecution,
    match_fn: mlua::Function,
}

impl ScriptEngine {
    fn new(
        io: Arc<dyn AutomationIo>,
        app_handle: tauri::AppHandle,
        session_id: &str,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self, ScriptEngineError> {
        let lua = create_sandboxed_lua()?;
        let execution = ScriptExecution::new(&lua, shutdown.clone())?;
        inject_lua_api(&lua, io, app_handle.clone(), session_id, shutdown)?;
        let string: mlua::Table = lua.globals().get("string")?;
        let match_fn = string.get("find")?;
        Ok(Self {
            lua,
            app_handle,
            session_id: session_id.to_string(),
            execution,
            match_fn,
        })
    }

    fn load_script(&self, code: &str) -> Result<(), ScriptEngineError> {
        self.clear_callbacks()?;
        let result = self
            .execution
            .call(&self.lua, self.lua.load(code).into_function()?, ());
        if let Err(error) = result {
            self.clear_callbacks()?;
            return Err(error.into());
        }
        let handlers: mlua::Table = self.lua.globals().raw_get("__handlers")?;
        let count = handlers.raw_len();
        log::info!("脚本已加载，注册了 {} 个数据处理器", count);
        self.emit_log(&format!("脚本已加载（{} 个处理器）", count));
        Ok(())
    }

    fn feed_data(&self, data: &[u8]) {
        let result = (|| -> mlua::Result<()> {
            let data = self.lua.create_string(data)?;
            for handler in self.callbacks("__handlers")? {
                if self.execution.is_cancelled() {
                    break;
                }
                let pattern: mlua::String = handler.raw_get("pattern")?;
                match self
                    .execution
                    .call(&self.lua, self.match_fn.clone(), (data.clone(), pattern))
                {
                    Ok(values) if !matches!(values.front(), None | Some(mlua::Value::Nil)) => {
                        let callback = handler.raw_get("callback")?;
                        if let Err(error) = self.execution.call(&self.lua, callback, data.clone()) {
                            self.report_error("Handler", &error);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => self.report_error("Pre-filter", &error),
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.report_error("Data", &error);
        }
    }

    fn tick_timers(&self) {
        let result = (|| -> mlua::Result<()> {
            let now = chrono::Utc::now().timestamp_millis() as f64;
            for timer in self.callbacks("__timers")? {
                if self.execution.is_cancelled() {
                    break;
                }
                let last_fire: f64 = timer.raw_get("last_fire")?;
                let interval: f64 = timer.raw_get("interval_ms")?;
                if now - last_fire >= interval {
                    timer.raw_set("last_fire", now)?;
                    let callback = timer.raw_get("callback")?;
                    if let Err(error) = self.execution.call(&self.lua, callback, ()) {
                        self.report_error("Timer", &error);
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.report_error("Timer dispatch", &error);
        }
    }

    fn callbacks(&self, name: &str) -> mlua::Result<Vec<mlua::Table>> {
        let entries: mlua::Table = self.lua.globals().raw_get(name)?;
        (1..=entries.raw_len())
            .map(|index| entries.raw_get(index))
            .collect()
    }

    fn clear_callbacks(&self) -> mlua::Result<()> {
        self.lua
            .globals()
            .raw_set("__handlers", self.lua.create_table()?)?;
        self.lua
            .globals()
            .raw_set("__timers", self.lua.create_table()?)
    }

    fn report_error(&self, context: &str, error: &mlua::Error) {
        if !self.execution.is_cancelled() {
            log::error!("Script {context}: {error}");
            self.emit_log(&format!("{context}: {error}"));
        }
    }

    fn stop(&mut self) {
        let _ = self.clear_callbacks();
        log::info!("ScriptEngine 已停止");
    }

    fn emit_log(&self, message: &str) {
        let _ = self.app_handle.emit(
            "script-log",
            serde_json::json!({
                "session_id": self.session_id,
                "message": message,
            }),
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptEngineError {
    #[error("Lua 错误: {0}")]
    LuaError(String),
}

impl From<mlua::Error> for ScriptEngineError {
    fn from(error: mlua::Error) -> Self {
        ScriptEngineError::LuaError(error.to_string())
    }
}

fn drain_automation(
    engine: &ScriptEngine,
    subscription: &mut Option<Box<dyn AutomationRx>>,
) -> bool {
    let Some(subscription) = subscription.as_mut() else {
        return true;
    };
    // Bound each drain so continuous RX cannot starve commands or timer callbacks.
    for _ in 0..64 {
        if engine.execution.is_cancelled() {
            return false;
        }
        match subscription.try_recv() {
            Ok(AutomationRxEvent::Data(data)) => engine.feed_data(&data),
            Ok(AutomationRxEvent::Closed) => return false,
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
    true
}

/// Start one per-session Lua VM. Ordinary sessions adapt their canonical DataPlane subscription;
/// multiplexed/container plugins can provide a protocol-owned bounded AutomationRx.
/// The shared cancellation flag is independent of command admission and queue capacity.
pub fn spawn_script_thread(
    io: Arc<dyn AutomationIo>,
    mut subscription: Option<Box<dyn AutomationRx>>,
    app_handle: tauri::AppHandle,
    rx: mpsc::Receiver<ScriptCmd>,
    session_id: String,
    shutdown: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut engine =
            match ScriptEngine::new(io, app_handle.clone(), &session_id, shutdown.clone()) {
                Ok(engine) => engine,
                Err(error) => {
                    log::error!("创建 ScriptEngine 失败: {}", error);
                    let _ = app_handle.emit(
                        "script-log",
                        serde_json::json!({
                            "session_id": session_id,
                            "message": format!("脚本引擎初始化失败: {error}"),
                        }),
                    );
                    return;
                }
            };

        log::info!("ScriptEngine 线程已启动");
        loop {
            if shutdown.load(Ordering::Acquire) || !drain_automation(&engine, &mut subscription) {
                engine.stop();
                log::info!("ScriptEngine 自动化数据源已关闭");
                break;
            }
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(ScriptCmd::LoadScript(code)) => {
                    if let Err(error) = engine.load_script(&code) {
                        log::error!("脚本加载失败: {}", error);
                        engine.emit_log(&format!("脚本加载失败: {error}"));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => engine.tick_timers(),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    engine.stop();
                    log::info!("ScriptEngine 线程退出 (channel 断开)");
                    break;
                }
            }
        }
    })
}
