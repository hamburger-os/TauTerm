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

use crate::kernel::comm_handle::CommHandle;

/// 向 Lua 全局环境注入脚本 API
///
/// `shutdown` 为脚本线程共享的关闭标志：`sleep()` 分片睡眠期间检查它，
/// 使停止脚本时长睡眠能及时中断（否则 join 会阻塞整段睡眠时长并卡住全局锁）。
pub fn inject_lua_api(
    lua: &Lua,
    comm: Arc<dyn CommHandle>,
    app_handle: tauri::AppHandle,
    session_id: &str,
    shutdown: Arc<AtomicBool>,
) -> mlua::Result<()> {
    let globals = lua.globals();

    // 初始化 handler 表（由 Lua 管理）
    let handlers_table = lua.create_table()?;
    globals.set("__handlers", handlers_table)?;

    // 初始化定时器表（由 register_timer 填充，Rust 侧 tick_timers 遍历）
    let timers_table = lua.create_table()?;
    globals.set("__timers", timers_table)?;

    // ── send(data) ──
    let comm_send = comm.clone();
    let send_fn = lua.create_function(move |_, data: mlua::String| {
        let bytes: Vec<u8> = data.as_bytes().to_vec();
        comm_send
            .send(&bytes)
            .map_err(|e| mlua::Error::RuntimeError(format!("send 失败: {}", e)))
    })?;
    globals.set("send", send_fn)?;

    // ── sleep(ms) ──
    // 协作式分片睡眠：每 50ms 检查一次 shutdown 标志，使停止脚本时能及时中断，
    // 避免长睡眠期间无法响应 Shutdown 导致 join 阻塞（并卡住 SessionStore 全局锁）。
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

    // ── log(message) ──
    let app_log = app_handle.clone();
    let sid = session_id.to_string();
    let log_fn = lua.create_function(move |_, msg: mlua::String| {
        let text = msg.to_str()?.to_string();
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        let formatted = format!("[{}] {}", timestamp, text);
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

    // ── on_data(pattern, callback) ──
    // 存储 { pattern = pattern_str, callback = callback_fn } 到 __handlers 表中
    let on_data_fn = lua.create_function(
        |lua, (pattern, callback): (mlua::String, Function)| {
            let pattern_str = pattern.to_str()?.to_string();
            let globals = lua.globals();
            let handlers: Table = globals.get("__handlers")?;

            let entry = lua.create_table()?;
            entry.set("pattern", pattern_str)?;
            entry.set("callback", callback)?;

            // 追加到 handlers 表
            let len: i64 = handlers.len()?;
            handlers.set(len + 1, entry)?;

            Ok(())
        },
    )?;
    globals.set("on_data", on_data_fn)?;

    // ── register_timer(id, interval_ms, callback) ──
    // 注册周期定时器。存储 { id, interval_ms, last_fire = 0, callback } 到 __timers。
    // last_fire = 0 使定时器首次 tick 立即触发一次。
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

    // ── unregister_timer(id) ──
    // 从 __timers 中移除指定 id 的定时器（就地重建表以保持 ipairs 连续）。
    let unregister_timer_fn = lua.create_function(|lua, id: mlua::String| {
        let id_str = id.to_str()?.to_string();
        let globals = lua.globals();
        let timers: Table = globals.get("__timers")?;
        let kept = lua.create_table()?;
        let mut idx = 1i64;
        for pair in timers.sequence_values::<Table>() {
            let t = pair?;
            let tid: String = t.get("id")?;
            if tid != id_str {
                kept.set(idx, t)?;
                idx += 1;
            }
        }
        globals.set("__timers", kept)?;
        Ok(())
    })?;
    globals.set("unregister_timer", unregister_timer_fn)?;

    // ── regex_find(pattern, data) → captures table or nil ──
    // 使用 Rust regex crate 提供完整的正则表达式支持。
    // 返回的捕获组表是 0-indexed（[0] = 完整匹配, [1] = 第一个捕获组...）
    let regex_find_fn = lua.create_function(
        |lua, (pattern, data): (mlua::String, mlua::String)| {
            let pat_str = pattern.to_str()?;
            let data_str = data.to_str()?;

            let re = regex::Regex::new(&pat_str).map_err(|e| {
                mlua::Error::RuntimeError(format!("正则表达式语法错误: {}", e))
            })?;

            if let Some(caps) = re.captures(&data_str) {
                let result = lua.create_table()?;
                for (i, cap) in caps.iter().enumerate() {
                    if let Some(m) = cap {
                        result.set(i, m.as_str().to_string())?;
                    }
                }
                Ok(mlua::Value::Table(result))
            } else {
                Ok(mlua::Value::Nil)
            }
        },
    )?;
    globals.set("regex_find", regex_find_fn)?;

    // ── _time_ms() → Unix 毫秒时间戳 ──
    let time_ms_fn = lua.create_function(|_, _: ()| {
        let ts = chrono::Utc::now().timestamp_millis() as f64;
        Ok(ts)
    })?;
    globals.set("_time_ms", time_ms_fn)?;

    // ── _datetime_iso() → ISO 8601 格式日期时间 ──
    let datetime_iso_fn = lua.create_function(|_, _: ()| {
        let s = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        Ok(s)
    })?;
    globals.set("_datetime_iso", datetime_iso_fn)?;

    // ── _datetime_format(fmt) → 自定义 strftime 格式 ──
    let datetime_format_fn = lua.create_function(|_, fmt: mlua::String| {
        let format_str = fmt.to_str()?;
        let s = chrono::Local::now().format(&format_str).to_string();
        Ok(s)
    })?;
    globals.set("_datetime_format", datetime_format_fn)?;

    Ok(())
}
