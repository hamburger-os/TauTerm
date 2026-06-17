//! 凭据存储
//!
//! 安全地管理密码、SSH 密钥、证书和 Token。
//! 主后端：OS 原生 keyring（macOS Keychain / Windows Credential Manager / Linux Secret Service）。
//! 降级后端：AES-256-GCM 加密文件。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// 凭据类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CredentialType {
    Password,
    SshKey,
    Certificate,
    Token,
}

/// 凭据条目元数据（不包含密钥值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub account: String,
    pub credential_type: CredentialType,
    pub description: String,
}

/// 凭据值
#[derive(Debug, Clone)]
pub enum CredentialValue {
    Password(String),
    SshKey { private_key: String, passphrase: Option<String> },
    Certificate { cert_data: Vec<u8>, key_data: Vec<u8> },
    Token(String),
}

/// 凭据存储
///
/// 当前实现使用内存存储作为基础（后续集成 keyring-rs 和 AES-256-GCM 文件降级）。
pub struct CredentialStore {
    /// 内存中的凭据存储（开发阶段）
    /// 生产环境替换为 keyring + AES 降级
    entries: RwLock<HashMap<String, CredentialEntry>>,
    secrets: RwLock<HashMap<String, CredentialValue>>,
}

impl CredentialStore {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            secrets: RwLock::new(HashMap::new()),
        }
    }

    /// 存储凭据
    pub fn store_credential(
        &self,
        account: &str,
        credential_type: CredentialType,
        value: CredentialValue,
        description: &str,
    ) -> Result<(), CredentialStoreError> {
        let mut entries = self.entries.write().map_err(|_| CredentialStoreError::LockError)?;
        let mut secrets = self.secrets.write().map_err(|_| CredentialStoreError::LockError)?;

        entries.insert(account.to_string(), CredentialEntry {
            account: account.to_string(),
            credential_type: credential_type.clone(),
            description: description.to_string(),
        });
        secrets.insert(account.to_string(), value);

        Ok(())
    }

    /// 获取凭据
    pub fn get_credential(&self, account: &str) -> Result<CredentialValue, CredentialStoreError> {
        let secrets = self.secrets.read().map_err(|_| CredentialStoreError::LockError)?;
        secrets.get(account)
            .cloned()
            .ok_or_else(|| CredentialStoreError::NotFound(account.to_string()))
    }

    /// 列出所有凭据（仅元数据，不包含密钥值）
    pub fn list_credentials(&self) -> Result<Vec<CredentialEntry>, CredentialStoreError> {
        let entries = self.entries.read().map_err(|_| CredentialStoreError::LockError)?;
        Ok(entries.values().cloned().collect())
    }

    /// 删除凭据
    pub fn delete_credential(&self, account: &str) -> Result<(), CredentialStoreError> {
        let mut entries = self.entries.write().map_err(|_| CredentialStoreError::LockError)?;
        let mut secrets = self.secrets.write().map_err(|_| CredentialStoreError::LockError)?;
        entries.remove(account);
        secrets.remove(account);
        Ok(())
    }
}

impl Default for CredentialStore {
    fn default() -> Self { Self::new() }
}

/// 凭据存储错误
#[derive(Debug, thiserror::Error)]
pub enum CredentialStoreError {
    #[error("凭据 '{0}' 不存在")]
    NotFound(String),
    #[error("类型不匹配")]
    TypeMismatch,
    #[error("内部锁错误")]
    LockError,
}
