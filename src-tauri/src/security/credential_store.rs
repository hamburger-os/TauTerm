//! Persistent credential storage: native OS keyring first, authenticated encrypted vault fallback.
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "com.tauterm.desktop.credentials";
const INDEX: &str = "__tauterm_index_v1";
const VAULT: &str = "credentials.vault.json";
const VERSION_NO: u32 = 1;
const M_COST: u32 = 64 * 1024;
const T_COST: u32 = 3;
const P_COST: u32 = 1;
const SALT: usize = 16;
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CredentialType {
    Password,
    SshKey,
    Certificate,
    Token,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub account: String,
    pub credential_type: CredentialType,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CredentialValue {
    Password(String),
    SshKey {
        private_key: String,
        passphrase: Option<String>,
    },
    Certificate {
        cert_data: Vec<u8>,
        key_data: Vec<u8>,
    },
    Token(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    entry: CredentialEntry,
    value: CredentialValue,
}
#[derive(Debug, Clone, Serialize)]
pub struct CredentialStorageStatus {
    pub backend: &'static str,
    pub native_available: bool,
    pub fallback_configured: bool,
    pub fallback_unlocked: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct Kdf {
    algorithm: String,
    salt_b64: String,
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
}
#[derive(Debug, Serialize, Deserialize)]
struct Cipher {
    algorithm: String,
    nonce_b64: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: u32,
    kdf: Kdf,
    cipher: Cipher,
    ciphertext_b64: String,
}
#[derive(Default)]
struct Runtime {
    key: Option<Zeroizing<Vec<u8>>>,
}

pub struct CredentialStore {
    runtime: Mutex<Runtime>,
    vault_path: PathBuf,
}
impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}
impl CredentialStore {
    pub fn new() -> Self {
        let d = ProjectDirs::from("com", "TauTerm", "TauTerm")
            .expect("supported desktop platform must provide an application data directory")
            .data_local_dir()
            .to_path_buf();
        Self::with_data_dir(d)
    }
    pub fn with_data_dir(d: PathBuf) -> Self {
        Self {
            runtime: Mutex::new(Runtime::default()),
            vault_path: d.join(VAULT),
        }
    }
    pub fn status(&self) -> CredentialStorageStatus {
        let n = Self::native_available();
        let u = self
            .runtime
            .lock()
            .map(|x| x.key.is_some())
            .unwrap_or(false);
        CredentialStorageStatus {
            backend: if n {
                "native_keyring"
            } else {
                "encrypted_vault"
            },
            native_available: n,
            fallback_configured: self.vault_path.exists(),
            fallback_unlocked: u,
        }
    }
    pub fn unlock_fallback(&self, password: &str) -> Result<(), CredentialStoreError> {
        if Self::native_available() {
            return Err(CredentialStoreError::Backend(
                "系统原生安全凭据存储可用，无需 fallback vault".into(),
            ));
        }
        if password.chars().count() < 10 {
            return Err(CredentialStoreError::Backend("主密码至少 10 个字符".into()));
        }
        if let Some(p) = self.vault_path.parent() {
            fs::create_dir_all(p).map_err(be)?
        }
        let retained = if self.vault_path.exists() {
            let e = self.read_env()?;
            let salt = dec_fixed::<SALT>(&e.kdf.salt_b64, "salt")?;
            let mut k = derive(password, &salt, &e.kdf)?;
            decrypt(&e, &k).map_err(|_| CredentialStoreError::InvalidMasterPassword)?;
            let z = Zeroizing::new(k.to_vec());
            k.zeroize();
            z
        } else {
            let mut salt = [0u8; SALT];
            getrandom::fill(&mut salt).map_err(|e| CredentialStoreError::Backend(e.to_string()))?;
            let kdf = Kdf {
                algorithm: "argon2id".into(),
                salt_b64: B64.encode(salt),
                memory_kib: M_COST,
                iterations: T_COST,
                lanes: P_COST,
            };
            let mut k = derive(password, &salt, &kdf)?;
            self.write_map(&BTreeMap::new(), &k, kdf)?;
            let z = Zeroizing::new(k.to_vec());
            k.zeroize();
            z
        };
        self.runtime
            .lock()
            .map_err(|_| CredentialStoreError::LockError)?
            .key = Some(retained);
        Ok(())
    }
    pub fn lock_fallback(&self) {
        if let Ok(mut s) = self.runtime.lock() {
            s.key = None
        }
    }
    pub fn store_credential(
        &self,
        account: &str,
        credential_type: CredentialType,
        value: CredentialValue,
        description: &str,
    ) -> Result<(), CredentialStoreError> {
        valid_account(account)?;
        let stored = Stored {
            entry: CredentialEntry {
                account: account.into(),
                credential_type,
                description: description.into(),
            },
            value,
        };
        if Self::native_available() {
            self.native_store(account, &stored)
        } else {
            self.with_map_mut(|m| {
                m.insert(account.into(), stored);
                Ok(())
            })
        }
    }
    pub fn get_credential(&self, account: &str) -> Result<CredentialValue, CredentialStoreError> {
        valid_account(account)?;
        if Self::native_available() {
            self.native_get(account)?
                .map(|s| s.value)
                .ok_or_else(|| CredentialStoreError::NotFound(account.into()))
        } else {
            self.with_map(|m| {
                m.get(account)
                    .map(|s| s.value.clone())
                    .ok_or_else(|| CredentialStoreError::NotFound(account.into()))
            })
        }
    }
    pub fn list_credentials(&self) -> Result<Vec<CredentialEntry>, CredentialStoreError> {
        if Self::native_available() {
            let mut out = Vec::new();
            for a in self.read_index()? {
                if let Some(s) = self.native_get(&a)? {
                    out.push(s.entry)
                }
            }
            Ok(out)
        } else {
            self.with_map(|m| Ok(m.values().map(|s| s.entry.clone()).collect()))
        }
    }
    pub fn delete_credential(&self, account: &str) -> Result<(), CredentialStoreError> {
        valid_account(account)?;
        if Self::native_available() {
            let e = Self::entry(account)?;
            match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(be(e)),
            };
            let mut i = self.read_index()?;
            if i.remove(account) {
                self.write_index(&i)?
            }
            Ok(())
        } else {
            self.with_map_mut(|m| {
                m.remove(account);
                Ok(())
            })
        }
    }
    fn native_available() -> bool {
        keyring::Entry::store_status().is_ok()
    }
    fn entry(a: &str) -> Result<keyring::Entry, CredentialStoreError> {
        keyring::Entry::new(SERVICE, a).map_err(be)
    }
    fn native_store(&self, a: &str, s: &Stored) -> Result<(), CredentialStoreError> {
        let mut b = serde_json::to_vec(s).map_err(be)?;
        let e = Self::entry(a)?;
        e.set_secret(&b).map_err(be)?;
        b.zeroize();
        let mut i = match self.read_index() {
            Ok(index) => index,
            Err(error) => {
                let _ = e.delete_credential();
                return Err(error);
            }
        };
        if i.insert(a.into()) {
            if let Err(x) = self.write_index(&i) {
                let _ = e.delete_credential();
                return Err(x);
            }
        }
        Ok(())
    }
    fn native_get(&self, a: &str) -> Result<Option<Stored>, CredentialStoreError> {
        match Self::entry(a)?.get_secret() {
            Ok(mut b) => {
                let x = serde_json::from_slice(&b).map_err(be);
                b.zeroize();
                x.map(Some)
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(be(e)),
        }
    }
    fn read_index(&self) -> Result<BTreeSet<String>, CredentialStoreError> {
        match Self::entry(INDEX)?.get_secret() {
            Ok(mut b) => {
                let x = serde_json::from_slice(&b).map_err(be);
                b.zeroize();
                x
            }
            Err(keyring::Error::NoEntry) => Ok(BTreeSet::new()),
            Err(e) => Err(be(e)),
        }
    }
    fn write_index(&self, i: &BTreeSet<String>) -> Result<(), CredentialStoreError> {
        let mut b = serde_json::to_vec(i).map_err(be)?;
        let x = Self::entry(INDEX)?.set_secret(&b).map_err(be);
        b.zeroize();
        x
    }
    fn key(&self) -> Result<Zeroizing<Vec<u8>>, CredentialStoreError> {
        self.runtime
            .lock()
            .map_err(|_| CredentialStoreError::LockError)?
            .key
            .as_ref()
            .map(|x| Zeroizing::new(x.to_vec()))
            .ok_or(CredentialStoreError::VaultLocked)
    }
    fn with_map<T>(
        &self,
        f: impl FnOnce(&BTreeMap<String, Stored>) -> Result<T, CredentialStoreError>,
    ) -> Result<T, CredentialStoreError> {
        let k = self.key()?;
        let e = self.read_env()?;
        let m = decrypt(&e, &k)?;
        f(&m)
    }
    fn with_map_mut<T>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, Stored>) -> Result<T, CredentialStoreError>,
    ) -> Result<T, CredentialStoreError> {
        let k = self.key()?;
        let e = self.read_env()?;
        let mut m = decrypt(&e, &k)?;
        let x = f(&mut m)?;
        self.write_map(&m, &k, e.kdf)?;
        Ok(x)
    }
    fn read_env(&self) -> Result<Envelope, CredentialStoreError> {
        let b = fs::read(&self.vault_path).map_err(be)?;
        let e: Envelope = serde_json::from_slice(&b).map_err(be)?;
        validate_env(&e)?;
        Ok(e)
    }
    fn write_map(
        &self,
        m: &BTreeMap<String, Stored>,
        key: &[u8],
        kdf: Kdf,
    ) -> Result<(), CredentialStoreError> {
        let mut p = serde_json::to_vec(m).map_err(be)?;
        let mut n = [0u8; NONCE_LEN];
        getrandom::fill(&mut n).map_err(|e| CredentialStoreError::Backend(e.to_string()))?;
        let c = Aes256Gcm::new_from_slice(key)
            .map_err(|_| CredentialStoreError::Backend("invalid vault key".into()))?;
        let aad = aad(&kdf);
        let ct = c
            .encrypt(
                Nonce::from_slice(&n),
                Payload {
                    msg: &p,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| CredentialStoreError::Backend("vault encryption failed".into()))?;
        p.zeroize();
        let e = Envelope {
            version: VERSION_NO,
            kdf,
            cipher: Cipher {
                algorithm: "aes-256-gcm".into(),
                nonce_b64: B64.encode(n),
            },
            ciphertext_b64: B64.encode(ct),
        };
        private_atomic_write(
            &self.vault_path,
            &serde_json::to_vec_pretty(&e).map_err(be)?,
        )
    }
}
fn be(e: impl std::fmt::Display) -> CredentialStoreError {
    CredentialStoreError::Backend(e.to_string())
}
fn valid_account(a: &str) -> Result<(), CredentialStoreError> {
    if a.trim().is_empty() || a.len() > 512 || a.contains('\0') || a == INDEX {
        Err(CredentialStoreError::Backend(
            "invalid credential account".into(),
        ))
    } else {
        Ok(())
    }
}
fn validate_env(e: &Envelope) -> Result<(), CredentialStoreError> {
    if e.version != VERSION_NO
        || e.kdf.algorithm != "argon2id"
        || e.cipher.algorithm != "aes-256-gcm"
    {
        return Err(be("unsupported vault format"));
    }
    validate_kdf(&e.kdf)
}
fn validate_kdf(k: &Kdf) -> Result<(), CredentialStoreError> {
    if !(32 * 1024..=1024 * 1024).contains(&k.memory_kib)
        || !(1..=10).contains(&k.iterations)
        || !(1..=16).contains(&k.lanes)
    {
        Err(be("unsafe vault KDF parameters"))
    } else {
        Ok(())
    }
}
fn derive(password: &str, salt: &[u8; SALT], k: &Kdf) -> Result<[u8; 32], CredentialStoreError> {
    validate_kdf(k)?;
    let p = Params::new(k.memory_kib, k.iterations, k.lanes, Some(32)).map_err(be)?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(be)?;
    Ok(out)
}
fn aad(k: &Kdf) -> String {
    format!(
        "TauTerm:credential-vault:v{}:argon2id:m={}:t={}:p={}",
        VERSION_NO, k.memory_kib, k.iterations, k.lanes
    )
}
fn decrypt(e: &Envelope, key: &[u8]) -> Result<BTreeMap<String, Stored>, CredentialStoreError> {
    validate_env(e)?;
    let n = dec_fixed::<NONCE_LEN>(&e.cipher.nonce_b64, "nonce")?;
    let ct = B64.decode(&e.ciphertext_b64).map_err(be)?;
    let c = Aes256Gcm::new_from_slice(key).map_err(|_| be("invalid vault key"))?;
    let a = aad(&e.kdf);
    let mut p = c
        .decrypt(
            Nonce::from_slice(&n),
            Payload {
                msg: &ct,
                aad: a.as_bytes(),
            },
        )
        .map_err(|_| be("vault authentication failed"))?;
    let x = serde_json::from_slice(&p).map_err(be);
    p.zeroize();
    x
}
fn dec_fixed<const N: usize>(s: &str, name: &str) -> Result<[u8; N], CredentialStoreError> {
    B64.decode(s)
        .map_err(be)?
        .try_into()
        .map_err(|_| be(format!("invalid {name} length")))
}
fn private_atomic_write(path: &Path, b: &[u8]) -> Result<(), CredentialStoreError> {
    let p = path.parent().ok_or_else(|| be("vault has no parent"))?;
    fs::create_dir_all(p).map_err(be)?;
    let t = p.join(format!(".{VAULT}.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&t, b).map_err(be)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&t, fs::Permissions::from_mode(0o600)).map_err(be)?;
    }
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(path).map_err(be)?;
    }
    fs::rename(t, path).map_err(be)
}
#[derive(Debug, thiserror::Error)]
pub enum CredentialStoreError {
    #[error("凭据 '{0}' 不存在")]
    NotFound(String),
    #[error("类型不匹配")]
    TypeMismatch,
    #[error("内部锁错误")]
    LockError,
    #[error("加密凭据 vault 尚未解锁")]
    VaultLocked,
    #[error("主密码错误或 vault 被篡改")]
    InvalidMasterPassword,
    #[error("凭据存储错误: {0}")]
    Backend(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aead_detects_tamper() {
        let d = tempfile::tempdir().unwrap();
        let s = CredentialStore::with_data_dir(d.path().into());
        let salt = [7u8; SALT];
        let kdf = Kdf {
            algorithm: "argon2id".into(),
            salt_b64: B64.encode(salt),
            memory_kib: 32 * 1024,
            iterations: 1,
            lanes: 1,
        };
        let key = derive("correct horse battery staple", &salt, &kdf).unwrap();
        let mut m = BTreeMap::new();
        m.insert(
            "x".into(),
            Stored {
                entry: CredentialEntry {
                    account: "x".into(),
                    credential_type: CredentialType::Password,
                    description: "".into(),
                },
                value: CredentialValue::Password("secret".into()),
            },
        );
        s.write_map(&m, &key, kdf).unwrap();
        let mut e = s.read_env().unwrap();
        assert!(decrypt(&e, &key).is_ok());
        let mut ct = B64.decode(&e.ciphertext_b64).unwrap();
        ct[0] ^= 1;
        e.ciphertext_b64 = B64.encode(ct);
        assert!(decrypt(&e, &key).is_err());
    }
}
