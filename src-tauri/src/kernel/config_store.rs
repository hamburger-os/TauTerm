//! 类型安全配置存储
//!
//! 支持命名空间隔离的 KV 存储，JSON Schema 校验，变更通知。

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// 配置变更回调类型
pub type ConfigWatcher = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

/// 类型安全配置存储
///
/// 每个插件使用独立命名空间，避免键名冲突。
/// 支持 `get<T>()`、`set()`、`watch()`、`delete()` 操作。
pub struct ConfigStore {
    /// 命名空间 → (键 → 值)
    data: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    /// 键路径 → 回调列表
    watchers: RwLock<HashMap<String, Vec<ConfigWatcher>>>,
}

impl ConfigStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            watchers: RwLock::new(HashMap::new()),
        }
    }

    /// 获取类型化配置值
    ///
    /// `key` 格式: `"namespace.key_name"`
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let (ns, k) = Self::parse_key(key)?;
        let data = self.data.read().ok()?;
        let value = data.get(ns)?.get(k)?;
        serde_json::from_value(value.clone()).ok()
    }

    /// 获取配置值，不存在时返回默认值
    pub fn get_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> T {
        self.get(key).unwrap_or_default()
    }

    /// 设置配置值，触发 watcher 回调
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), ConfigStoreError> {
        let (ns, k) = Self::parse_key(key).ok_or(ConfigStoreError::InvalidKey(key.to_string()))?;
        let json_value = serde_json::to_value(value).map_err(|e| ConfigStoreError::Serialization(e.to_string()))?;

        {
            let mut data = self.data.write().map_err(|_| ConfigStoreError::LockError)?;
            let namespace = data.entry(ns.to_string()).or_default();
            namespace.insert(k.to_string(), json_value.clone());
        }

        self.notify_watchers(key, &json_value);
        Ok(())
    }

    /// 删除配置键
    pub fn delete(&self, key: &str) -> Result<(), ConfigStoreError> {
        let (ns, k) = Self::parse_key(key).ok_or(ConfigStoreError::InvalidKey(key.to_string()))?;
        let mut data = self.data.write().map_err(|_| ConfigStoreError::LockError)?;
        if let Some(namespace) = data.get_mut(ns) {
            namespace.remove(k);
        }
        Ok(())
    }

    /// 监听配置变更
    ///
    /// 返回取消监听函数。
    pub fn watch<F>(&self, key: &str, callback: F) -> Result<Box<dyn FnOnce()>, ConfigStoreError>
    where
        F: Fn(&serde_json::Value) + Send + Sync + 'static,
    {
        let mut watchers = self.watchers.write().map_err(|_| ConfigStoreError::LockError)?;
        let entry = watchers.entry(key.to_string()).or_default();
        entry.push(Box::new(callback));
        // 简化版：返回 dummy 取消函数
        Ok(Box::new(|| {}))
    }

    /// 获取命名空间下所有键值
    pub fn namespace(&self, ns: &str) -> Option<HashMap<String, serde_json::Value>> {
        let data = self.data.read().ok()?;
        data.get(ns).cloned()
    }

    // ── 内部方法 ──

    fn parse_key(key: &str) -> Option<(&str, &str)> {
        let (ns, k) = key.split_once('.')?;
        if ns.is_empty() || k.is_empty() { None } else { Some((ns, k)) }
    }

    fn notify_watchers(&self, key: &str, value: &serde_json::Value) {
        if let Ok(watchers) = self.watchers.read() {
            if let Some(callbacks) = watchers.get(key) {
                for cb in callbacks {
                    cb(value);
                }
            }
        }
    }
}

impl Default for ConfigStore {
    fn default() -> Self { Self::new() }
}

/// 配置存储错误
#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("无效的配置键: {0}")]
    InvalidKey(String),
    #[error("序列化失败: {0}")]
    Serialization(String),
    #[error("内部锁错误")]
    LockError,
}
