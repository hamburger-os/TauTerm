//! Application-layer plugin contributions.
//!
//! Kernel contracts deliberately stay unaware of credentials, UI commands, and other host services.
//! This module defines the small host-application extension points that need those services while
//! still being registered through the canonical `PluginRuntime`.

use serde_json::Value;
use std::sync::Mutex;

use crate::kernel::session_store::{SavedSession, SessionStore};
use crate::security::credential_store::CredentialStore;

pub(crate) struct SessionConfigServices<'a> {
    pub credential_store: &'a CredentialStore,
    pub session_store: &'a Mutex<SessionStore>,
}

pub(crate) struct PreparedSessionConfig {
    commit: Option<Box<dyn FnOnce(&CredentialStore) -> Result<(), String> + Send>>,
}

impl PreparedSessionConfig {
    pub(crate) fn unchanged() -> Self {
        Self { commit: None }
    }

    pub(crate) fn with_commit<F>(commit: F) -> Self
    where
        F: FnOnce(&CredentialStore) -> Result<(), String> + Send + 'static,
    {
        Self {
            commit: Some(Box::new(commit)),
        }
    }

    pub(crate) fn commit(self, credential_store: &CredentialStore) -> Result<(), String> {
        match self.commit {
            Some(commit) => commit(credential_store),
            None => Ok(()),
        }
    }
}

pub(crate) type ValidateSessionConfig = fn(&Value) -> Result<(), String>;
pub(crate) type PrepareSessionConfig = fn(
    services: &SessionConfigServices<'_>,
    session_id: &str,
    params: &mut Value,
) -> Result<PreparedSessionConfig, String>;
pub(crate) type DefaultSessionName = fn(&Value, &str) -> Result<String, String>;
pub(crate) type SanitizeSavedSession = fn(&mut SavedSession) -> Result<bool, String>;

#[derive(Clone, Copy)]
pub(crate) struct SessionConfigHandler {
    pub validate: Option<ValidateSessionConfig>,
    pub prepare: PrepareSessionConfig,
    pub default_name: Option<DefaultSessionName>,
    pub sanitize_saved: Option<SanitizeSavedSession>,
}

pub(crate) fn unchanged_session_config(
    _services: &SessionConfigServices<'_>,
    _session_id: &str,
    _params: &mut Value,
) -> Result<PreparedSessionConfig, String> {
    Ok(PreparedSessionConfig::unchanged())
}
