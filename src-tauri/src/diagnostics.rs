//! Sanitized diagnostic bundle export.
//!
//! Diagnostic bundles contain health/shape metadata only. They intentionally exclude credentials,
//! session endpoints, session names, raw payloads and Session Data Log files.

use crate::kernel::session_store::SessionState;
use crate::security::log_sanitizer::sanitize_log;
use crate::AppState;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::State;

const MAX_RECENT_LOG_LINES: usize = 200;

#[derive(Debug, Serialize)]
struct PluginDiagnostic {
    id: String,
    version: String,
    category: String,
}

#[derive(Debug, Default, Serialize)]
struct SessionAggregate {
    total: u64,
    connecting: u64,
    connected: u64,
    transferring: u64,
    disconnected: u64,
    child_connections: u64,
    tx_bytes: u64,
    rx_bytes: u64,
}

#[derive(Debug, Serialize)]
struct VirtualPortDiagnostic {
    resources_present: bool,
    backend_available: bool,
    pending_orphans: u32,
}

#[derive(Debug, Serialize)]
struct CredentialDiagnostic {
    backend: String,
    native_available: bool,
    fallback_configured: bool,
    fallback_unlocked: bool,
}

#[derive(Debug, Serialize)]
struct LogDiagnostic {
    dropped_system_entries: u64,
    dropped_session_entries: u64,
}

#[derive(Debug, Serialize)]
struct DiagnosticBundle {
    schema_version: u32,
    generated_at: String,
    app_version: &'static str,
    os: &'static str,
    arch: &'static str,
    config_store_ready: bool,
    credential: CredentialDiagnostic,
    logging: LogDiagnostic,
    virtual_port: VirtualPortDiagnostic,
    plugins: Vec<PluginDiagnostic>,
    sessions: BTreeMap<String, SessionAggregate>,
    recent_system_log: Vec<String>,
}

fn redact_environment_paths(input: &str) -> String {
    let mut output = input.to_string();
    if let Some(base) = directories::BaseDirs::new() {
        let home = base.home_dir().to_string_lossy();
        if !home.is_empty() {
            output = output.replace(home.as_ref(), "<home>");
        }
    }
    if let Ok(ipv4) = regex::Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b") {
        output = ipv4.replace_all(&output, "<ip>").into_owned();
    }
    output
}

fn latest_system_log(log_dir: &Path) -> Option<PathBuf> {
    let mut candidates = std::fs::read_dir(log_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("TauTerm_") && name.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    candidates.pop()
}

fn recent_sanitized_system_log(log_dir: &Path) -> Vec<String> {
    let Some(path) = latest_system_log(log_dir) else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines = raw.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(MAX_RECENT_LOG_LINES);
    lines[start..]
        .iter()
        .map(|line| redact_environment_paths(&sanitize_log(line)))
        .collect()
}

#[tauri::command]
pub async fn export_diagnostics(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let plugins = {
        let host = state.plugin_host.lock().map_err(|error| error.to_string())?;
        let mut plugins = host
            .plugins()
            .into_iter()
            .map(|manifest| PluginDiagnostic {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                category: manifest.category.clone(),
            })
            .collect::<Vec<_>>();
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    };

    let sessions = {
        let store = state.session_store.lock().map_err(|error| error.to_string())?;
        let mut aggregates = BTreeMap::<String, SessionAggregate>::new();
        for id in store.tab_ids() {
            let Some(session) = store.get_session(&id) else {
                continue;
            };
            let aggregate = aggregates.entry(session.plugin_id.clone()).or_default();
            aggregate.total += 1;
            match session.state {
                SessionState::Connecting => aggregate.connecting += 1,
                SessionState::Connected => aggregate.connected += 1,
                SessionState::Transferring => aggregate.transferring += 1,
                SessionState::Disconnected => aggregate.disconnected += 1,
            }
            aggregate.child_connections += session.sub_connections.len() as u64;
            aggregate.tx_bytes += session
                .tx_bytes
                .load(std::sync::atomic::Ordering::Relaxed);
            aggregate.rx_bytes += session
                .rx_bytes
                .load(std::sync::atomic::Ordering::Relaxed);
        }
        aggregates
    };

    let credential_status = state.credential_store.status();
    let credential = CredentialDiagnostic {
        backend: credential_status.backend.to_string(),
        native_available: credential_status.native_available,
        fallback_configured: credential_status.fallback_configured,
        fallback_unlocked: credential_status.fallback_unlocked,
    };

    let (logging, log_dir) = {
        let log_engine = state.log_engine.lock().map_err(|error| error.to_string())?;
        let health = log_engine.get_health();
        let config = log_engine.get_config()?;
        (
            LogDiagnostic {
                dropped_system_entries: health.dropped_system_entries,
                dropped_session_entries: health.dropped_session_entries,
            },
            config.log_dir,
        )
    };

    // This command is async, so platform probing cannot block the WebView/main command path.
    let virtual_port = {
        let vpm = state
            .virtual_port_manager
            .lock()
            .map_err(|error| error.to_string())?;
        VirtualPortDiagnostic {
            resources_present: vpm.are_files_present(),
            backend_available: vpm.detect_driver(),
            pending_orphans: vpm.pending_orphan_count(),
        }
    };

    let mut bundle = DiagnosticBundle {
        schema_version: 1,
        generated_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        config_store_ready: state.config_store.persistence_ready(),
        credential,
        logging,
        virtual_port,
        plugins,
        sessions,
        recent_system_log: Vec::new(),
    };

    let destination = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        bundle.recent_system_log = recent_sanitized_system_log(&log_dir);
        let json = serde_json::to_vec_pretty(&bundle)
            .map_err(|error| format!("serialize diagnostics failed: {error}"))?;
        crate::kernel::persistence::atomic_write(&destination, &json)
            .map_err(|error| format!("write diagnostics failed: {error}"))
    })
    .await
    .map_err(|error| format!("diagnostics worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_log_redacts_home_and_ipv4_addresses() {
        let home = directories::BaseDirs::new()
            .map(|base| base.home_dir().to_string_lossy().to_string())
            .unwrap_or_default();
        let sample = format!("failed at {home}/workspace while connecting 192.168.1.20");
        let redacted = redact_environment_paths(&sample);
        if !home.is_empty() {
            assert!(!redacted.contains(&home));
            assert!(redacted.contains("<home>"));
        }
        assert!(!redacted.contains("192.168.1.20"));
        assert!(redacted.contains("<ip>"));
    }
}
