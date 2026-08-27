from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


# ---------------------------------------------------------------------------
# 1. VirtualPortBackend: migrate the domain model from PortPair to endpoint.
# ---------------------------------------------------------------------------
for path in list((ROOT / "src-tauri/src").rglob("*.rs")) + list((ROOT / "src").rglob("*.ts")) + list((ROOT / "src").rglob("*.tsx")):
    text = path.read_text(encoding="utf-8")
    replacements = [
        ("PortPair", "VirtualEndpoint"),
        ("create_pairs_elevated", "create_endpoints_elevated"),
        ("create_pairs", "create_endpoints"),
        ("destroy_pair", "destroy_endpoint"),
        ("cleanup_pairs_elevated", "cleanup_endpoints_elevated"),
        ("active_pairs", "active_endpoints"),
        ("virtual_port_pairs", "virtual_endpoints"),
        ("virtualPortPairs", "virtualEndpoints"),
        ("vport_pairs_json", "vport_endpoints_json"),
        ("port_a", "bridge_path"),
        ("port_b", "external_path"),
        ("bus_number", "resource_id"),
    ]
    for old, new in replacements:
        text = text.replace(old, new)
    path.write_text(text, encoding="utf-8")

backend = read("src-tauri/src/virtual_port/backend.rs")
backend = backend.replace("虚拟串口端口对", "虚拟串口端点")
backend = backend.replace("一对已创建且保持连接的虚拟 COM 端口", "一个对外暴露的虚拟串口端点；bridge_path 属于后端内部桥接定位")
backend = backend.replace("用于创建虚拟端口对的配置", "用于创建虚拟端点的配置")
backend = backend.replace("虚拟串口端口对的生命周期", "虚拟串口端点的生命周期")
backend = backend.replace("创建 `count` 个虚拟串口端口对", "创建 `count` 个虚拟串口端点")
backend = backend.replace("创建端口对", "创建虚拟端点")
backend = backend.replace("销毁一个虚拟端口对", "销毁一个虚拟端点")
backend = backend.replace("清理所有活跃端口对", "清理所有活跃虚拟端点")
backend = backend.replace("遗留的端口对", "遗留的虚拟端点")
write("src-tauri/src/virtual_port/backend.rs", backend)

# The native Unix implementation is a PTY backend, not a socat backend.
socat = ROOT / "src-tauri/src/virtual_port/socat.rs"
pty = ROOT / "src-tauri/src/virtual_port/pty.rs"
if socat.exists():
    socat.rename(pty)
for path in list((ROOT / "src-tauri/src").rglob("*.rs")):
    text = path.read_text(encoding="utf-8")
    text = text.replace("virtual_port::socat::PtyBackend", "virtual_port::pty::PtyBackend")
    text = text.replace("pub mod socat;", "pub mod pty;")
    text = text.replace("super::socat::", "super::pty::")
    text = text.replace("socat::", "pty::")
    text = text.replace("SocatBackend", "PtyBackend")
    text = text.replace("socat 已就绪，虚拟串口功能可用", "原生 PTY 后端已就绪，虚拟串口功能可用")
    text = text.replace("清理上次异常退出可能遗留的孤儿 symlink", "PTY 句柄随进程退出由内核回收，无持久化孤儿资源")
    text = text.replace("个孤儿虚拟端口对 (socat)", "个孤儿虚拟端点 (PTY)")
    text = text.replace("socat 未安装，虚拟串口功能不可用。安装: apt install socat (Linux) / brew install socat (macOS)", "原生 PTY 后端不可用")
    text = text.replace("socat not installed. Install via: sudo apt install socat (Linux) or brew install socat (macOS)", "Native PTY backend unavailable")
    path.write_text(text, encoding="utf-8")

# Only expose endpoint data to the frontend; bridge_path is internal implementation detail.
commands = read("src-tauri/src/commands.rs")
commands = commands.replace('"bridge_path": p.bridge_path,\n                    "external_path": p.external_path,\n                    "resource_id": p.resource_id,', '"external_path": p.external_path,\n                    "resource_id": p.resource_id,')
commands = commands.replace('"pairs": &vport_endpoints_json,', '"endpoints": &vport_endpoints_json,')
commands = commands.replace('"virtual_endpoints": vport_endpoints_json,', '"virtual_endpoints": vport_endpoints_json,')
write("src-tauri/src/commands.rs", commands)

# ---------------------------------------------------------------------------
# 2. Credentials: native platform keyring first; password-unlocked AEAD vault
#    only when the native secure store is unavailable.
# ---------------------------------------------------------------------------
cargo = read("src-tauri/Cargo.toml")
needle = 'crc32fast = "1.5.0"\n'
extra = '''crc32fast = "1.5.0"\nkeyring = "4.1.6"\naes-gcm = "0.11"\nargon2 = "0.5"\nbase64 = "0.22"\ngetrandom = "0.3"\nzeroize = "1.8"\ndirectories = "6"\n'''
if "keyring =" not in cargo:
    cargo = replace_once(cargo, needle, extra, "Cargo dependencies")
if "[dev-dependencies]" not in cargo:
    cargo += '\n[dev-dependencies]\ntempfile = "3"\n'
elif "tempfile =" not in cargo:
    cargo = cargo.replace("[dev-dependencies]", '[dev-dependencies]\ntempfile = "3"', 1)
write("src-tauri/Cargo.toml", cargo)

credential_store = r'''//! Release-grade credential persistence.
//!
//! Native secure storage is the primary backend:
//! - Windows: Credential Manager
//! - macOS: Keychain Services
//! - Linux/*nix: Secret Service
//!
//! If the native store is unavailable, TauTerm never derives a key from machine
//! identifiers. The fallback vault must be explicitly unlocked with a user master
//! password. Argon2id derives a 256-bit key and AES-256-GCM authenticates the entire
//! encrypted credential map. A fresh nonce is generated for every write.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const KEYRING_SERVICE: &str = "com.tauterm.desktop.credentials";
const KEYRING_INDEX_ACCOUNT: &str = "__tauterm_index_v1";
const VAULT_FILE: &str = "credentials.vault.json";
const VAULT_VERSION: u32 = 1;
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_ITERATIONS: u32 = 3;
const KDF_LANES: u32 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub secret: String,
    pub credential_type: CredentialType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    Password,
    SshKey,
    Token,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStorageStatus {
    pub backend: &'static str,
    pub native_available: bool,
    pub fallback_configured: bool,
    pub fallback_unlocked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultKdf {
    algorithm: String,
    salt_b64: String,
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultCipher {
    algorithm: String,
    nonce_b64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultEnvelope {
    version: u32,
    kdf: VaultKdf,
    cipher: VaultCipher,
    ciphertext_b64: String,
}

#[derive(Default)]
struct RuntimeState {
    fallback_key: Option<Zeroizing<Vec<u8>>>,
}

pub struct CredentialStore {
    state: Mutex<RuntimeState>,
    vault_path: PathBuf,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore {
    pub fn new() -> Self {
        let data_dir = ProjectDirs::from("com", "TauTerm", "TauTerm")
            .map(|dirs| dirs.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_data_dir(data_dir)
    }

    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            state: Mutex::new(RuntimeState::default()),
            vault_path: data_dir.join(VAULT_FILE),
        }
    }

    pub fn status(&self) -> CredentialStorageStatus {
        let native_available = Self::native_available();
        let fallback_unlocked = self
            .state
            .lock()
            .map(|s| s.fallback_key.is_some())
            .unwrap_or(false);
        CredentialStorageStatus {
            backend: if native_available { "native_keyring" } else { "encrypted_vault" },
            native_available,
            fallback_configured: self.vault_path.exists(),
            fallback_unlocked,
        }
    }

    pub fn unlock_fallback(&self, master_password: &str) -> Result<(), String> {
        if Self::native_available() {
            return Err("系统原生安全凭据存储可用，无需启用 fallback vault".into());
        }
        if master_password.len() < 10 {
            return Err("主密码至少需要 10 个字符".into());
        }

        if let Some(parent) = self.vault_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建凭据目录失败: {e}"))?;
        }

        let key = if self.vault_path.exists() {
            let envelope = self.read_envelope()?;
            let salt = decode_fixed::<SALT_LEN>(&envelope.kdf.salt_b64, "salt")?;
            let mut key = derive_key(master_password, &salt, &envelope.kdf)?;
            // Authentication happens before the key is retained in memory.
            let _ = decrypt_map(&envelope, &key)
                .map_err(|_| "主密码错误或凭据 vault 已被篡改/损坏".to_string())?;
            let retained = Zeroizing::new(key.to_vec());
            key.zeroize();
            retained
        } else {
            let mut salt = [0u8; SALT_LEN];
            getrandom::fill(&mut salt).map_err(|e| format!("生成 vault salt 失败: {e}"))?;
            let kdf = VaultKdf {
                algorithm: "argon2id".into(),
                salt_b64: B64.encode(salt),
                memory_kib: KDF_MEMORY_KIB,
                iterations: KDF_ITERATIONS,
                lanes: KDF_LANES,
            };
            let mut key = derive_key(master_password, &salt, &kdf)?;
            self.write_encrypted_map(&BTreeMap::new(), &key, kdf)?;
            let retained = Zeroizing::new(key.to_vec());
            key.zeroize();
            retained
        };

        let mut state = self.state.lock().map_err(|_| "凭据状态锁损坏".to_string())?;
        state.fallback_key = Some(key);
        Ok(())
    }

    pub fn lock_fallback(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.fallback_key = None;
        }
    }

    pub fn store(&self, key: String, credential: Credential) -> Result<(), String> {
        validate_key(&key)?;
        if Self::native_available() {
            self.native_store(&key, &credential)
        } else {
            self.with_fallback_map_mut(|map| {
                map.insert(key, credential);
                Ok(())
            })
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<Credential>, String> {
        validate_key(key)?;
        if Self::native_available() {
            self.native_get(key)
        } else {
            self.with_fallback_map(|map| Ok(map.get(key).cloned()))
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        validate_key(key)?;
        if Self::native_available() {
            self.native_delete(key)
        } else {
            self.with_fallback_map_mut(|map| {
                map.remove(key);
                Ok(())
            })
        }
    }

    pub fn list(&self) -> Result<Vec<(String, Credential)>, String> {
        if Self::native_available() {
            let keys = self.native_read_index()?;
            let mut out = Vec::with_capacity(keys.len());
            for key in keys {
                if let Some(credential) = self.native_get(&key)? {
                    out.push((key, credential));
                }
            }
            Ok(out)
        } else {
            self.with_fallback_map(|map| {
                Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            })
        }
    }

    fn native_available() -> bool {
        keyring::Entry::store_status().is_ok()
    }

    fn native_entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .map_err(|e| format!("创建系统凭据项失败: {e}"))
    }

    fn native_store(&self, key: &str, credential: &Credential) -> Result<(), String> {
        let bytes = serde_json::to_vec(credential).map_err(|e| e.to_string())?;
        let entry = Self::native_entry(key)?;
        entry.set_secret(&bytes).map_err(|e| format!("写入系统凭据存储失败: {e}"))?;

        let mut index = self.native_read_index()?;
        if index.insert(key.to_string()) {
            if let Err(error) = self.native_write_index(&index) {
                let _ = entry.delete_credential();
                return Err(error);
            }
        }
        Ok(())
    }

    fn native_get(&self, key: &str) -> Result<Option<Credential>, String> {
        let entry = Self::native_entry(key)?;
        match entry.get_secret() {
            Ok(mut bytes) => {
                let parsed = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("系统凭据内容损坏: {e}"));
                bytes.zeroize();
                parsed.map(Some)
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("读取系统凭据存储失败: {e}")),
        }
    }

    fn native_delete(&self, key: &str) -> Result<(), String> {
        let entry = Self::native_entry(key)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("删除系统凭据失败: {e}")),
        }
        let mut index = self.native_read_index()?;
        if index.remove(key) {
            self.native_write_index(&index)?;
        }
        Ok(())
    }

    fn native_read_index(&self) -> Result<BTreeSet<String>, String> {
        let entry = Self::native_entry(KEYRING_INDEX_ACCOUNT)?;
        match entry.get_secret() {
            Ok(mut bytes) => {
                let parsed = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("系统凭据索引损坏: {e}"));
                bytes.zeroize();
                parsed
            }
            Err(keyring::Error::NoEntry) => Ok(BTreeSet::new()),
            Err(e) => Err(format!("读取系统凭据索引失败: {e}")),
        }
    }

    fn native_write_index(&self, index: &BTreeSet<String>) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(index).map_err(|e| e.to_string())?;
        let result = Self::native_entry(KEYRING_INDEX_ACCOUNT)?
            .set_secret(&bytes)
            .map_err(|e| format!("写入系统凭据索引失败: {e}"));
        bytes.zeroize();
        result
    }

    fn fallback_key(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        let state = self.state.lock().map_err(|_| "凭据状态锁损坏".to_string())?;
        state
            .fallback_key
            .as_ref()
            .map(|key| Zeroizing::new(key.to_vec()))
            .ok_or_else(|| "系统安全凭据存储不可用；请先在设置 → 安全中解锁加密凭据 vault".to_string())
    }

    fn with_fallback_map<T>(&self, f: impl FnOnce(&BTreeMap<String, Credential>) -> Result<T, String>) -> Result<T, String> {
        let key = self.fallback_key()?;
        let envelope = self.read_envelope()?;
        let map = decrypt_map(&envelope, &key)?;
        f(&map)
    }

    fn with_fallback_map_mut<T>(&self, f: impl FnOnce(&mut BTreeMap<String, Credential>) -> Result<T, String>) -> Result<T, String> {
        let key = self.fallback_key()?;
        let envelope = self.read_envelope()?;
        let mut map = decrypt_map(&envelope, &key)?;
        let result = f(&mut map)?;
        self.write_encrypted_map(&map, &key, envelope.kdf)?;
        Ok(result)
    }

    fn read_envelope(&self) -> Result<VaultEnvelope, String> {
        let bytes = fs::read(&self.vault_path).map_err(|e| format!("读取凭据 vault 失败: {e}"))?;
        let envelope: VaultEnvelope = serde_json::from_slice(&bytes)
            .map_err(|e| format!("凭据 vault 格式损坏: {e}"))?;
        validate_envelope(&envelope)?;
        Ok(envelope)
    }

    fn write_encrypted_map(&self, map: &BTreeMap<String, Credential>, key: &[u8], kdf: VaultKdf) -> Result<(), String> {
        let mut plaintext = serde_json::to_vec(map).map_err(|e| e.to_string())?;
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| format!("生成 AES-GCM nonce 失败: {e}"))?;
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "无效的 vault key".to_string())?;
        let aad = vault_aad(&kdf);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: &plaintext, aad: aad.as_bytes() })
            .map_err(|_| "加密凭据 vault 失败".to_string())?;
        plaintext.zeroize();

        let envelope = VaultEnvelope {
            version: VAULT_VERSION,
            kdf,
            cipher: VaultCipher { algorithm: "aes-256-gcm".into(), nonce_b64: B64.encode(nonce) },
            ciphertext_b64: B64.encode(ciphertext),
        };
        let serialized = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;
        atomic_private_write(&self.vault_path, &serialized)
    }
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.trim().is_empty() || key.len() > 512 || key.contains('\0') {
        return Err("无效的凭据 key".into());
    }
    if key == KEYRING_INDEX_ACCOUNT {
        return Err("凭据 key 与保留项冲突".into());
    }
    Ok(())
}

fn validate_envelope(envelope: &VaultEnvelope) -> Result<(), String> {
    if envelope.version != VAULT_VERSION
        || envelope.kdf.algorithm != "argon2id"
        || envelope.cipher.algorithm != "aes-256-gcm"
        || envelope.kdf.memory_kib < 32 * 1024
        || envelope.kdf.iterations == 0
        || envelope.kdf.lanes == 0
    {
        return Err("不支持或不安全的凭据 vault 参数".into());
    }
    Ok(())
}

fn derive_key(password: &str, salt: &[u8; SALT_LEN], kdf: &VaultKdf) -> Result<[u8; 32], String> {
    validate_kdf(kdf)?;
    let params = Params::new(kdf.memory_kib, kdf.iterations, kdf.lanes, Some(32))
        .map_err(|e| format!("Argon2 参数无效: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Argon2id 派生失败: {e}"))?;
    Ok(key)
}

fn validate_kdf(kdf: &VaultKdf) -> Result<(), String> {
    if kdf.algorithm != "argon2id"
        || kdf.memory_kib < 32 * 1024
        || kdf.memory_kib > 1024 * 1024
        || !(1..=10).contains(&kdf.iterations)
        || !(1..=16).contains(&kdf.lanes)
    {
        return Err("拒绝异常的 vault KDF 参数".into());
    }
    Ok(())
}

fn vault_aad(kdf: &VaultKdf) -> String {
    format!(
        "TauTerm:credential-vault:v{}:argon2id:m={}:t={}:p={}",
        VAULT_VERSION, kdf.memory_kib, kdf.iterations, kdf.lanes
    )
}

fn decrypt_map(envelope: &VaultEnvelope, key: &[u8]) -> Result<BTreeMap<String, Credential>, String> {
    validate_envelope(envelope)?;
    let nonce = decode_fixed::<NONCE_LEN>(&envelope.cipher.nonce_b64, "nonce")?;
    let ciphertext = B64.decode(&envelope.ciphertext_b64).map_err(|_| "vault ciphertext Base64 无效".to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "无效的 vault key".to_string())?;
    let aad = vault_aad(&envelope.kdf);
    let mut plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &ciphertext, aad: aad.as_bytes() })
        .map_err(|_| "凭据 vault 认证失败（密码错误、文件损坏或被篡改）".to_string())?;
    let parsed = serde_json::from_slice(&plaintext).map_err(|e| format!("vault 明文格式损坏: {e}"));
    plaintext.zeroize();
    parsed
}

fn decode_fixed<const N: usize>(value: &str, name: &str) -> Result<[u8; N], String> {
    let decoded = B64.decode(value).map_err(|_| format!("vault {name} Base64 无效"))?;
    decoded.try_into().map_err(|_| format!("vault {name} 长度无效"))
}

fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "vault 路径缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("创建 vault 目录失败: {e}"))?;
    let temp = parent.join(format!(".{VAULT_FILE}.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temp, bytes).map_err(|e| format!("写入临时 vault 失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置 vault 权限失败: {e}"))?;
    }
    if path.exists() {
        #[cfg(target_os = "windows")]
        fs::remove_file(path).map_err(|e| format!("替换旧 vault 失败: {e}"))?;
    }
    fs::rename(&temp, path).map_err(|e| format!("原子替换 vault 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_kdf(salt: [u8; SALT_LEN]) -> VaultKdf {
        VaultKdf {
            algorithm: "argon2id".into(),
            salt_b64: B64.encode(salt),
            memory_kib: 32 * 1024,
            iterations: 1,
            lanes: 1,
        }
    }

    #[test]
    fn encrypted_vault_round_trip_and_tamper_detection() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::with_data_dir(dir.path().to_path_buf());
        let salt = [7u8; SALT_LEN];
        let kdf = test_kdf(salt);
        let key = derive_key("correct horse battery staple", &salt, &kdf).unwrap();
        let mut map = BTreeMap::new();
        map.insert("ssh:test".into(), Credential {
            username: "alice".into(),
            secret: "s3cret".into(),
            credential_type: CredentialType::Password,
        });
        store.write_encrypted_map(&map, &key, kdf).unwrap();
        let envelope = store.read_envelope().unwrap();
        let decoded = decrypt_map(&envelope, &key).unwrap();
        assert_eq!(decoded.get("ssh:test").unwrap().secret, "s3cret");

        let mut envelope = envelope;
        let mut ciphertext = B64.decode(&envelope.ciphertext_b64).unwrap();
        ciphertext[0] ^= 0x80;
        envelope.ciphertext_b64 = B64.encode(ciphertext);
        assert!(decrypt_map(&envelope, &key).is_err());
    }

    #[test]
    fn abnormal_kdf_parameters_are_rejected_before_work() {
        let mut kdf = test_kdf([1u8; SALT_LEN]);
        kdf.memory_kib = u32::MAX;
        assert!(derive_key("password password", &[1u8; SALT_LEN], &kdf).is_err());
    }
}
'''
write("src-tauri/src/security/credential_store.rs", credential_store)

# Add vault commands next to the existing credential CRUD commands.
commands = read("src-tauri/src/commands.rs")
marker = "// ── 配置存储命令"
if "credential_storage_status" not in commands:
    addition = r'''
#[tauri::command]
pub fn credential_storage_status(
    state: State<'_, AppState>,
) -> Result<crate::security::CredentialStorageStatus, String> {
    Ok(state.credential_store.status())
}

#[tauri::command]
pub fn unlock_credential_vault(
    state: State<'_, AppState>,
    master_password: String,
) -> Result<(), String> {
    state.credential_store.unlock_fallback(&master_password)
}

#[tauri::command]
pub fn lock_credential_vault(state: State<'_, AppState>) -> Result<(), String> {
    state.credential_store.lock_fallback();
    Ok(())
}

'''
    commands = replace_once(commands, marker, addition + marker, "credential command insertion")
write("src-tauri/src/commands.rs", commands)

lib = read("src-tauri/src/lib.rs")
if "commands::credential_storage_status" not in lib:
    lib = lib.replace("commands::delete_credential,", "commands::delete_credential,\n            commands::credential_storage_status,\n            commands::unlock_credential_vault,\n            commands::lock_credential_vault,", 1)
write("src-tauri/src/lib.rs", lib)

# Export status type.
security_mod = read("src-tauri/src/security/mod.rs")
security_mod = security_mod.replace("pub use credential_store::{Credential, CredentialStore, CredentialType};", "pub use credential_store::{Credential, CredentialStorageStatus, CredentialStore, CredentialType};")
write("src-tauri/src/security/mod.rs", security_mod)

# Settings UI for unlocking/locking the fallback vault. It is shown even when native
# keyring is available so the user can see which backend is active.
panel_path = ROOT / "src/components/Settings/panels/SecuritySettings.tsx"
panel_path.write_text(r'''import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

interface StorageStatus {
  backend: "native_keyring" | "encrypted_vault";
  native_available: boolean;
  fallback_configured: boolean;
  fallback_unlocked: boolean;
}

export default function SecuritySettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<StorageStatus | null>(null);
  const [password, setPassword] = useState("");
  const [message, setMessage] = useState("");

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<StorageStatus>("credential_storage_status"));
    } catch (error) {
      setMessage(String(error));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const unlock = async () => {
    try {
      await invoke("unlock_credential_vault", { masterPassword: password });
      setPassword("");
      setMessage(t("settings.securityVaultUnlocked", { defaultValue: "Encrypted credential vault unlocked for this app session." }));
      await refresh();
    } catch (error) {
      setMessage(String(error));
    }
  };

  const lock = async () => {
    await invoke("lock_credential_vault");
    setPassword("");
    setMessage(t("settings.securityVaultLocked", { defaultValue: "Credential vault locked." }));
    await refresh();
  };

  return <div style={{ display: "grid", gap: 16, maxWidth: 680 }}>
    <h2>{t("settings.security", { defaultValue: "Security" })}</h2>
    <p>
      {status?.native_available
        ? t("settings.securityNative", { defaultValue: "Credentials are stored in the operating system secure store (Credential Manager / Keychain / Secret Service)." })
        : t("settings.securityFallback", { defaultValue: "The operating system secure store is unavailable. Unlock the AES-256-GCM vault with your master password before storing credentials." })}
    </p>
    {status && <p><strong>{t("settings.securityBackend", { defaultValue: "Active backend" })}:</strong> {status.backend}</p>}
    {!status?.native_available && <>
      <input
        className="liquid-glass-input"
        type="password"
        autoComplete="current-password"
        value={password}
        onChange={event => setPassword(event.target.value)}
        placeholder={t("settings.securityMasterPassword", { defaultValue: "Master password (10+ characters)" })}
      />
      <div style={{ display: "flex", gap: 8 }}>
        <button className="liquid-glass-button" disabled={password.length < 10} onClick={() => void unlock()}>
          {t("settings.securityUnlock", { defaultValue: "Unlock vault" })}
        </button>
        <button className="liquid-glass-button" disabled={!status?.fallback_unlocked} onClick={() => void lock()}>
          {t("settings.securityLock", { defaultValue: "Lock vault" })}
        </button>
      </div>
    </>}
    {message && <p role="status">{message}</p>}
  </div>;
}
''', encoding="utf-8")

settings = read("src/components/Settings/SettingsPage.tsx")
if "SecuritySettings" not in settings:
    settings = settings.replace('import ShortcutSettings from "./panels/ShortcutSettings";', 'import ShortcutSettings from "./panels/ShortcutSettings";\nimport SecuritySettings from "./panels/SecuritySettings";')
    settings = settings.replace('type Category = "appearance" | "language" | "logging" | "shortcuts" | "about";', 'type Category = "appearance" | "language" | "logging" | "security" | "shortcuts" | "about";')
    settings = settings.replace('{ id: "logging", icon: "log" as const, labelKey: "settings.logging" },', '{ id: "logging", icon: "log" as const, labelKey: "settings.logging" },\n  { id: "security", icon: "lock" as const, labelKey: "settings.security" },')
    settings = settings.replace('case "logging": return <LoggingSettings />;', 'case "logging": return <LoggingSettings />;\n      case "security": return <SecuritySettings />;')
write("src/components/Settings/SettingsPage.tsx", settings)

# ---------------------------------------------------------------------------
# 3. TFTP: canonical target containment and explicit exposure confirmation.
# ---------------------------------------------------------------------------
tftp_mod = read("src-tauri/src/plugins/tftp/mod.rs")
if "exposure_confirmed" not in tftp_mod:
    tftp_mod = tftp_mod.replace('pub single_port: bool,', 'pub single_port: bool,\n    /// 用户已明确确认对非 loopback 网络开放可写+覆盖的风险。\n    #[serde(default)]\n    pub exposure_confirmed: bool,')
    risk = 'if is_risky_exposure(listen_ip, &config) {'
    tftp_mod = tftp_mod.replace(risk, 'if is_risky_exposure(listen_ip, &config) && !config.exposure_confirmed {\n            return Err(SessionError::ConnectionFailed {\n                reason: "TFTP 配置会向非 loopback 网络开放远程写入并允许覆盖；请在连接界面明确确认该风险".into(),\n            });\n        }\n        if is_risky_exposure(listen_ip, &config) {', 1)
write("src-tauri/src/plugins/tftp/mod.rs", tftp_mod)

server = read("src-tauri/src/plugins/tftp/server.rs")
# Replace RRQ/WRQ lexical-only checks with canonical containment.
old_rrq = '''    let file_path = sanitize_filename(filename);\n    let full_path = root.join(&file_path);'''
new_rrq = '''    let full_path = match resolve_read_path(root, filename) {\n        Ok(path) => path,\n        Err(message) => {\n            log::warn!("[TFTP Server] 拒绝 RRQ 路径 {}: {}", filename, message);\n            return send_path_error(remote, ErrorCode::AccessViolation, "access violation");\n        }\n    };'''
# We cannot call send_path_error before closure is defined. Do targeted replacement after closure instead.
# Keep initial construction, then replace validation blocks below.
rrq_validation = '''    if !validate_file_path(&full_path, root) {\n        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);\n        send_error(ErrorCode::AccessViolation, "access violation");\n        return;\n    }\n    if !full_path.exists() {'''
rrq_new = '''    let full_path = match resolve_read_path(root, filename) {\n        Ok(path) => path,\n        Err(PathResolveError::NotFound) => {\n            log::warn!("[TFTP Server] 文件不存在: {}", filename);\n            send_error(ErrorCode::FileNotFound, "file not found");\n            return;\n        }\n        Err(error) => {\n            log::warn!("[TFTP Server] 拒绝 RRQ 路径 {}: {:?}", filename, error);\n            send_error(ErrorCode::AccessViolation, "access violation");\n            return;\n        }\n    };\n    if !full_path.exists() {'''
if rrq_validation in server:
    server = server.replace(rrq_validation, rrq_new, 1)

wrq_validation = '''    if !validate_file_path(&full_path, root) {\n        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);\n        send_error(ErrorCode::AccessViolation, "access violation");\n        return;\n    }\n    if full_path.exists() && !overwrite {'''
wrq_new = '''    let full_path = match resolve_write_path(root, filename) {\n        Ok(path) => path,\n        Err(error) => {\n            log::warn!("[TFTP Server] 拒绝 WRQ 路径 {}: {:?}", filename, error);\n            send_error(ErrorCode::AccessViolation, "access violation");\n            return;\n        }\n    };\n    if full_path.exists() && !overwrite {'''
if wrq_validation in server:
    server = server.replace(wrq_validation, wrq_new, 1)

# Replace legacy sanitizer/validator tail with canonical resolver helpers and tests.
start = server.find("/// 清理文件名：")
if start == -1:
    raise RuntimeError("TFTP path helper marker not found")
# Preserve anything before the helper; helper is at file tail in current source.
server = server[:start] + r'''#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathResolveError {
    InvalidName,
    Escape,
    NotFound,
    Io,
}

fn relative_request_path(filename: &str) -> Result<PathBuf, PathResolveError> {
    let path = Path::new(filename);
    if path.as_os_str().is_empty() {
        return Err(PathResolveError::InvalidName);
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return Err(PathResolveError::InvalidName),
        }
    }
    Ok(path.to_path_buf())
}

fn resolve_read_path(root: &Path, filename: &str) -> Result<PathBuf, PathResolveError> {
    let relative = relative_request_path(filename)?;
    let candidate = root.join(relative);
    let resolved = candidate.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PathResolveError::NotFound
        } else {
            PathResolveError::Io
        }
    })?;
    if !resolved.starts_with(root) {
        return Err(PathResolveError::Escape);
    }
    Ok(resolved)
}

fn resolve_write_path(root: &Path, filename: &str) -> Result<PathBuf, PathResolveError> {
    let relative = relative_request_path(filename)?;
    let candidate = root.join(relative);
    if candidate.exists() {
        let resolved = candidate.canonicalize().map_err(|_| PathResolveError::Io)?;
        return resolved
            .starts_with(root)
            .then_some(resolved)
            .ok_or(PathResolveError::Escape);
    }

    let parent = candidate.parent().ok_or(PathResolveError::InvalidName)?;
    let resolved_parent = parent.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PathResolveError::NotFound
        } else {
            PathResolveError::Io
        }
    })?;
    if !resolved_parent.starts_with(root) {
        return Err(PathResolveError::Escape);
    }
    let file_name = candidate.file_name().ok_or(PathResolveError::InvalidName)?;
    Ok(resolved_parent.join(file_name))
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal_and_allows_nested_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/file.bin"), b"ok").unwrap();
        assert_eq!(resolve_read_path(&root, "../outside"), Err(PathResolveError::InvalidName));
        assert!(resolve_read_path(&root, "nested/file.bin").is_ok());
        assert!(resolve_write_path(&root, "nested/new.bin").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_for_reads_and_writes() {
        use std::os::unix::fs::symlink;
        let root_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();
        std::fs::write(outside.path().join("secret"), b"nope").unwrap();
        symlink(outside.path(), root.join("escape")).unwrap();
        assert_eq!(resolve_read_path(&root, "escape/secret"), Err(PathResolveError::Escape));
        assert_eq!(resolve_write_path(&root, "escape/new"), Err(PathResolveError::Escape));
    }
}
'''
write("src-tauri/src/plugins/tftp/server.rs", server)

# Frontend confirmation; backend still independently enforces the flag.
connect = read("src/components/Layout/ConnectDialog.tsx")
if "tftpExposureConfirmed" not in connect:
    insertion = '''    let tftpExposureConfirmed = false;\n    if (isTftp && tftpWriteEnabled && tftpOverwrite) {\n      const bind = tftpListenIp.trim().toLowerCase();\n      const loopback = bind === "127.0.0.1" || bind === "::1" || bind === "localhost";\n      if (!loopback) {\n        tftpExposureConfirmed = window.confirm(\n          t("tftp.exposureWarning", { defaultValue: "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Only continue on a trusted network." })\n        );\n        if (!tftpExposureConfirmed) {\n          setConnecting(false);\n          return;\n        }\n      }\n    }\n\n'''
    connect = replace_once(connect, "    const params: Record<string, unknown> = isSerial ? {", insertion + "    const params: Record<string, unknown> = isSerial ? {", "TFTP confirmation insertion")
    connect = connect.replace("      single_port: tftpSinglePort,", "      single_port: tftpSinglePort,\n      exposure_confirmed: tftpExposureConfirmed,", 1)
write("src/components/Layout/ConnectDialog.tsx", connect)

# README claims are now true for credential persistence.
for doc in ["README.md", "README.zh-CN.md"]:
    text = read(doc)
    text = text.replace("Credential persistence is currently in-memory only; OS-native keyring storage remains planned hardening work.", "Credentials use the OS secure store when available, with an explicitly unlocked Argon2id + AES-256-GCM vault fallback.")
    text = text.replace("凭据当前仅保存在进程内存中；OS 原生 keyring 与加密持久化仍属于后续安全强化。", "凭据优先使用系统原生安全存储；不可用时可显式解锁 Argon2id + AES-256-GCM 加密 vault。")
    write(doc, text)

print("hardening migration applied")
