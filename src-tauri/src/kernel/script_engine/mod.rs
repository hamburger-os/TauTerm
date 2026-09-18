//! 脚本引擎模块
//!
//! 基于 mlua (Lua 5.4) 的嵌入式脚本运行时，每个会话独立的 Lua VM。
//! 发送与接收只依赖协议无关的 AutomationIo/AutomationRx 能力；普通流式 Session 由
//! SessionIo/DataPlane 适配，RTT 等多流容器可提供自己的有界自动化流而无需伪造主 DataPlane。

pub mod codegen;
pub mod lua_api;
pub mod sandbox;

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use tauri::Emitter;

use crate::session::{AutomationIo, AutomationRx, AutomationRxEvent};

use self::lua_api::inject_lua_api;
use self::sandbox::create_sandboxed_lua;

pub enum ScriptCmd {
    LoadScript(String),
    FeedData(Vec<u8>),
    Shutdown,
}

const FEED_CODE: &str = r#"
	local data = __current_data or ""
	local handlers = __handlers or {}
	for _, handler in ipairs(handlers) do
	    local ok, match_or_err = pcall(string.find, data, handler.pattern)
	    if ok and match_or_err then
	        local status, err = pcall(handler.callback, data)
	        if not status then
	            log("Handler error [" .. tostring(handler.pattern) .. "]: " .. tostring(err))
	        end
	    elseif not ok then
	        log("Pre-filter error [" .. tostring(handler.pattern) .. "]: " .. tostring(match_or_err))
	    end
	end
"#;

const TICK_CODE: &str = r#"
	local now = _time_ms()
	local timers = __timers or {}
	for _, timer in ipairs(timers) do
	    if now - timer.last_fire >= timer.interval_ms then
	        timer.last_fire = now
	        local ok, err = pcall(timer.callback)
	        if not ok then
	            log("Timer error [" .. tostring(timer.id) .. "]: " .. tostring(err))
	        end
	    end
	end
"#;

struct ScriptEngine {
    lua: mlua::Lua,
    app_handle: tauri::AppHandle,
    session_id: String,
    feed_fn: mlua::Function,
    tick_fn: mlua::Function,
}

impl ScriptEngine {
    fn new(
        io: Arc<dyn AutomationIo>,
        app_handle: tauri::AppHandle,
        session_id: &str,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self, ScriptEngineError> {
        let lua = create_sandboxed_lua()?;
        inject_lua_api(&lua, io, app_handle.clone(), session_id, shutdown)?;
        let feed_fn = lua.load(FEED_CODE).into_function().map_err(|error| {
            ScriptEngineError::LuaError(format!("预编译 feed_fn 失败: {error}"))
        })?;
        let tick_fn = lua.load(TICK_CODE).into_function().map_err(|error| {
            ScriptEngineError::LuaError(format!("预编译 tick_fn 失败: {error}"))
        })?;
        Ok(Self {
            lua,
            app_handle,
            session_id: session_id.to_string(),
            feed_fn,
            tick_fn,
        })
    }

    fn load_script(&self, code: &str) -> Result<(), ScriptEngineError> {
        self.lua.load("__handlers = {}\n__timers = {}").exec()?;
        self.lua.load(code).exec()?;
        let count: i64 = self.lua.load("return #__handlers").eval().unwrap_or(0);
        log::info!("脚本已加载，注册了 {} 个数据处理器", count);
        self.emit_log(&format!("脚本已加载（{} 个处理器）", count));
        Ok(())
    }

    fn feed_data(&self, data: &[u8]) {
        let lua_str = match self.lua.create_string(data) {
            Ok(value) => value,
            Err(error) => {
                log::error!("feed_data 创建 Lua 字符串失败: {}", error);
                return;
            }
        };
        if let Err(error) = self.lua.globals().set("__current_data", lua_str) {
            log::error!("feed_data 设置 __current_data 失败: {}", error);
            return;
        }
        if let Err(error) = self.feed_fn.call::<()>(()) {
            log::error!("feed_data Lua 执行错误: {}", error);
        }
    }

    fn tick_timers(&self) {
        if let Err(error) = self.tick_fn.call::<()>(()) {
            log::error!("tick_timers Lua 执行错误: {}", error);
        }
    }

    fn stop(&mut self) {
        let _ = self.lua.load("__handlers = nil\n__timers = nil").exec();
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
    loop {
        match subscription.try_recv() {
            Ok(AutomationRxEvent::Data(data)) => engine.feed_data(&data),
            Ok(AutomationRxEvent::Closed) => return false,
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}

/// Start one per-session Lua VM. Ordinary sessions adapt their canonical DataPlane subscription;
/// multiplexed/container plugins can provide a protocol-owned bounded AutomationRx. FeedData remains
/// available for background child aggregation that is intentionally external to a root target.
pub fn spawn_script_thread(
    io: Arc<dyn AutomationIo>,
    mut subscription: Option<Box<dyn AutomationRx>>,
    app_handle: tauri::AppHandle,
    rx: mpsc::Receiver<ScriptCmd>,
    session_id: String,
    shutdown: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut engine = match ScriptEngine::new(io, app_handle.clone(), &session_id, shutdown) {
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
            if !drain_automation(&engine, &mut subscription) {
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
                Ok(ScriptCmd::FeedData(data)) => engine.feed_data(&data),
                Ok(ScriptCmd::Shutdown) => {
                    engine.stop();
                    log::info!("ScriptEngine 线程退出");
                    break;
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
