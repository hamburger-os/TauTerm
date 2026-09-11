//! Lua API 注入
//!
//! 向 Lua VM 全局环境注入 `send()`、`sleep()`、`log()`、`on_data()` 函数，
//! 以及 regex 匹配引擎和时间工具函数。
//!
//! 所有 handler 存储在 Lua 全局表 `__handlers` 中，完全在 Lua VM 内部管理，
//! 无需从 Rust 侧持有 RegistryKey 引用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mlua::{Function, Lua, Table};
use tauri::Emitter;

use crate::session::SessionIo;

/// 向 Lua 全局环境注入脚本 API
///
/// `shutdown` 为脚本线程共享的关闭标志：`sleep()` 分片睡眠期间检查它，
/// 使停止脚本时长睡眠能及时中断（否则 join 会阻塞整段睡眠时长并卡住全局锁）。
pub fn inject_lua_api(
    lua: &Lua,
    io: Arc<SessionIo>,
    app_handle: tauri::AppHandle,
    session_id: &str,
    shutdown: Arc<AtomicBool>,
) -> mlua::Result<()> {
    let globals = lua.globals();

    let handlers_table = lua.create_table()?;
    globals.set("__handlers", handlers_table)?;
    let timers_table = lua.create_table()?;
    globals.set("__timers", timers_table)?;

    let raw_io = io.clone();
    let send_fn = lua.create_function(move |_, data: mlua::String| {
        let bytes: Vec<u8> = data.as_bytes().to_vec();
        raw_io
            .send(&bytes)
            .map_err(|error| mlua::Error::RuntimeError(format!("send 失败: {error}")))
    })?;
    globals.set("send", send_fn)?;

    let text_io = io.clone();
    let send_text_fn = lua.create_function(move |_, data: mlua::String| {
        let bytes: Vec<u8> = data.as_bytes().to_vec();
        text_io
            .send_text(&bytes)
            .map(|_| ())
            .map_err(|error| mlua::Error::RuntimeError(format!("send_text 失败: {error}")))
    })?;
    globals.set("send_text", send_text_fn)?;

    let targeted_io = io.clone();
    let send_to_fn =
        lua.create_function(move |_, (target, data): (mlua::String, mlua::String)| {
            let target = target.to_str()?.to_string();
            let bytes: Vec<u8> = data.as_bytes().to_vec();
            targeted_io
                .send_to(&target, &bytes)
                .map_err(|error| mlua::Error::RuntimeError(format!("send_to 失败: {error}")))
        })?;
    globals.set("send_to", send_to_fn)?;

    let targeted_text_io = io;
    let send_to_text_fn =
        lua.create_function(move |_, (target, data): (mlua::String, mlua::String)| {
            let target = target.to_str()?.to_string();
            let bytes: Vec<u8> = data.as_bytes().to_vec();
            targeted_text_io
                .send_to_text(&target, &bytes)
                .map(|_| ())
                .map_err(|error| mlua::Error::RuntimeError(format!("send_to_text 失败: {error}")))
        })?;
    globals.set("send_to_text", send_to_text_fn)?;

    let sleep_shutdown = shutdown.clone();
    let sleep_fn = lua.create_function(move |_, ms: u64| {
        const SLICE_MS: u64 = 50;
        let mut remaining = ms;
        while remaining > 0 {
            if sleep_shutdown.load(Ordering::Relaxed) {
                break;
            }
            let chunk = remaining.min(SLICE_MS);
            std::thread::sleep(Duration::from_millis(chunk));
            remaining -= chunk;
        }
        Ok(())
    })?;
    globals.set("sleep", sleep_fn)?;

    let app_log = app_handle.clone();
    let sid = session_id.to_string();
    let log_fn = lua.create_function(move |_, msg: mlua::String| {
        let text = msg.to_str()?.to_string();
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        let formatted = format!("[{timestamp}] {text}");
        let _ = app_log.emit(
            "script-log",
            serde_json::json!({
                "session_id": sid,
                "message": formatted,
            }),
        );
        Ok(())
    })?;
    globals.set("log", log_fn)?;

    let on_data_fn =
        lua.create_function(|lua, (pattern, callback): (mlua::String, Function)| {
            let pattern_str = pattern.to_str()?.to_string();
            let globals = lua.globals();
            let handlers: Table = globals.get("__handlers")?;
            let entry = lua.create_table()?;
            entry.set("pattern", pattern_str)?;
            entry.set("callback", callback)?;
            let len: i64 = handlers.len()?;
            handlers.set(len + 1, entry)?;
            Ok(())
        })?;
    globals.set("on_data", on_data_fn)?;

    let register_timer_fn = lua.create_function(
        |lua, (id, interval_ms, callback): (mlua::String, u64, Function)| {
            let id_str = id.to_str()?.to_string();
            let globals = lua.globals();
            let timers: Table = globals.get("__timers")?;
            let entry = lua.create_table()?;
            entry.set("id", id_str)?;
            entry.set("interval_ms", interval_ms.max(1) as f64)?;
            entry.set("last_fire", 0.0f64)?;
            entry.set("callback", callback)?;
            let len: i64 = timers.len()?;
            timers.set(len + 1, entry)?;
            Ok(())
        },
    )?;
    globals.set("register_timer", register_timer_fn)?;

    let unregister_timer_fn = lua.create_function(|lua, id: mlua::String| {
        let id_str = id.to_str()?.to_string();
        let globals = lua.globals();
        let timers: Table = globals.get("__timers")?;
        let kept = lua.create_table()?;
        let mut idx = 1i64;
        for pair in timers.sequence_values::<Table>() {
            let timer = pair?;
            let timer_id: String = timer.get("id")?;
            if timer_id != id_str {
                kept.set(idx, timer)?;
                idx += 1;
            }
        }
        globals.set("__timers", kept)?;
        Ok(())
    })?;
    globals.set("unregister_timer", unregister_timer_fn)?;

    let regex_find_fn =
        lua.create_function(|lua, (pattern, data): (mlua::String, mlua::String)| {
            let pat_str = pattern.to_str()?;
            let data_str = data.to_str()?;
            let re = regex::Regex::new(&pat_str).map_err(|error| {
                mlua::Error::RuntimeError(format!("正则表达式语法错误: {error}"))
            })?;
            if let Some(caps) = re.captures(&data_str) {
                let result = lua.create_table()?;
                for (index, cap) in caps.iter().enumerate() {
                    if let Some(value) = cap {
                        result.set(index, value.as_str().to_string())?;
                    }
                }
                Ok(mlua::Value::Table(result))
            } else {
                Ok(mlua::Value::Nil)
            }
        })?;
    globals.set("regex_find", regex_find_fn)?;

    let time_ms_fn = lua.create_function(|_, _: ()| {
        let ts = chrono::Utc::now().timestamp_millis() as f64;
        Ok(ts)
    })?;
    globals.set("_time_ms", time_ms_fn)?;

    let datetime_iso_fn = lua.create_function(|_, _: ()| {
        let value = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        Ok(value)
    })?;
    globals.set("_datetime_iso", datetime_iso_fn)?;

    let datetime_format_fn = lua.create_function(|_, fmt: mlua::String| {
        let format_str = fmt.to_str()?;
        let value = chrono::Local::now().format(&format_str).to_string();
        Ok(value)
    })?;
    globals.set("_datetime_format", datetime_format_fn)?;

    Ok(())
}
