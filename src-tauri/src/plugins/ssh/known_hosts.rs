//! Persistent SSH host trust store.
//!
//! Unknown hosts require explicit user confirmation. A stored host whose fingerprint changes is
//! rejected fail-closed; the new key is never silently learned.

use crate::kernel::persistence::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

const KNOWN_HOSTS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownHostRecord {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct KnownHostsFile {
    version: u32,
    hosts: HashMap<String, KnownHostRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTrustDecision {
    Trusted,
    Unknown,
    Changed { expected_fingerprint: String },
}

pub struct KnownHostStore {
    path: RwLock<Option<PathBuf>>,
    hosts: RwLock<HashMap<String, KnownHostRecord>>,
    mutation_lock: Mutex<()>,
}

impl KnownHostStore {
    pub fn new() -> Self {
        Self {
            path: RwLock::new(None),
            hosts: RwLock::new(HashMap::new()),
            mutation_lock: Mutex::new(()),
        }
    }

    pub fn configure(&self, path: PathBuf) -> Result<(), String> {
        let _mutation = self
            .mutation_lock
            .lock()
            .map_err(|_| "SSH known-host mutation 锁错误".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("无法创建 SSH 信任目录: {e}"))?;
        }
        let hosts = Self::load(&path)?;
        *self
            .hosts
            .write()
            .map_err(|_| "SSH known-host 锁错误".to_string())? = hosts;
        *self
            .path
            .write()
            .map_err(|_| "SSH known-host 路径锁错误".to_string())? = Some(path);
        Ok(())
    }

    fn load(path: &Path) -> Result<HashMap<String, KnownHostRecord>, String> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 SSH known-host 文件: {e}"))?;
        if raw.trim().is_empty() {
            return Ok(HashMap::new());
        }
        match serde_json::from_str::<KnownHostsFile>(&raw) {
            Ok(file) if file.version == KNOWN_HOSTS_VERSION => Ok(file.hosts),
            Ok(file) => {
                Self::backup_invalid(path)?;
                log::warn!(
                    "SSH known-host 版本 {} 不受支持（expected {}）；将重新询问主机信任",
                    file.version,
                    KNOWN_HOSTS_VERSION
                );
                Ok(HashMap::new())
            }
            Err(error) => {
                Self::backup_invalid(path)?;
                log::warn!("SSH known-host 文件损坏: {error}；将重新询问主机信任");
                Ok(HashMap::new())
            }
        }
    }

    fn backup_invalid(path: &Path) -> Result<(), String> {
        let backup = path.with_extension("json.invalid.bak");
        std::fs::copy(path, backup)
            .map(|_| ())
            .map_err(|e| format!("备份无效 SSH known-host 文件失败: {e}"))
    }

    fn key(host: &str, port: u16) -> String {
        format!("{}:{}", host.trim().to_ascii_lowercase(), port)
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn evaluate(&self, host: &str, port: u16, fingerprint: &str) -> HostTrustDecision {
        let key = Self::key(host, port);
        let Ok(hosts) = self.hosts.read() else {
            return HostTrustDecision::Unknown;
        };
        match hosts.get(&key) {
            None => HostTrustDecision::Unknown,
            Some(record) if record.fingerprint == fingerprint => HostTrustDecision::Trusted,
            Some(record) => HostTrustDecision::Changed {
                expected_fingerprint: record.fingerprint.clone(),
            },
        }
    }

    pub fn touch(&self, host: &str, port: u16) {
        let Ok(_mutation) = self.mutation_lock.lock() else {
            log::warn!("SSH known-host mutation 锁错误");
            return;
        };
        let key = Self::key(host, port);
        let Ok(mut next) = self.hosts.read().map(|hosts| hosts.clone()) else {
            log::warn!("SSH known-host 锁错误");
            return;
        };
        let Some(record) = next.get_mut(&key) else {
            return;
        };
        record.last_seen_ms = Self::now_ms();

        match self.persist_snapshot(&next) {
            Ok(()) => {
                if let Ok(mut hosts) = self.hosts.write() {
                    *hosts = next;
                }
            }
            Err(error) => log::warn!("更新 SSH known-host last_seen 失败: {error}"),
        }
    }

    pub fn trust(&self, host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
        let _mutation = self
            .mutation_lock
            .lock()
            .map_err(|_| "SSH known-host mutation 锁错误".to_string())?;
        let key = Self::key(host, port);
        let now = Self::now_ms();
        let mut next = self
            .hosts
            .read()
            .map_err(|_| "SSH known-host 锁错误".to_string())?
            .clone();
        let first_seen = next
            .get(&key)
            .map(|record| record.first_seen_ms)
            .unwrap_or(now);
        next.insert(
            key,
            KnownHostRecord {
                host: host.to_string(),
                port,
                fingerprint: fingerprint.to_string(),
                first_seen_ms: first_seen,
                last_seen_ms: now,
            },
        );

        self.persist_snapshot(&next)?;
        *self
            .hosts
            .write()
            .map_err(|_| "SSH known-host 锁错误".to_string())? = next;
        Ok(())
    }

    fn persist_snapshot(&self, hosts: &HashMap<String, KnownHostRecord>) -> Result<(), String> {
        let path = self
            .path
            .read()
            .map_err(|_| "SSH known-host 路径锁错误".to_string())?
            .clone()
            .ok_or_else(|| "SSH known-host 存储尚未初始化".to_string())?;
        let file = KnownHostsFile {
            version: KNOWN_HOSTS_VERSION,
            hosts: hosts.clone(),
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("序列化 SSH known-host 失败: {e}"))?;
        atomic_write(&path, json.as_bytes()).map_err(|e| format!("写入 SSH known-host 失败: {e}"))
    }
}

impl Default for KnownHostStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("tauterm-known-hosts-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn failed_trust_write_does_not_create_in_memory_trust() {
        let dir = temp_path();
        let path = dir.join("known_hosts.json");

        let store = KnownHostStore::new();
        store.configure(path).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::write(&dir, b"blocks-directory-recreation").unwrap();

        assert!(store.trust("example.test", 22, "SHA256:first").is_err());
        assert_eq!(
            store.evaluate("example.test", 22, "SHA256:first"),
            HostTrustDecision::Unknown
        );

        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn known_host_trust_persists_and_changed_key_fails_closed() {
        let dir = temp_path();
        let path = dir.join("known_hosts.json");

        let store = KnownHostStore::new();
        store.configure(path.clone()).unwrap();
        assert_eq!(
            store.evaluate("example.test", 22, "SHA256:first"),
            HostTrustDecision::Unknown
        );

        store.trust("example.test", 22, "SHA256:first").unwrap();
        assert_eq!(
            store.evaluate("example.test", 22, "SHA256:first"),
            HostTrustDecision::Trusted
        );
        assert_eq!(
            store.evaluate("example.test", 22, "SHA256:changed"),
            HostTrustDecision::Changed {
                expected_fingerprint: "SHA256:first".to_string(),
            }
        );

        let reopened = KnownHostStore::new();
        reopened.configure(path).unwrap();
        assert_eq!(
            reopened.evaluate("example.test", 22, "SHA256:first"),
            HostTrustDecision::Trusted
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
