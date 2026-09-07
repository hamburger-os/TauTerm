//! Persistent SSH host trust store.
//!
//! Unknown hosts require explicit user confirmation. A stored host whose fingerprint changes is
//! rejected fail-closed; the new key is never silently learned.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

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

#[derive(Debug, Clone)]
pub enum HostTrustDecision {
    Trusted,
    Unknown,
    Changed { expected_fingerprint: String },
}

pub struct KnownHostStore {
    path: RwLock<Option<PathBuf>>,
    hosts: RwLock<HashMap<String, KnownHostRecord>>,
}

impl KnownHostStore {
    pub fn new() -> Self {
        Self {
            path: RwLock::new(None),
            hosts: RwLock::new(HashMap::new()),
        }
    }

    pub fn configure(&self, path: PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建 SSH 信任目录: {e}"))?;
        }
        let hosts = Self::load(&path)?;
        *self.hosts.write().map_err(|_| "SSH known-host 锁错误".to_string())? = hosts;
        *self.path.write().map_err(|_| "SSH known-host 路径锁错误".to_string())? = Some(path);
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
                Self::backup_invalid(path);
                log::warn!(
                    "SSH known-host 版本 {} 不受支持（expected {}）；将重新询问主机信任",
                    file.version,
                    KNOWN_HOSTS_VERSION
                );
                Ok(HashMap::new())
            }
            Err(error) => {
                Self::backup_invalid(path);
                log::warn!("SSH known-host 文件损坏: {error}；将重新询问主机信任");
                Ok(HashMap::new())
            }
        }
    }

    fn backup_invalid(path: &Path) {
        let backup = path.with_extension("json.invalid.bak");
        let _ = std::fs::copy(path, backup);
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
        let key = Self::key(host, port);
        if let Ok(mut hosts) = self.hosts.write() {
            if let Some(record) = hosts.get_mut(&key) {
                record.last_seen_ms = Self::now_ms();
            }
        }
        if let Err(error) = self.persist() {
            log::warn!("更新 SSH known-host last_seen 失败: {error}");
        }
    }

    pub fn trust(&self, host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
        let key = Self::key(host, port);
        let now = Self::now_ms();
        {
            let mut hosts = self
                .hosts
                .write()
                .map_err(|_| "SSH known-host 锁错误".to_string())?;
            let first_seen = hosts
                .get(&key)
                .map(|record| record.first_seen_ms)
                .unwrap_or(now);
            hosts.insert(
                key,
                KnownHostRecord {
                    host: host.to_string(),
                    port,
                    fingerprint: fingerprint.to_string(),
                    first_seen_ms: first_seen,
                    last_seen_ms: now,
                },
            );
        }
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let path = self
            .path
            .read()
            .map_err(|_| "SSH known-host 路径锁错误".to_string())?
            .clone()
            .ok_or_else(|| "SSH known-host 存储尚未初始化".to_string())?;
        let hosts = self
            .hosts
            .read()
            .map_err(|_| "SSH known-host 锁错误".to_string())?
            .clone();
        let file = KnownHostsFile {
            version: KNOWN_HOSTS_VERSION,
            hosts,
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("序列化 SSH known-host 失败: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("写入 SSH known-host 失败: {e}"))
    }
}

impl Default for KnownHostStore {
    fn default() -> Self {
        Self::new()
    }
}
