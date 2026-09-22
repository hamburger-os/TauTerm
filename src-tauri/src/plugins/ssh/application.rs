//! SSH application-layer persistence and credential policy.
//!
//! The common Session command layer treats plugin params as opaque JSON. SSH owns how transient
//! authentication material is projected into the Session Library and hydrated again at connect time.

use serde_json::Value;

use crate::kernel::session_store::SavedSession;
use crate::plugin_application::{
    PreparedSessionConfig, SessionConfigHandler, SessionConfigServices,
};
use crate::security::credential_store::{
    CredentialStore, CredentialStoreError, CredentialType, CredentialValue,
};

use super::{SshConfig, SshConnectionParams};

const CREDENTIAL_ACCOUNT_KEY: &str = "credential_account";

fn credential_account(session_id: &str) -> String {
    format!("ssh-session:{session_id}")
}

fn non_empty_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn credential_from_params(
    params: &Value,
) -> Result<Option<(CredentialType, CredentialValue)>, String> {
    let auth_method = params
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("password");

    match auth_method {
        "password" => Ok(non_empty_param(params, "password").map(|password| {
            (
                CredentialType::Password,
                CredentialValue::Password(password.to_string()),
            )
        })),
        "key" => Ok(non_empty_param(params, "private_key").map(|private_key| {
            let passphrase = non_empty_param(params, "passphrase").map(str::to_string);
            (
                CredentialType::SshKey,
                CredentialValue::SshKey {
                    private_key: private_key.to_string(),
                    passphrase,
                },
            )
        })),
        other => Err(format!("不支持的 SSH 认证方式: {other}")),
    }
}

fn credential_matches_auth(auth_method: &str, credential: &CredentialValue) -> bool {
    matches!(
        (auth_method, credential),
        ("password", CredentialValue::Password(_)) | ("key", CredentialValue::SshKey { .. })
    )
}

fn strip_secret_fields(params: &mut Value) -> Result<bool, String> {
    let object = params
        .as_object_mut()
        .ok_or_else(|| "SSH 会话参数必须是 JSON object".to_string())?;
    let mut changed = false;
    changed |= object.remove("password").is_some();
    changed |= object.remove("private_key").is_some();
    changed |= object.remove("passphrase").is_some();
    Ok(changed)
}

fn sanitize_saved_session(session: &mut SavedSession) -> Result<bool, String> {
    strip_secret_fields(&mut session.params)
}

#[derive(Debug)]
pub(crate) struct PendingSshCredential {
    account: String,
    credential_type: CredentialType,
    value: CredentialValue,
    description: String,
}

/// Prepare the canonical SSH Session Library representation without mutating the credential store.
/// Plaintext secrets are transient input only; persisted params contain a deterministic credential
/// account reference and the actual secret is committed after the Session Library transaction.
pub(crate) fn prepare_session_params(
    credential_store: &CredentialStore,
    session_id: &str,
    params: &mut Value,
) -> Result<Option<PendingSshCredential>, String> {
    let auth_method = params
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("password")
        .to_string();
    let account = credential_account(session_id);

    let pending = if let Some((credential_type, value)) = credential_from_params(params)? {
        let username = params
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let host = params
            .get("host")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Some(PendingSshCredential {
            account: account.clone(),
            credential_type,
            value,
            description: format!("SSH {username}@{host}"),
        })
    } else {
        match credential_store.get_credential(&account) {
            Ok(value) if credential_matches_auth(&auth_method, &value) => {}
            Ok(_) => return Err("SSH 认证方式已变更，请重新输入对应凭据".into()),
            Err(CredentialStoreError::NotFound(_)) => {
                return Err("SSH 会话没有可用的安全凭据，请重新输入密码或私钥".into());
            }
            Err(error) => return Err(format!("无法读取 SSH 安全凭据: {error}")),
        }
        None
    };

    let object = params
        .as_object_mut()
        .ok_or_else(|| "SSH 会话参数必须是 JSON object".to_string())?;
    object.insert(CREDENTIAL_ACCOUNT_KEY.to_string(), Value::String(account));
    strip_secret_fields(params)?;
    Ok(pending)
}

pub(crate) fn commit_credential(
    credential_store: &CredentialStore,
    pending: PendingSshCredential,
) -> Result<(), String> {
    credential_store
        .store_credential(
            &pending.account,
            pending.credential_type,
            pending.value,
            &pending.description,
        )
        .map_err(|error| format!("无法安全保存 SSH 凭据: {error}"))
}

fn prepare_persisted_config(
    services: &SessionConfigServices<'_>,
    session_id: &str,
    params: &mut Value,
) -> Result<PreparedSessionConfig, String> {
    let pending = prepare_session_params(services.credential_store, session_id, params)?;
    Ok(match pending {
        Some(pending) => {
            PreparedSessionConfig::with_commit(move |store| commit_credential(store, pending))
        }
        None => PreparedSessionConfig::unchanged(),
    })
}

fn delete_session_config(
    credential_store: &CredentialStore,
    session_id: &str,
) -> Result<(), String> {
    credential_store
        .delete_credential(&credential_account(session_id))
        .map_err(|error| format!("无法删除 SSH 安全凭据: {error}"))
}

pub(crate) fn session_config_handler() -> SessionConfigHandler {
    SessionConfigHandler {
        validate: None,
        prepare: prepare_persisted_config,
        default_name: None,
        sanitize_saved: Some(sanitize_saved_session),
        delete: Some(delete_session_config),
    }
}

fn runtime_config(params: &Value, mut credential: CredentialValue) -> Result<SshConfig, String> {
    let connection: SshConnectionParams = serde_json::from_value(params.clone())
        .map_err(|error| format!("SSH 配置解析失败: {error}"))?;

    match (connection.auth_method.as_str(), &mut credential) {
        ("password", CredentialValue::Password(password)) => {
            Ok(SshConfig::password(connection, std::mem::take(password)))
        }
        (
            "key",
            CredentialValue::SshKey {
                private_key,
                passphrase,
            },
        ) => Ok(SshConfig::key(
            connection,
            std::mem::take(private_key),
            passphrase.take(),
        )),
        _ => Err("SSH 安全凭据类型与当前认证方式不匹配，请重新配置会话".into()),
    }
}

pub(crate) fn hydrate_config(
    credential_store: &CredentialStore,
    params: &Value,
) -> Result<SshConfig, String> {
    let account = params
        .get(CREDENTIAL_ACCOUNT_KEY)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "SSH 会话缺少安全凭据引用，请重新配置会话".to_string())?;
    let credential = credential_store
        .get_credential(account)
        .map_err(|error| format!("无法读取 SSH 安全凭据: {error}"))?;
    runtime_config(params, credential)
}

pub(crate) fn hydrate_config_with_pending(
    credential_store: &CredentialStore,
    params: &Value,
    pending: Option<&PendingSshCredential>,
) -> Result<SshConfig, String> {
    if let Some(pending) = pending {
        runtime_config(params, pending.value.clone())
    } else {
        hydrate_config(credential_store, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_type_must_match_auth_method() {
        assert!(credential_matches_auth(
            "password",
            &CredentialValue::Password("secret".into())
        ));
        assert!(!credential_matches_auth(
            "key",
            &CredentialValue::Password("secret".into())
        ));
    }
}
