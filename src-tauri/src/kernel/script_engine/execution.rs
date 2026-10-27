//! Bounded Lua execution. Each host entry gets a fresh instruction budget; the VM may stay
//! resident indefinitely between entries. Protected calls propagate host interruption, including
//! non-yieldable native-library callbacks such as table.sort comparators.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mlua::{Function, HookTriggers, IntoLuaMulti, Lua, MultiValue, Thread, ThreadStatus, VmState};

const INSTRUCTIONS_PER_HOOK: u32 = 1_000;
const INSTRUCTIONS_PER_ENTRY: u32 = 1_000_000;

#[derive(Clone, Copy)]
enum Interruption {
    Cancelled,
    BudgetExceeded,
}

struct ExecutionSlice {
    remaining: Cell<u32>,
    interruption: Cell<Option<Interruption>>,
}

#[derive(Clone)]
pub(super) struct ScriptExecution {
    shutdown: Arc<AtomicBool>,
    active: Rc<RefCell<Option<Rc<ExecutionSlice>>>>,
    instruction_limit: u32,
    protected_call: Function,
}

impl ScriptExecution {
    pub(super) fn new(lua: &Lua, shutdown: Arc<AtomicBool>) -> mlua::Result<Self> {
        let execution = Self {
            shutdown,
            active: Rc::default(),
            instruction_limit: INSTRUCTIONS_PER_ENTRY,
            protected_call: lua.globals().get("pcall")?,
        };
        execution.install_execution_boundaries(lua)?;
        Ok(execution)
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    fn charge(&self, slice: &ExecutionSlice) -> mlua::Result<()> {
        if self.is_cancelled() {
            slice.interruption.set(Some(Interruption::Cancelled));
        } else if slice.remaining.get() <= INSTRUCTIONS_PER_HOOK {
            slice.interruption.set(Some(Interruption::BudgetExceeded));
        } else {
            slice
                .remaining
                .set(slice.remaining.get() - INSTRUCTIONS_PER_HOOK);
        }
        Self::check_interruption(slice)
    }

    fn check_interruption(slice: &ExecutionSlice) -> mlua::Result<()> {
        match slice.interruption.get() {
            Some(Interruption::Cancelled) => Err(mlua::Error::RuntimeError("脚本已取消".into())),
            Some(Interruption::BudgetExceeded) => Err(mlua::Error::RuntimeError(
                "单次脚本执行超过指令预算；请将持续工作拆分为数据或定时回调".into(),
            )),
            None => Ok(()),
        }
    }

    fn hook_thread(&self, thread: &Thread, slice: Rc<ExecutionSlice>) {
        let execution = self.clone();
        thread.set_hook(
            HookTriggers::new().every_nth_instruction(INSTRUCTIONS_PER_HOOK),
            move |_, _| {
                execution.charge(&slice)?;
                Ok(VmState::Continue)
            },
        );
    }

    /// mlua 0.10 hooks target one thread. Rebind around every user coroutine resume so a
    /// coroutine cannot bypass the same entry budget, and restore the caller's hook afterwards.
    fn install_execution_boundaries(&self, lua: &Lua) -> mlua::Result<()> {
        // The interruption flag belongs to the host entry. Native pcall/xpcall may catch a Lua
        // error, but cannot clear that flag or turn an interrupted entry into success.
        for name in ["pcall", "xpcall"] {
            let original = self.protected_call.clone();
            let execution = self.clone();
            lua.globals().set(
                name,
                lua.create_function(move |lua, mut args: MultiValue| {
                    let handler = if name == "xpcall" {
                        if args.len() < 2 {
                            return Err(mlua::Error::RuntimeError(
                                "xpcall 需要函数和错误处理器".into(),
                            ));
                        }
                        let handler = args.remove(1).expect("xpcall handler argument");
                        if !matches!(handler, mlua::Value::Function(_)) {
                            return Err(mlua::Error::RuntimeError(
                                "xpcall 错误处理器必须是函数".into(),
                            ));
                        }
                        Some(handler)
                    } else {
                        None
                    };
                    let mut result = original.call::<MultiValue>(args)?;
                    if let Some(slice) = execution.active.borrow().as_ref() {
                        Self::check_interruption(slice)?;
                    }
                    // Lua's native xpcall error-handler phase disables instruction hooks. Run
                    // the handler as a separate protected call after the first call unwinds.
                    if let Some(handler) = handler {
                        if matches!(result.front(), Some(mlua::Value::Boolean(false))) {
                            let error = result.get(1).cloned().unwrap_or(mlua::Value::Nil);
                            let handled = original
                                .call::<MultiValue>(MultiValue::from_vec(vec![handler, error]))?;
                            if let Some(slice) = execution.active.borrow().as_ref() {
                                Self::check_interruption(slice)?;
                            }
                            let error =
                                if matches!(handled.front(), Some(mlua::Value::Boolean(true))) {
                                    handled.get(1).cloned().unwrap_or(mlua::Value::Nil)
                                } else {
                                    mlua::Value::String(
                                        lua.create_string("error in error handling")?,
                                    )
                                };
                            result = MultiValue::from_vec(vec![mlua::Value::Boolean(false), error]);
                        }
                    }
                    Ok(result)
                })?,
            )?;
        }
        let coroutine: mlua::Table = lua.globals().get("coroutine")?;
        let original_resume: Function = coroutine.get("resume")?;
        let execution = self.clone();
        coroutine.set(
            "resume",
            lua.create_function(move |lua, mut args: MultiValue| {
                let thread = match args.front() {
                    Some(mlua::Value::Thread(thread)) => thread.clone(),
                    _ => return original_resume.call::<MultiValue>(args),
                };
                let slice =
                    execution.active.borrow().clone().ok_or_else(|| {
                        mlua::Error::RuntimeError("脚本不在受控执行入口中".into())
                    })?;
                execution.charge(&slice)?;
                let caller = lua.current_thread();
                execution.hook_thread(&thread, slice.clone());
                // Keep Lua's native resume return convention: success flag followed by values.
                let result = original_resume.call::<MultiValue>(std::mem::take(&mut args));
                execution.hook_thread(&caller, slice.clone());
                Self::check_interruption(&slice)?;
                result
            })?,
        )?;
        lua.load(
            r#"
            local create, resume = coroutine.create, coroutine.resume
            local pack, unpack, raise = table.pack, table.unpack, error
            coroutine.wrap = function(callback)
                local thread = create(callback)
                return function(...)
                    local result = pack(resume(thread, ...))
                    if not result[1] then raise(result[2], 2) end
                    return unpack(result, 2, result.n)
                end
            end
        "#,
        )
        .exec()
    }

    pub(super) fn call(
        &self,
        lua: &Lua,
        function: Function,
        args: impl IntoLuaMulti,
    ) -> mlua::Result<MultiValue> {
        if self.is_cancelled() {
            return Err(mlua::Error::RuntimeError("脚本已取消".into()));
        }
        let slice = Rc::new(ExecutionSlice {
            remaining: Cell::new(self.instruction_limit),
            interruption: Cell::new(None),
        });
        // Catch raw Lua error values before mlua's traceback formatter can invoke an untrusted
        // __tostring on its unhooked main thread. Never format a user error table/userdata.
        let thread = lua.create_thread(self.protected_call.clone())?;
        let mut args = args.into_lua_multi(lua)?;
        args.push_front(mlua::Value::Function(function));
        *self.active.borrow_mut() = Some(slice.clone());
        self.hook_thread(&thread, slice.clone());
        let result = thread.resume::<MultiValue>(args);
        self.active.borrow_mut().take();
        lua.remove_hook();
        Self::check_interruption(&slice)?;
        if result.is_ok() && thread.status() == ThreadStatus::Resumable {
            Err(mlua::Error::RuntimeError("脚本入口不能直接 yield".into()))
        } else {
            let mut values = result?;
            if matches!(values.pop_front(), Some(mlua::Value::Boolean(true))) {
                Ok(values)
            } else {
                let error = values.pop_front().unwrap_or(mlua::Value::Nil);
                let message = match error {
                    mlua::Value::String(value) => value.to_string_lossy(),
                    // This variant is an mlua-owned Rust error, not a user metatable.
                    mlua::Value::Error(error) => error.to_string(),
                    other => format!("Lua raised a {} error value", other.type_name()),
                };
                Err(mlua::Error::RuntimeError(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::script_engine::sandbox::create_sandboxed_lua;
    use std::sync::mpsc;
    use std::time::Duration;

    fn run(code: &str) -> mlua::Result<MultiValue> {
        let lua = create_sandboxed_lua()?;
        let mut execution = ScriptExecution::new(&lua, Arc::new(AtomicBool::new(false)))?;
        execution.instruction_limit = 10_000;
        execution.call(&lua, lua.load(code).into_function()?, ())
    }

    #[test]
    fn instruction_budget_stops_loops_and_cannot_be_swallowed_by_pcall() {
        for code in [
            "while true do end",
            "while true do pcall(function() while true do end end) end",
            "coroutine.resume(coroutine.create(function() while true do end end))",
            "coroutine.wrap(function() while true do end end)()",
            "while true do coroutine.resume(coroutine.create(function() return 1 end)) end",
            "pcall(function() table.sort({3,2,1},function() while true do end end) end)",
            "xpcall(function() while true do end end, function() while true do end end)",
            "xpcall(function() error('trigger') end, function() while true do end end)",
        ] {
            let error = run(code).expect_err(code);
            assert!(error.to_string().contains("指令预算"), "{code}: {error}");
        }
    }

    #[test]
    fn each_callback_has_a_fresh_budget_and_coroutines_keep_their_return_values() {
        let lua = create_sandboxed_lua().expect("lua");
        let mut execution =
            ScriptExecution::new(&lua, Arc::new(AtomicBool::new(false))).expect("execution");
        execution.instruction_limit = 10_000;
        let callback = lua
            .load("local n=0; for i=1,2000 do n=n+i end; return n")
            .into_function()
            .expect("callback");
        for _ in 0..20 {
            let values = execution
                .call(&lua, callback.clone(), ())
                .expect("independent budget");
            assert_eq!(values.front(), Some(&mlua::Value::Integer(2_001_000)));
        }
        assert_eq!(run("local co=coroutine.wrap(function() coroutine.yield(7); return 9 end); return co(),co()")
            .expect("coroutine values").into_vec(), vec![mlua::Value::Integer(7), mlua::Value::Integer(9)]);
    }

    #[test]
    fn protected_calls_preserve_normal_errors_without_running_error_object_formatters() {
        let values = run(r#"
            local ok, value = pcall(function() error('ordinary') end)
            assert(not ok and value:find('ordinary'))
            local handled, message = xpcall(function() error('failure') end, function() return 'handled' end)
            assert(not handled and message == 'handled')
            local success, first, second = xpcall(function(a,b) return a,b end, function() end, 7,9)
            assert(success and first == 7 and second == 9)
            return 1
        "#).expect("normal protected calls");
        assert_eq!(values.front(), Some(&mlua::Value::Integer(1)));
        let error = run("error(setmetatable({}, {__tostring=function() while true do end end}))")
            .expect_err("raw table error");
        assert!(error.to_string().contains("table error value"));
        let error =
            run("tostring(setmetatable({}, {__tostring=function() while true do end end}))")
                .expect_err("explicit tostring is budgeted");
        assert!(error.to_string().contains("指令预算"));

        let lua = create_sandboxed_lua().expect("lua");
        let execution =
            ScriptExecution::new(&lua, Arc::new(AtomicBool::new(false))).expect("execution");
        let host_failure = lua
            .create_function(|_, ()| -> mlua::Result<()> {
                Err(mlua::Error::RuntimeError("transport closed".into()))
            })
            .expect("host callback");
        let error = execution
            .call(&lua, host_failure, ())
            .expect_err("host error");
        assert!(error.to_string().contains("transport closed"));
    }

    #[test]
    fn cancellation_interrupts_an_active_computation() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let lua = create_sandboxed_lua().expect("lua");
            lua.globals()
                .set(
                    "started",
                    lua.create_function(move |_, ()| {
                        started_tx.send(()).expect("signal");
                        Ok(())
                    })
                    .expect("callback"),
                )
                .expect("global");
            let mut execution = ScriptExecution::new(&lua, worker_shutdown).expect("execution");
            execution.instruction_limit = u32::MAX;
            let result = execution.call(
                &lua,
                lua.load("started(); while true do end")
                    .into_function()
                    .expect("script"),
                (),
            );
            done_tx
                .send(result.map(|_| ()).map_err(|error| error.to_string()))
                .expect("result");
        });
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("running");
        shutdown.store(true, Ordering::Release);
        let error = done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled promptly")
            .expect_err("cancelled");
        assert!(error.contains("脚本已取消"), "{error}");
        worker.join().expect("worker");
    }
}
