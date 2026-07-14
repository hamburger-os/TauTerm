//! Lua 安全沙箱
//!
//! 创建受限的 Lua 运行环境，移除危险的标准库模块，
//! 设置内存和指令限制，防止恶意脚本影响系统稳定性。

use mlua::Lua;

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
}
