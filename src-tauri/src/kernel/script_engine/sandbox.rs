//! Lua 安全沙箱
//!
//! 创建受限的 Lua 运行环境，移除危险的标准库模块，
//! 这里设置内存限制；指令预算与取消统一由 execution 模块拥有。

use mlua::Lua;

fn snapshot_metatable(lua: &Lua, source: mlua::Table) -> mlua::Result<mlua::Table> {
    for name in ["__gc", "__close"] {
        if !matches!(source.raw_get::<mlua::Value>(name)?, mlua::Value::Nil) {
            return Err(mlua::Error::RuntimeError(format!(
                "自动化沙箱不支持自定义 {name} 清理函数"
            )));
        }
    }
    let snapshot = lua.create_table()?;
    for pair in source.pairs::<mlua::Value, mlua::Value>() {
        let (key, value) = pair?;
        snapshot.raw_set(key, value)?;
    }
    Ok(snapshot)
}

fn install_metatable_boundary(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let native_set: mlua::Function = globals.get("setmetatable")?;
    globals.set(
        "setmetatable",
        lua.create_function(
            move |lua, (table, metatable): (mlua::Table, Option<mlua::Table>)| {
                let snapshot = metatable
                    .map(|value| snapshot_metatable(lua, value))
                    .transpose()?;
                native_set.call::<mlua::Table>((table, snapshot))
            },
        )?,
    )?;
    let native_get: mlua::Function = globals.get("getmetatable")?;
    globals.set(
        "getmetatable",
        lua.create_function(move |lua, value: mlua::Value| {
            // Preserve explicitly protected metatables and their public marker value.
            if let mlua::Value::Table(table) = &value {
                if let Some(metatable) = table.metatable() {
                    let protected: mlua::Value = metatable.raw_get("__metatable")?;
                    if !matches!(protected, mlua::Value::Nil) {
                        return Ok(protected);
                    }
                }
            }
            match native_get.call::<mlua::Value>(value)? {
                mlua::Value::Table(metatable) => {
                    snapshot_metatable(lua, metatable).map(mlua::Value::Table)
                }
                other => Ok(other),
            }
        })?,
    )
}

/// 创建沙箱化的 Lua VM
///
/// 安全措施：
/// - 移除 `os` 模块（防止系统调用）
/// - 移除 `io` 模块（防止文件系统访问）
/// - 移除 `require`（防止加载外部 C 模块）
/// - 移除 `dofile`、`loadfile`（防止文件系统访问）
/// - 设置 1MB 内存限制
pub fn create_sandboxed_lua() -> mlua::Result<Lua> {
    let lua = Lua::new();

    // ── 移除危险模块 ──
    let globals = lua.globals();

    // os.execute, os.remove 等可执行系统命令
    globals.set("os", mlua::Value::Nil)?;

    // io.open, io.read 等可读写文件系统
    globals.set("io", mlua::Value::Nil)?;

    // require 可加载外部 C 扩展模块
    globals.set("require", mlua::Value::Nil)?;

    // dofile / loadfile 可从磁盘加载并执行 Lua 文件
    globals.set("dofile", mlua::Value::Nil)?;
    globals.set("loadfile", mlua::Value::Nil)?;

    // load 保留可用 — 代码生成器中的 EXPR 宏需要 load() 进行算术求值，
    // 生成的代码已包含字符串白名单校验（仅允许数字、运算符和括号），风险可控
    // globals.set("load", mlua::Value::Nil)?;  // 保留 load 供 EXPR 宏使用

    // debug 提供自省与元表操作（debug.setmetatable 等），
    // 无 I/O 能力但会暴露 VM 内部结构，无益于脚本用途
    globals.set("debug", mlua::Value::Nil)?;

    // ── 保留的安全模块 ──
    // string, table, math, coroutine 保留 — 纯计算，无 I/O

    // ── 内存限制 ──
    // 限制 Lua VM 总内存分配为 1MB
    lua.set_memory_limit(1024 * 1024)?;
    // Lua disables hooks during GC finalization. Snapshot installation and inspection prevent
    // late metatable mutation from injecting uninterruptible cleanup code after validation.
    install_metatable_boundary(&lua)?;

    Ok(lua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_modules_removed() {
        let lua = create_sandboxed_lua().unwrap();
        let globals = lua.globals();

        assert!(globals.get::<mlua::Value>("os").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("io").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("require").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("dofile").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("loadfile").unwrap().is_nil());
        // load 保留可用供 EXPR 宏使用
        assert!(globals.get::<mlua::Value>("load").unwrap().is_function());
        assert!(globals.get::<mlua::Value>("debug").unwrap().is_nil());
    }

    #[test]
    fn test_safe_modules_preserved() {
        let lua = create_sandboxed_lua().unwrap();

        // string, table, math 应该可用
        lua.load(r#"return string.upper("test")"#)
            .eval::<String>()
            .unwrap();
    }

    #[test]
    fn test_os_execute_blocked() {
        let lua = create_sandboxed_lua().unwrap();
        let result = lua.load(r#"return os == nil"#).eval::<bool>().unwrap();
        assert!(result);
    }

    #[test]
    fn finalizers_are_rejected_and_late_mutation_cannot_install_them() {
        let lua = create_sandboxed_lua().unwrap();
        for name in ["__gc", "__close"] {
            let error = lua
                .load(format!(
                    "setmetatable({{}}, {{ {name}=function() while true do end end }})"
                ))
                .exec()
                .expect_err("unsupported finalizer");
            assert!(error.to_string().contains(name));
        }
        lua.load(
            r#"
            local mt = {__index = {value = 7}}
            local object = setmetatable({}, mt)
            mt.__gc = function() error('must never run') end
            mt.__close = mt.__gc
            local inspected = getmetatable(object)
            inspected.__gc = mt.__gc
            inspected.__close = mt.__close
            assert(object.value == 7)
            assert(getmetatable(object).__gc == nil)
            assert(getmetatable(object).__close == nil)
            setmetatable(object, {__index={value=9}})
            assert(object.value == 9)
            setmetatable(object, nil)
            assert(getmetatable(object) == nil)
            object = nil
            collectgarbage()
            local marker = {}
            local protected = setmetatable({}, {__metatable=marker})
            assert(getmetatable(protected) == marker)
            assert(not pcall(setmetatable, protected, {}))
        "#,
        )
        .exec()
        .unwrap();
    }
}
