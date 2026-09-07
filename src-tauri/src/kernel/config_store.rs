//! 持久化配置存储
//!
//! 为非敏感的应用设置与工程资产索引提供命名空间 KV 存储。
//! 磁盘格式带显式版本；凭据不得进入本存储。

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

const CONFIG_STORE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct PersistedConfigStore {
    version: u32,
    namespaces: HashMap<String, HashMap<String, serde_json::Value>>,
}

/// 非敏感配置的进程级权威存储。
///
/// `configure_persistence()` 在 Tauri setup 阶段绑定 app config 目录并加载磁盘快照；
/// 之后 `set()` / `delete()` 会同步持久化。安全凭据始终由 CredentialStore 管理。
pub struct ConfigStore {
    data: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    persistence_path: RwLock<Option<PathBuf>>,
}

impl ConfigStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            persistence_path: RwLock::new(None),
        }
    }

    /// 绑定磁盘文件并加载当前版本配置。
    ///
    /// 研发阶段不保留旧格式兼容：损坏或版本不匹配的文件会备份后从空配置开始。
    pub fn configure_persistence(&self, path: PathBuf) -> Result<(), ConfigStoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigStoreError::Io)?;
        }

        let loaded = Self::load_file(&path)?;
        {
            let mut data = self.data.write().map_err(|_| ConfigStoreError::LockError)?;
            *data = loaded;
        }
        {
            let mut persisted = self
                .persistence_path
                .write()
                .map_err(|_| ConfigStoreError::LockError)?;
            *persisted = Some(path);
        }
        Ok(())
    }

    fn load_file(
        path: &Path,
    ) -> Result<HashMap<String, HashMap<String, serde_json::Value>>, ConfigStoreError> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let raw = std::fs::read_to_string(path).map_err(ConfigStoreError::Io)?;
        if raw.trim().is_empty() {
            return Ok(HashMap::new());
        }
        match serde_json::from_str::<PersistedConfigStore>(&raw) {
            Ok(snapshot) if snapshot.version == CONFIG_STORE_VERSION => Ok(snapshot.namespaces),
            Ok(snapshot) => {
                Self::backup_invalid(path);
                log::warn!(
                    "配置存储版本不受支持: {} (expected {})，已从空配置启动",
                    snapshot.version,
                    CONFIG_STORE_VERSION
                );
                Ok(HashMap::new())
            }
            Err(error) => {
                Self::backup_invalid(path);
                log::warn!("配置存储损坏: {}，已从空配置启动", error);
                Ok(HashMap::new())
            }
        }
    }

    fn backup_invalid(path: &Path) {
        let backup = path.with_extension("json.invalid.bak");
        let _ = std::fs::copy(path, backup);
    }

    fn persist(&self) -> Result<(), ConfigStoreError> {
        let path = self
            .persistence_path
            .read()
            .map_err(|_| ConfigStoreError::LockError)?
            .clone();
        let Some(path) = path else {
            // setup 之前的极短窗口只保留内存状态；setup 会随后加载并绑定磁盘。
            return Ok(());
        };

        let namespaces = self
            .data
            .read()
            .map_err(|_| ConfigStoreError::LockError)?
            .clone();
        let snapshot = PersistedConfigStore {
            version: CONFIG_STORE_VERSION,
            namespaces,
        };
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| ConfigStoreError::Serialization(e.to_string()))?;
        std::fs::write(path, json).map_err(ConfigStoreError::Io)
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let (ns, k) = Self::parse_key(key)?;
        let data = self.data.read().ok()?;
        let value = data.get(ns)?.get(k)?;
        serde_json::from_value(value.clone()).ok()
    }

    pub fn get_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> T {
        self.get(key).unwrap_or_default()
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), ConfigStoreError> {
        let (ns, k) = Self::parse_key(key).ok_or(ConfigStoreError::InvalidKey(key.to_string()))?;
        let json_value = serde_json::to_value(value)
            .map_err(|e| ConfigStoreError::Serialization(e.to_string()))?;

        {
            let mut data = self.data.write().map_err(|_| ConfigStoreError::LockError)?;
            data.entry(ns.to_string())
                .or_default()
                .insert(k.to_string(), json_value);
        }
        self.persist()
    }

    pub fn delete(&self, key: &str) -> Result<(), ConfigStoreError> {
        let (ns, k) = Self::parse_key(key).ok_or(ConfigStoreError::InvalidKey(key.to_string()))?;
        {
            let mut data = self.data.write().map_err(|_| ConfigStoreError::LockError)?;
            if let Some(namespace) = data.get_mut(ns) {
                namespace.remove(k);
                if namespace.is_empty() {
                    data.remove(ns);
                }
            }
        }
        self.persist()
    }

    pub fn namespace(&self, ns: &str) -> Option<HashMap<String, serde_json::Value>> {
        let data = self.data.read().ok()?;
        data.get(ns).cloned()
    }

    fn parse_key(key: &str) -> Option<(&str, &str)> {
        let (ns, k) = key.split_once('.')?;
        if ns.is_empty() || k.is_empty() {
            None
        } else {
            Some((ns, k))
        }
    }
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("无效的配置键: {0}")]
    InvalidKey(String),
    #[error("序列化失败: {0}")]
    Serialization(String),
    #[error("配置存储 I/O 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("内部锁错误")]
    LockError,
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tauterm-config-store-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn config_store_persists_across_reopen() {
        let dir = temp_path("reopen");
        let path = dir.join("settings.json");

        let store = ConfigStore::new();
        store.configure_persistence(path.clone()).unwrap();
        store.set("workspace.sample", &42_u64).unwrap();

        let reopened = ConfigStore::new();
        reopened.configure_persistence(path.clone()).unwrap();
        assert_eq!(reopened.get::<u64>("workspace.sample"), Some(42));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn config_store_rejects_future_schema_without_migration() {
        let dir = temp_path("future");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"version":999,"namespaces":{"test":{"value":1}}}"#).unwrap();

        let store = ConfigStore::new();
        store.configure_persistence(path.clone()).unwrap();
        assert_eq!(store.get::<u64>("test.value"), None);
        assert!(path.with_extension("json.invalid.bak").exists());

        let _ = std::fs::remove_dir_all(dir);
    }
}
