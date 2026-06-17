//! 快捷键引擎
//!
//! 全局和插件作用域快捷键注册、冲突检测、作用域分发。

use std::collections::HashMap;
use std::sync::RwLock;

/// 快捷键作用域
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ShortcutScope {
    /// 全局快捷键（始终生效）
    Global,
    /// 插件作用域（仅当该插件的标签页活跃时生效）
    Plugin(String),
}

/// 快捷键动作（使用 Arc 以支持跨锁边界共享）
pub type ShortcutAction = std::sync::Arc<dyn Fn() + Send + Sync>;

/// 快捷键注册条目
struct ShortcutEntry {
    _keys: String,
    description: String,
    scope: ShortcutScope,
    action: ShortcutAction,
}

/// 快捷键引擎
pub struct ShortcutEngine {
    shortcuts: RwLock<HashMap<String, Vec<ShortcutEntry>>>,
}

impl ShortcutEngine {
    pub fn new() -> Self {
        Self {
            shortcuts: RwLock::new(HashMap::new()),
        }
    }

    /// 注册快捷键
    ///
    /// 返回 `Err` 如果快捷键与已有注册冲突。
    pub fn register(
        &self,
        keys: &str,
        description: &str,
        scope: ShortcutScope,
        action: ShortcutAction,
    ) -> Result<(), ShortcutError> {
        let normalized = Self::normalize_keys(keys);
        let mut shortcuts = self.shortcuts.write().map_err(|_| ShortcutError::LockError)?;
        let entry = ShortcutEntry {
            _keys: keys.to_string(),
            description: description.to_string(),
            scope,
            action,
        };

        // 冲突检测：同一组合键 + 相同作用域
        if let Some(existing) = shortcuts.get(&normalized) {
            for e in existing {
                if e.scope == entry.scope {
                    return Err(ShortcutError::Conflict {
                        keys: keys.to_string(),
                        existing: e.description.clone(),
                    });
                }
            }
        }

        shortcuts.entry(normalized).or_default().push(entry);
        Ok(())
    }

    /// 查找匹配的快捷键
    pub fn find(
        &self,
        keys: &str,
        active_plugin_id: Option<&str>,
    ) -> Option<ShortcutAction> {
        let normalized = Self::normalize_keys(keys);
        let shortcuts = self.shortcuts.read().ok()?;
        let entries = shortcuts.get(&normalized)?;

        // 优先级: 活跃插件作用域 > 全局
        if let Some(plugin_id) = active_plugin_id {
            for entry in entries {
                if entry.scope == ShortcutScope::Plugin(plugin_id.to_string()) {
                    return Some(entry.action.clone());
                }
            }
        }

        for entry in entries {
            if entry.scope == ShortcutScope::Global {
                return Some(entry.action.clone());
            }
        }

        None
    }

    fn normalize_keys(keys: &str) -> String {
        let mut parts: Vec<&str> = keys.split('+').map(|s| s.trim()).collect();
        parts.sort();
        parts.join("+")
    }
}

impl Default for ShortcutEngine {
    fn default() -> Self { Self::new() }
}

/// 快捷键错误
#[derive(Debug, thiserror::Error)]
pub enum ShortcutError {
    #[error("快捷键冲突: '{keys}' 已被 '{existing}' 使用")]
    Conflict { keys: String, existing: String },
    #[error("内部锁错误")]
    LockError,
}
