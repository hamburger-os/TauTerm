//! 脚本引擎模块
//!
//! 基于 mlua (Lua 5.4) 的嵌入式脚本运行时，每个会话独立的 Lua VM。
//! 通过 channel 与 I/O 线程通信，支持热加载、安全沙箱和代码生成。
//!
//! ## 架构
//!
//! ScriptEngine (per-session)
//! ├── mlua::Lua VM (独立 std::thread)
//! ├── CommHandle (通信抽象，不感知底层协议)
//! ├── Lua 内部 __handlers 表 (由 on_data() 填充，feed_data() 遍历)
//! └── ScriptCmd channel: LoadScript / FeedData / Shutdown
//!
//! ## 日志机制
//!
//! Lua 的 `log()` 函数通过 Tauri `script-log` 事件发送消息到前端。
//! 错误消息同样通过事件推送，前端 ScriptEditor 的 "Script Output" 面板
//! 通过 `listen("script-log")` 接收并展示。

pub mod codegen;
pub mod lua_api;
pub mod sandbox;

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use tauri::Emitter;

use crate::kernel::comm_handle::CommHandle;

use self::lua_api::inject_lua_api;
use self::sandbox::create_sandboxed_lua;

/// 脚本引擎命令
pub enum ScriptCmd {
    /// 加载并执行新的 Lua 脚本代码
    LoadScript(String),
    /// 将接收到的数据喂给脚本引擎（触发 on_data 回调）
    FeedData(Vec<u8>),
    /// 优雅关闭脚本引擎线程
    Shutdown,
}

// ── 预编译的 Lua 热点代码片段 ──────────────────────────
// 在 ScriptEngine::new() 中编译为 mlua::Function，避免每次数据包/tick
// 重复解析相同源码。

/// feed_data 的数据匹配与分发循环（pcall 隔离单个 handler 错误）
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

/// tick_timers 的定时器到期检查与回调执行循环
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

/// 每会话脚本引擎
struct ScriptEngine {
    /// Lua 5.4 虚拟机
    lua: mlua::Lua,
    /// Tauri AppHandle（用于 emit 日志事件）
    app_handle: tauri::AppHandle,
    /// 会话 ID（用于日志事件的 session_id 过滤）
    session_id: String,
    /// 预编译的 feed_data 分发函数（避免每次数据包重新解析 FEED_CODE）
    feed_fn: mlua::Function,
    /// 预编译的 tick_timers 分发函数（避免每次超时重新解析 TICK_CODE）
    tick_fn: mlua::Function,
}

impl ScriptEngine {
    /// 创建新的脚本引擎实例
    fn new(
        comm: Arc<dyn CommHandle>,
        app_handle: tauri::AppHandle,
        session_id: &str,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self, ScriptEngineError> {
        let lua = create_sandboxed_lua()?;

        // 向 Lua 全局环境注入 API（含协作式 sleep 所需的 shutdown 标志）
        inject_lua_api(&lua, comm, app_handle.clone(), session_id, shutdown)?;

        // 预编译热点 Lua 代码片段为 Function，避免每次数据包/tick 重新解析
        let feed_fn = lua
            .load(FEED_CODE)
            .into_function()
            .map_err(|e| ScriptEngineError::LuaError(format!("预编译 feed_fn 失败: {}", e)))?;
        let tick_fn = lua
            .load(TICK_CODE)
            .into_function()
            .map_err(|e| ScriptEngineError::LuaError(format!("预编译 tick_fn 失败: {}", e)))?;

        Ok(Self {
            lua,
            app_handle,
            session_id: session_id.to_string(),
            feed_fn,
            tick_fn,
        })
    }

    /// 加载并执行用户脚本
    fn load_script(&self, code: &str) -> Result<(), ScriptEngineError> {
        // 清空旧 handlers 表与定时器表
        self.lua.load("__handlers = {}\n__timers = {}").exec()?;

        // 执行脚本：脚本中的 on_data(pattern, fn) 会填充 __handlers 表
        self.lua.load(code).exec()?;

        // 获取 handler 数量
        let count: i64 = self
            .lua
            .load("return #__handlers")
            .eval()
            .unwrap_or(0);

        log::info!("脚本已加载，注册了 {} 个数据处理器", count);
        self.emit_log(&format!("脚本已加载（{} 个处理器）", count));

        Ok(())
    }

    /// 将收到的数据推送给脚本引擎
    ///
    /// 通过 `create_string()` 将原始字节设为全局 `__current_data`（二进制安全，
    /// 可含 \0 等任意字节），再执行预编译的 feed_fn 遍历 __handlers 匹配并调用 callback。
    fn feed_data(&self, data: &[u8]) {
        // 1. 二进制安全地传入数据
        let lua_str = match self.lua.create_string(data) {
            Ok(s) => s,
            Err(e) => {
                log::error!("feed_data 创建 Lua 字符串失败: {}", e);
                return;
            }
        };
        if let Err(e) = self.lua.globals().set("__current_data", lua_str) {
            log::error!("feed_data 设置 __current_data 失败: {}", e);
            return;
        }

        // 2. 执行预编译的匹配循环（pcall 隔离单个 handler 错误）
        if let Err(e) = self.feed_fn.call::<()>(()) {
            log::error!("feed_data Lua 执行错误: {}", e);
        }
    }

    /// 检查并触发到期的定时器（由事件循环在空闲时调用）
    ///
    /// 遍历 __timers 表，对每个 last_fire + interval_ms <= now 的定时器触发 callback，
    /// 并更新 last_fire。last_fire 初始为 0，使定时器首次 tick 立即触发一次。
    fn tick_timers(&self) {
        if let Err(e) = self.tick_fn.call::<()>(()) {
            log::error!("tick_timers Lua 执行错误: {}", e);
        }
    }

    /// 停止脚本引擎并释放 Lua VM
    fn stop(&mut self) {
        let _ = self.lua.load("__handlers = nil\n__timers = nil").exec();
        log::info!("ScriptEngine 已停止");
    }

    /// 通过 Tauri 事件发送日志消息到前端
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

/// 脚本引擎错误类型
#[derive(Debug, thiserror::Error)]
pub enum ScriptEngineError {
    #[error("Lua 错误: {0}")]
    LuaError(String),
}

impl From<mlua::Error> for ScriptEngineError {
    fn from(e: mlua::Error) -> Self {
        ScriptEngineError::LuaError(e.to_string())
    }
}

/// 启动脚本引擎线程
///
/// 在独立的 std::thread 中创建 Lua VM，通过 channel 接收命令。
/// 返回 JoinHandle 供调用方管理生命周期。
pub fn spawn_script_thread(
    comm: Arc<dyn CommHandle>,
    app_handle: tauri::AppHandle,
    rx: mpsc::Receiver<ScriptCmd>,
    session_id: String,
    shutdown: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut engine = match ScriptEngine::new(comm, app_handle.clone(), &session_id, shutdown) {
            Ok(e) => e,
            Err(e) => {
                log::error!("创建 ScriptEngine 失败: {}", e);
                let _ = app_handle.emit(
                    "script-log",
                    serde_json::json!({
                        "session_id": session_id,
                        "message": format!("脚本引擎初始化失败: {}", e),
                    }),
                );
                return;
            }
        };

        log::info!("ScriptEngine 线程已启动");

        loop {
            // 使用 recv_timeout 替代 recv，确保 Shutdown 命令可被及时处理，
            // 同时在空闲时驱动定时器 tick。Lua sleep() 采用协作式分片睡眠
            // （见 lua_api.rs），停止时经 shutdown 标志中断，join 不会长时阻塞。
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(ScriptCmd::LoadScript(code)) => {
                    if let Err(e) = engine.load_script(&code) {
                        log::error!("脚本加载失败: {}", e);
                        engine.emit_log(&format!("脚本加载失败: {}", e));
                    }
                }
                Ok(ScriptCmd::FeedData(data)) => {
                    engine.feed_data(&data);
                }
                Ok(ScriptCmd::Shutdown) => {
                    engine.stop();
                    log::info!("ScriptEngine 线程退出");
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // 超时：检查并触发到期的定时器规则，同时继续循环检查 Shutdown
                    engine.tick_timers();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    engine.stop();
                    log::info!("ScriptEngine 线程退出 (channel 断开)");
                    break;
                }
            }
        }
    })
}
