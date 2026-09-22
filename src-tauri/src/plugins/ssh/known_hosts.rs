//! Versioned persistent SSH host trust store.
//!
//! The store is intentionally fail-closed. Unsupported or malformed trust files are quarantined
//! for diagnostics, but the running process never silently converts them into a fresh TOFU store.

use crate::kernel::persistence::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

const KNOWN_HOSTS_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownHostKeyRecord {
    pub algorithm: String,
    pub fingerprint: String,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownHostRecord {
    pub host: String,
    pub port: u16,
    pub keys: Vec<KnownHostKeyRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KnownHostsFile {
    version: u32,
    hosts: BTreeMap<String, KnownHostRecord>,
}

#[derive(Debug, Deserialize)]
struct KnownHostsVersion {
    version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTrustDecision {
    Trusted,
    FirstSeen,
    AdditionalKey {
        known_algorithms: Vec<String>,
    },
    Changed {
        algorithm: String,
        expected_fingerprints: Vec<String>,
    },
    Unavailable {
        reason: String,
    },
}

pub struct KnownHostStore {
    path: RwLock<Option<PathBuf>>,
    hosts: RwLock<BTreeMap<String, KnownHostRecord>>,
    mutation_lock: Mutex<()>,
    available: AtomicBool,
}

impl KnownHostStore {
    pub fn new() -> Self {
        Self {
            path: RwLock::new(None),
            hosts: RwLock::new(BTreeMap::new()),
            mutation_lock: Mutex::new(()),
            available: AtomicBool::new(false),
        }
    }

    pub fn configure(&self, path: PathBuf) -> Result<(), String> {
        let _mutation = self
            .mutation_lock
            .lock()
            .map_err(|_| "SSH known-host mutation 锁错误".to_string())?;
        self.available.store(false, Ordering::Release);

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
        self.available.store(true, Ordering::Release);
        Ok(())
    }

    fn load(path: &Path) -> Result<BTreeMap<String, KnownHostRecord>, String> {
        if !path.exists() {
            return Ok(BTreeMap::new());
        }

        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 SSH known-host 文件: {e}"))?;
        if raw.trim().is_empty() {
            return Ok(BTreeMap::new());
        }

        let version = match serde_json::from_str::<KnownHostsVersion>(&raw) {
            Ok(header) => header.version,
            Err(error) => {
                let quarantine = Self::quarantine_invalid(path)?;
                return Err(format!(
                    "SSH known-host 文件损坏: {error}；原文件已隔离至 {:?}，必须显式重置信任后才能继续",
                    quarantine
                ));
            }
        };

        if version != KNOWN_HOSTS_VERSION {
            let quarantine = Self::quarantine_invalid(path)?;
            return Err(format!(
                "SSH known-host schema v{version} 不受支持（expected v{KNOWN_HOSTS_VERSION}）；旧文件已隔离至 {:?}，不会自动降级为新的 TOFU 信任库",
                quarantine
            ));
        }

        match serde_json::from_str::<KnownHostsFile>(&raw) {
            Ok(file) => Ok(file.hosts),
            Err(error) => {
                let quarantine = Self::quarantine_invalid(path)?;
                Err(format!(
                    "SSH known-host schema v{KNOWN_HOSTS_VERSION} 文件损坏: {error}；原文件已隔离至 {:?}，必须显式重置信任后才能继续",
                    quarantine
                ))
            }
        }
    }

    fn quarantine_invalid(path: &Path) -> Result<PathBuf, String> {
        let mut index = 0u32;
        loop {
            let extension = if index == 0 {
                "json.invalid.bak".to_string()
            } else {
                format!("json.invalid.{index}.bak")
            };
            let quarantine = path.with_extension(extension);
            if quarantine.exists() {
                index = index
                    .checked_add(1)
                    .ok_or_else(|| "生成 SSH known-host 隔离文件名失败".to_string())?;
                continue;
            }
            std::fs::rename(path, &quarantine)
                .map_err(|e| format!("隔离无效 SSH known-host 文件失败: {e}"))?;
            return Ok(quarantine);
        }
    }

    fn normalized_host(host: &str) -> String {
        let trimmed = host.trim();
        trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(trimmed)
            .to_ascii_lowercase()
    }

    fn key(host: &str, port: u16) -> String {
        format!("{}|{port}", Self::normalized_host(host))
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn evaluate(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> HostTrustDecision {
        if !self.available.load(Ordering::Acquire) {
            return HostTrustDecision::Unavailable {
                reason: "SSH known-host 存储不可用或配置未完成".to_string(),
            };
        }

        let configured = match self.path.read() {
            Ok(path) => path.is_some(),
            Err(error) => {
                return HostTrustDecision::Unavailable {
                    reason: format!("SSH known-host 路径锁错误: {error}"),
                };
            }
        };
        if !configured {
            return HostTrustDecision::Unavailable {
                reason: "SSH known-host 存储尚未初始化".to_string(),
            };
        }

        let key = Self::key(host, port);
        let hosts = match self.hosts.read() {
            Ok(hosts) => hosts,
            Err(error) => {
                return HostTrustDecision::Unavailable {
                    reason: format!("SSH known-host 锁错误: {error}"),
                };
            }
        };

        let Some(record) = hosts.get(&key) else {
            return HostTrustDecision::FirstSeen;
        };

        if record
            .keys
            .iter()
            .any(|known| known.algorithm == algorithm && known.fingerprint == fingerprint)
        {
            return HostTrustDecision::Trusted;
        }

        let expected_fingerprints: Vec<String> = record
            .keys
            .iter()
            .filter(|known| known.algorithm == algorithm)
            .map(|known| known.fingerprint.clone())
            .collect();

        if expected_fingerprints.is_empty() {
            let mut known_algorithms: Vec<String> = record
                .keys
                .iter()
                .map(|known| known.algorithm.clone())
                .collect();
            known_algorithms.sort();
            known_algorithms.dedup();
            HostTrustDecision::AdditionalKey { known_algorithms }
        } else {
            HostTrustDecision::Changed {
                algorithm: algorithm.to_string(),
                expected_fingerprints,
            }
        }
    }

    pub fn touch(&self, host: &str, port: u16, algorithm: &str, fingerprint: &str) {
        if !self.available.load(Ordering::Acquire) {
            log::warn!("SSH known-host 存储不可用，拒绝更新 last_seen");
            return;
        }
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
        let Some(known) = record
            .keys
            .iter_mut()
            .find(|known| known.algorithm == algorithm && known.fingerprint == fingerprint)
        else {
            return;
        };

        known.last_seen_ms = Self::now_ms();
        match self.persist_snapshot(&next) {
            Ok(()) => {
                if let Ok(mut hosts) = self.hosts.write() {
                    *hosts = next;
                }
            }
            Err(error) => log::warn!("更新 SSH known-host last_seen 失败: {error}"),
        }
    }

    pub fn trust(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String> {
        self.mutate_record(host, port, |record, now| {
            if let Some(known) = record
                .keys
                .iter_mut()
                .find(|known| known.algorithm == algorithm && known.fingerprint == fingerprint)
            {
                known.last_seen_ms = now;
            } else {
                record.keys.push(KnownHostKeyRecord {
                    algorithm: algorithm.to_string(),
                    fingerprint: fingerprint.to_string(),
                    first_seen_ms: now,
                    last_seen_ms: now,
                });
            }
        })
    }

    pub fn replace(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String> {
        self.mutate_record(host, port, |record, now| {
            record.keys.retain(|known| known.algorithm != algorithm);
            record.keys.push(KnownHostKeyRecord {
                algorithm: algorithm.to_string(),
                fingerprint: fingerprint.to_string(),
                first_seen_ms: now,
                last_seen_ms: now,
            });
        })
    }

    pub fn forget(&self, host: &str, port: u16) -> Result<bool, String> {
        if !self.available.load(Ordering::Acquire) {
            return Err("SSH known-host 存储不可用".to_string());
        }

        let _mutation = self
            .mutation_lock
            .lock()
            .map_err(|_| "SSH known-host mutation 锁错误".to_string())?;
        let key = Self::key(host, port);
        let mut next = self
            .hosts
            .read()
            .map_err(|_| "SSH known-host 锁错误".to_string())?
            .clone();
        let removed = next.remove(&key).is_some();
        if removed {
            self.persist_snapshot(&next)?;
            *self
                .hosts
                .write()
                .map_err(|_| "SSH known-host 锁错误".to_string())? = next;
        }
        Ok(removed)
    }

    pub fn list(&self) -> Result<Vec<KnownHostRecord>, String> {
        if !self.available.load(Ordering::Acquire) {
            return Err("SSH known-host 存储不可用".to_string());
        }
        let mut records: Vec<KnownHostRecord> = self
            .hosts
            .read()
            .map_err(|_| "SSH known-host 锁错误".to_string())?
            .values()
            .cloned()
            .collect();
        records.sort_by(|left, right| {
            left.host
                .cmp(&right.host)
                .then_with(|| left.port.cmp(&right.port))
        });
        Ok(records)
    }

    fn mutate_record(
        &self,
        host: &str,
        port: u16,
        mutator: impl FnOnce(&mut KnownHostRecord, u64),
    ) -> Result<(), String> {
        if !self.available.load(Ordering::Acquire) {
            return Err("SSH known-host 存储不可用，不能修改主机信任".to_string());
        }

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
        let record = next.entry(key).or_insert_with(|| KnownHostRecord {
            host: Self::normalized_host(host),
            port,
            keys: Vec::new(),
        });

        mutator(record, now);
        record.keys.sort_by(|left, right| {
            left.algorithm
                .cmp(&right.algorithm)
                .then_with(|| left.fingerprint.cmp(&right.fingerprint))
        });

        self.persist_snapshot(&next)?;
        *self
            .hosts
            .write()
            .map_err(|_| "SSH known-host 锁错误".to_string())? = next;
        Ok(())
    }

    fn persist_snapshot(&self, hosts: &BTreeMap<String, KnownHostRecord>) -> Result<(), String> {
        if !self.available.load(Ordering::Acquire) {
            return Err("SSH known-host 存储不可用".to_string());
        }
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

        assert!(store
            .trust("example.test", 22, "ssh-ed25519", "SHA256:first")
            .is_err());
        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::FirstSeen
        );

        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn failed_reconfigure_disables_previous_trust() {
        let dir = temp_path();
        std::fs::create_dir_all(&dir).unwrap();
        let good_path = dir.join("known_hosts.json");
        let bad_path = dir.join("broken.json");

        let store = KnownHostStore::new();
        store.configure(good_path).unwrap();
        store
            .trust("example.test", 22, "ssh-ed25519", "SHA256:first")
            .unwrap();
        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::Trusted
        );

        std::fs::write(&bad_path, b"{not-json").unwrap();
        assert!(store.configure(bad_path.clone()).is_err());
        assert!(!bad_path.exists());
        assert!(bad_path.with_extension("json.invalid.bak").exists());
        assert!(matches!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::Unavailable { .. }
        ));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unsupported_version_is_quarantined_and_fails_closed() {
        let dir = temp_path();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts.json");
        std::fs::write(
            &path,
            br#"{
  "version": 2,
  "hosts": {}
}"#,
        )
        .unwrap();

        let store = KnownHostStore::new();
        assert!(store.configure(path.clone()).is_err());
        assert!(!path.exists());
        assert!(path.with_extension("json.invalid.bak").exists());
        assert!(matches!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:new"),
            HostTrustDecision::Unavailable { .. }
        ));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_current_store_is_quarantined_and_fails_closed() {
        let dir = temp_path();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts.json");
        std::fs::write(&path, br#"{"version":3,"hosts":{"broken":{}}}"#).unwrap();

        let store = KnownHostStore::new();
        assert!(store.configure(path.clone()).is_err());
        assert!(!path.exists());
        assert!(path.with_extension("json.invalid.bak").exists());
        assert!(matches!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::Unavailable { .. }
        ));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn quarantine_preserves_existing_backup() {
        let dir = temp_path();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts.json");
        let first_backup = path.with_extension("json.invalid.bak");
        std::fs::write(&first_backup, b"older-backup").unwrap();
        std::fs::write(&path, b"{not-json").unwrap();

        let store = KnownHostStore::new();
        assert!(store.configure(path.clone()).is_err());

        assert_eq!(std::fs::read(&first_backup).unwrap(), b"older-backup");
        assert!(path.with_extension("json.invalid.1.bak").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn additional_algorithm_is_not_first_seen_and_changed_algorithm_fails_closed() {
        let dir = temp_path();
        let path = dir.join("known_hosts.json");

        let store = KnownHostStore::new();
        store.configure(path.clone()).unwrap();
        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:ed-first"),
            HostTrustDecision::FirstSeen
        );

        store
            .trust("example.test", 22, "ssh-ed25519", "SHA256:ed-first")
            .unwrap();

        assert_eq!(
            store.evaluate(
                "example.test",
                22,
                "ecdsa-sha2-nistp256",
                "SHA256:ecdsa-first"
            ),
            HostTrustDecision::AdditionalKey {
                known_algorithms: vec!["ssh-ed25519".to_string()],
            }
        );

        store
            .trust(
                "example.test",
                22,
                "ecdsa-sha2-nistp256",
                "SHA256:ecdsa-first",
            )
            .unwrap();

        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:ed-changed"),
            HostTrustDecision::Changed {
                algorithm: "ssh-ed25519".to_string(),
                expected_fingerprints: vec!["SHA256:ed-first".to_string()],
            }
        );

        store
            .replace("example.test", 22, "ssh-ed25519", "SHA256:ed-changed")
            .unwrap();
        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:ed-changed"),
            HostTrustDecision::Trusted
        );

        let reopened = KnownHostStore::new();
        reopened.configure(path).unwrap();
        assert_eq!(
            reopened.evaluate("example.test", 22, "ssh-ed25519", "SHA256:ed-changed"),
            HostTrustDecision::Trusted
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn forget_removes_endpoint_trust() {
        let dir = temp_path();
        let path = dir.join("known_hosts.json");
        let store = KnownHostStore::new();
        store.configure(path).unwrap();
        store
            .trust("example.test", 22, "ssh-ed25519", "SHA256:first")
            .unwrap();
        assert!(store.forget("example.test", 22).unwrap());
        assert_eq!(
            store.evaluate("example.test", 22, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::FirstSeen
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ipv6_endpoint_key_is_normalized() {
        let store = KnownHostStore::new();
        let dir = temp_path();
        let path = dir.join("known_hosts.json");
        store.configure(path).unwrap();
        store
            .trust("[2001:db8::1]", 2222, "ssh-ed25519", "SHA256:first")
            .unwrap();
        assert_eq!(
            store.evaluate("2001:db8::1", 2222, "ssh-ed25519", "SHA256:first"),
            HostTrustDecision::Trusted
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
