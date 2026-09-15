from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 match, got {count}")
    return text.replace(old, new, 1)


def replace_function(text: str, marker: str, replacement: str, label: str) -> str:
    start = text.find(marker)
    if start < 0:
        raise SystemExit(f"{label}: marker not found")
    brace = text.find("{", start)
    if brace < 0:
        raise SystemExit(f"{label}: opening brace not found")
    depth = 0
    end = None
    for idx in range(brace, len(text)):
        ch = text[idx]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                end = idx + 1
                break
    if end is None:
        raise SystemExit(f"{label}: closing brace not found")
    return text[:start] + replacement.rstrip() + text[end:]


def remove_test(text: str, name: str) -> str:
    marker = f"    #[test]\n    fn {name}"
    start = text.find(marker)
    if start < 0:
        return text
    brace = text.find("{", start)
    depth = 0
    end = None
    for idx in range(brace, len(text)):
        if text[idx] == "{":
            depth += 1
        elif text[idx] == "}":
            depth -= 1
            if depth == 0:
                end = idx + 1
                break
    if end is None:
        raise SystemExit(f"test {name}: closing brace not found")
    while end < len(text) and text[end] == "\n":
        end += 1
    return text[:start] + text[end:]


# ── Frontend SessionContext: no Local Shell special policy in common state ──
path = Path("src/context/SessionContext.tsx")
text = path.read_text(encoding="utf-8")
old = '''function localizeSessionError(error: unknown): string {
  const message = String(error);
  return message.includes("User cancelled the UAC elevation prompt")
    ? i18n.t("localShell.elevationCancelled")
    : message;
}

'''
text = replace_once(text, old, "", "SessionContext localizeSessionError")
text = replace_once(
    text,
    '''      const presentationName = plugin?.sessionPresentation?.defaultName?.(normalizedParams, endpoint)?.trim();
      const effectiveName = requestedName || (pluginId === "local-shell"
        ? await invoke<string>("resolve_local_shell_session_name", { params })
        : presentationName || `${pluginName} @ ${endpoint}`);''',
    '''      const presentationName = plugin?.sessionPresentation?.defaultName?.(normalizedParams, endpoint)?.trim();
      const resolvedName = plugin?.resolveDefaultSessionName
        ? (await plugin.resolveDefaultSessionName(normalizedParams, endpoint)).trim()
        : "";
      const effectiveName = requestedName || resolvedName || presentationName || `${pluginName} @ ${endpoint}`;''',
    "SessionContext default name contribution",
)
text = replace_once(
    text,
    '      dispatch({ type: "SET_ERROR", error: `${i18n.t("localShell.connectFailed")}: ${localizeSessionError(e)}` });',
    '      dispatch({ type: "SET_ERROR", error: String(e) });',
    "SessionContext generic connect error",
)
text = replace_once(
    text,
    '      dispatch({ type: "SET_ERROR", error: `${i18n.t("localShell.openFailed")}: ${localizeSessionError(e)}` });',
    '      dispatch({ type: "SET_ERROR", error: String(e) });',
    "SessionContext generic channel error",
)
path.write_text(text, encoding="utf-8")

# ── SSH application policy completes delete lifecycle ──
path = Path("src-tauri/src/plugins/ssh/application.rs")
text = path.read_text(encoding="utf-8")
insert = '''fn delete_session_config(credential_store: &CredentialStore, session_id: &str) -> Result<(), String> {
    credential_store
        .delete_credential(&credential_account(session_id))
        .map_err(|error| format!("无法删除 SSH 安全凭据: {error}"))
}

'''
text = replace_once(
    text,
    "pub(crate) fn session_config_handler() -> SessionConfigHandler {\n",
    insert + "pub(crate) fn session_config_handler() -> SessionConfigHandler {\n",
    "SSH delete hook insertion",
)
text = replace_once(
    text,
    '''        sanitize_saved: Some(sanitize_saved_session),
    }''',
    '''        sanitize_saved: Some(sanitize_saved_session),
        delete: Some(delete_session_config),
    }''',
    "SSH config handler delete field",
)
path.write_text(text, encoding="utf-8")

# ── Local Shell owns save validation/default naming and its command ──
path = Path("src-tauri/src/plugins/local_shell/mod.rs")
text = path.read_text(encoding="utf-8")
marker = 'pub const PLUGIN_ID: &str = "local-shell";\n\n'
addition = '''pub const PLUGIN_ID: &str = "local-shell";

pub(crate) fn session_config_handler() -> crate::plugin_application::SessionConfigHandler {
    crate::plugin_application::SessionConfigHandler {
        validate: Some(LocalShellAdapter::validate_params),
        prepare: crate::plugin_application::unchanged_session_config,
        default_name: Some(|params, _endpoint| LocalShellAdapter::default_session_name(params)),
        sanitize_saved: None,
        delete: None,
    }
}

#[tauri::command]
pub fn resolve_local_shell_session_name(params: serde_json::Value) -> Result<String, String> {
    LocalShellAdapter::default_session_name(&params)
}

'''
text = replace_once(text, marker, addition, "Local Shell config handler")
path.write_text(text, encoding="utf-8")

# ── TRDP runtime index is plugin-owned, not process-global ──
path = Path("src-tauri/src/plugins/trdp.rs")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    'use crate::kernel::plugin_adapter::{SessionAttach, SessionService};\n',
    'use crate::kernel::plugin_adapter::{SessionAttach, SessionService};\nuse crate::kernel::plugin_runtime::SessionRuntimeRegistry;\n',
    "TRDP runtime registry import",
)
text = replace_once(
    text,
    'pub mod xml;\n\n',
    'pub mod xml;\n\npub const PLUGIN_ID: &str = "trdp";\n\n',
    "TRDP plugin id",
)
old = '''fn runtime_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<TrdpRuntime>>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<TrdpRuntime>>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn runtime(session_id: &str) -> Option<Arc<TrdpRuntime>> {
    runtime_registry().lock().ok()?.get(session_id).cloned()
}

struct RuntimeAttach {
    runtime: Arc<TrdpRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        if let Ok(mut map) = runtime_registry().lock() {
            map.insert(session_id.to_string(), self.runtime.clone());
        }
    }
    fn on_detached(&self, session_id: &str) {
        if let Ok(mut map) = runtime_registry().lock() {
            map.remove(session_id);
        }
    }
}
'''
new = '''pub struct TrdpPlugin {
    runtimes: SessionRuntimeRegistry<TrdpRuntime>,
}

impl TrdpPlugin {
    pub fn new() -> Self {
        Self {
            runtimes: SessionRuntimeRegistry::new(),
        }
    }

    pub fn runtime(&self, session_id: &str) -> Option<Arc<TrdpRuntime>> {
        self.runtimes.get(session_id)
    }
}

struct RuntimeAttach {
    runtime: Arc<TrdpRuntime>,
    runtimes: SessionRuntimeRegistry<TrdpRuntime>,
}
impl SessionAttach for RuntimeAttach {
    fn on_attached(&self, session_id: &str) {
        self.runtimes.attach(session_id, &self.runtime);
    }
    fn on_detached(&self, session_id: &str) {
        self.runtimes.detach(session_id);
    }
}

fn validate_session_config(params: &Value) -> Result<(), String> {
    match params.get("mode").and_then(Value::as_str).unwrap_or("node") {
        "node" | "monitor" => Ok(()),
        mode => Err(format!("未知 TRDP 会话模式: {mode}")),
    }
}

fn prepare_session_config(
    services: &crate::plugin_application::SessionConfigServices<'_>,
    session_id: &str,
    params: &mut Value,
) -> Result<crate::plugin_application::PreparedSessionConfig, String> {
    if params.get("trdp_workspace").is_none() {
        let workspace = services
            .session_store
            .lock()
            .ok()
            .and_then(|store| store.get_session(session_id))
            .and_then(|handle| handle.params.get("trdp_workspace"))
            .cloned();
        if let (Some(workspace), Some(object)) = (workspace, params.as_object_mut()) {
            object.insert("trdp_workspace".to_string(), workspace);
        }
    }
    Ok(crate::plugin_application::PreparedSessionConfig::unchanged())
}

fn default_session_name(params: &Value, _endpoint: &str) -> Result<String, String> {
    validate_session_config(params)?;
    Ok(if params.get("mode").and_then(Value::as_str) == Some("monitor") {
        "TRDP @ Monitor".to_string()
    } else {
        "TRDP @ Node".to_string()
    })
}

pub(crate) fn session_config_handler() -> crate::plugin_application::SessionConfigHandler {
    crate::plugin_application::SessionConfigHandler {
        validate: Some(validate_session_config),
        prepare: prepare_session_config,
        default_name: Some(default_session_name),
        sanitize_saved: None,
        delete: None,
    }
}
'''
text = replace_once(text, old, new, "TRDP static runtime registry")
text = replace_once(
    text,
    '    let runtime = Arc::new(TrdpRuntime::new(params.clone()));\n',
    '    let plugin = state.plugin::<TrdpPlugin>(PLUGIN_ID);\n    let runtime = Arc::new(TrdpRuntime::new(params.clone()));\n',
    "TRDP connect plugin lookup",
)
text = replace_once(
    text,
    '''                attachment: Some(Arc::new(RuntimeAttach {
                    runtime: runtime.clone(),
                })),''',
    '''                attachment: Some(Arc::new(RuntimeAttach {
                    runtime: runtime.clone(),
                    runtimes: plugin.runtimes.clone(),
                })),''',
    "TRDP attachment registry",
)
text = replace_once(
    text,
    '    let trdp = runtime(&session_id).ok_or("会话不是 TRDP 会话")?;\n',
    '    let trdp = state\n        .plugin::<TrdpPlugin>(PLUGIN_ID)\n        .runtime(&session_id)\n        .ok_or("会话不是 TRDP 会话")?;\n',
    "TRDP command runtime lookup",
)
path.write_text(text, encoding="utf-8")

# ── Common commands consume contributions; SSH credential policy leaves the hub ──
path = Path("src-tauri/src/commands.rs")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    'use crate::AppState;\n',
    'use crate::plugin_application::{SessionConfigHandler, SessionConfigServices};\nuse crate::AppState;\n',
    "commands config contribution import",
)
text = replace_once(
    text,
    '    pub journald_enabled: Option<bool>,\n',
    '',
    "ConnectSessionRequest journald field",
)
start = text.find('const SSH_CREDENTIAL_ACCOUNT_KEY: &str = "credential_account";')
end_marker = '#[derive(Debug, Deserialize)]\n#[serde(rename_all = "camelCase")]\npub struct JournaldQueryRequest'
end = text.find(end_marker, start)
if start < 0 or end < 0:
    raise SystemExit("commands old SSH credential block markers missing")
text = text[:start] + text[end:]
text = replace_once(
    text,
    '        send_bar_enabled,\n        journald_enabled,\n        session_id,\n',
    '        send_bar_enabled,\n        session_id,\n',
    "SSH connect destructure journald",
)
text = replace_once(
    text,
    '''    let pending_ssh_credential =
        prepare_ssh_session_params(&state, &effective_session_id, &mut params)?;
    let ssh_config =
        hydrate_ssh_config_with_pending(&state, &params, pending_ssh_credential.as_ref())?;

    // 将 journald_enabled 提升为 params 的通用字段（不再耦合 SshConfig）。
    // reconfigure/restore 统一从当前 Session params 读取。
    let journald_enabled_val = if let Some(obj) = params.as_object_mut() {
        if let Some(existing) = obj.get("journald_enabled").and_then(|v| v.as_bool()) {
            existing
        } else {
            let v = journald_enabled.unwrap_or(false);
            obj.insert("journald_enabled".to_string(), serde_json::Value::Bool(v));
            v
        }
    } else {
        journald_enabled.unwrap_or(false)
    };''',
    '''    let pending_ssh_credential = crate::plugins::ssh::application::prepare_session_params(
        &state.credential_store,
        &effective_session_id,
        &mut params,
    )?;
    let ssh_config = crate::plugins::ssh::application::hydrate_config_with_pending(
        &state.credential_store,
        &params,
        pending_ssh_credential.as_ref(),
    )?;
    let journald_enabled_val = params
        .get("journald_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);''',
    "SSH connect application policy",
)
text = text.replace(
    'commit_ssh_credential(&state, pending)',
    'crate::plugins::ssh::application::commit_credential(&state.credential_store, pending)',
)

new_load = '''#[tauri::command]
pub async fn load_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SavedSessionInfo>, String> {
    let path = SessionStore::sessions_file_path(&app)?;
    let mut saved = SessionStore::load_from_disk(&path)?;
    let mut changed = false;
    for session in &mut saved {
        if let Some(handler) = state
            .plugins
            .contribution_by_str::<SessionConfigHandler>(&session.plugin_id)
        {
            if let Some(sanitize) = handler.sanitize_saved {
                changed |= sanitize(session)?;
            }
        }
    }
    if changed {
        SessionStore::replace_saved_sessions(&path, &saved)?;
    }

    Ok(saved
        .into_iter()
        .map(|s| SavedSessionInfo {
            id: s.id,
            name: s.name,
            connection_type: s.plugin_id.clone(),
            endpoint: s.endpoint,
            params: s.params,
            timestamp: s.timestamp,
            plugin_id: s.plugin_id,
            transfer_enabled: s.transfer_enabled,
            transfer_protocol: s.transfer_protocol,
            send_bar_enabled: s.send_bar_enabled,
        })
        .collect())
}'''
text = replace_function(text, '#[tauri::command]\npub async fn load_sessions(', new_load, "load_sessions")

new_save = '''#[tauri::command]
pub async fn save_session_config(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SaveSessionConfigRequest,
) -> Result<String, String> {
    let SaveSessionConfigRequest {
        endpoint,
        mut params,
        name,
        plugin_id,
        transfer_enabled,
        transfer_protocol,
        send_bar_enabled,
        session_id,
    } = request;
    let pid = plugin_id;
    let id = if let Some(ref raw) = session_id {
        if uuid::Uuid::parse_str(raw).is_err() {
            return Err(format!("无效的 session_id 格式: {}", raw));
        }
        raw.clone()
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let handler = state
        .plugins
        .contribution_by_str::<SessionConfigHandler>(&pid);
    if let Some(validate) = handler.as_deref().and_then(|handler| handler.validate) {
        validate(&params)?;
    }
    let services = SessionConfigServices {
        credential_store: &state.credential_store,
        session_store: &state.session_store,
    };
    let prepared = if let Some(handler) = handler.as_deref() {
        (handler.prepare)(&services, &id, &mut params)?
    } else {
        crate::plugin_application::unchanged_session_config(&services, &id, &mut params)?
    };

    let session_name = if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
        name
    } else if let Some(default_name) = handler.as_deref().and_then(|handler| handler.default_name) {
        default_name(&params, &endpoint)?
    } else {
        format!("{} @ {}", pid, endpoint)
    };
    let saved = crate::kernel::session_store::SavedSession {
        id: id.clone(),
        name: session_name,
        plugin_id: pid,
        endpoint,
        params,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        transfer_enabled: transfer_enabled.unwrap_or(true),
        transfer_protocol,
        send_bar_enabled: send_bar_enabled.unwrap_or(true),
    };
    SessionStore::save_config_to_disk_transactional(&app, saved, || {
        prepared.commit(&state.credential_store)
    })?;
    Ok(id)
}'''
text = replace_function(text, '#[tauri::command]\npub async fn save_session_config(', new_save, "save_session_config")
text = replace_function(
    text,
    '#[tauri::command]\npub fn resolve_local_shell_session_name(',
    '',
    "remove common local shell name command",
)
new_delete = '''#[tauri::command]
pub async fn delete_session_config(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    SessionStore::delete_config_from_disk_transactional(&app, &session_id, |deleted_session| {
        if let Some(saved) = deleted_session {
            if let Some(handler) = state
                .plugins
                .contribution_by_str::<SessionConfigHandler>(&saved.plugin_id)
            {
                if let Some(delete) = handler.delete {
                    delete(&state.credential_store, &session_id)?;
                }
            }
        }
        Ok(())
    })
}'''
text = replace_function(text, '#[tauri::command]\npub async fn delete_session_config(', new_delete, "delete_session_config")
for name in [
    "ssh_secret_fields_are_removed_from_persisted_params",
    "ssh_credential_type_must_match_auth_method",
    "ssh_session_credential_account_is_stable",
]:
    text = remove_test(text, name)
path.write_text(text, encoding="utf-8")

# ── Composition root registers application contributions ──
path = Path("src-tauri/src/lib.rs")
text = path.read_text(encoding="utf-8")
text = replace_once(text, 'mod performance_contract;\nmod plugins;\n', 'mod performance_contract;\nmod plugin_application;\nmod plugins;\n', "lib plugin_application module")
needle = '''    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/modbus.json"),
        ModbusAdapter::new(),
        commands::modbus_session_connector,
    );

    let trdp_id = runtime'''
replacement = '''    register_builtin_adapter(
        &mut runtime,
        include_str!("../../src/plugin-manifests/modbus.json"),
        ModbusAdapter::new(),
        commands::modbus_session_connector,
    );

    for (plugin_id, handler) in [
        (
            plugins::ssh::PLUGIN_ID,
            plugins::ssh::application::session_config_handler(),
        ),
        (
            plugins::local_shell::PLUGIN_ID,
            plugins::local_shell::session_config_handler(),
        ),
    ] {
        let plugin_id = kernel::plugin_adapter::PluginId::parse(plugin_id)
            .expect("built-in plugin id");
        runtime
            .register_contribution(&plugin_id, handler)
            .unwrap_or_else(|error| panic!("注册 Session 配置 contribution 失败: {error}"));
    }

    let trdp_id = runtime'''
text = replace_once(text, needle, replacement, "lib standard config handlers")
needle = '''    runtime
        .register_contribution(
            &trdp_id,
            commands::trdp_session_connector as commands::SessionConnectHandler,
        )
        .unwrap_or_else(|error| panic!("注册 TRDP 连接 contribution 失败: {error}"));

    runtime
}'''
replacement = '''    runtime
        .register_contribution(
            &trdp_id,
            commands::trdp_session_connector as commands::SessionConnectHandler,
        )
        .unwrap_or_else(|error| panic!("注册 TRDP 连接 contribution 失败: {error}"));
    runtime
        .register_contribution(&trdp_id, plugins::trdp::TrdpPlugin::new())
        .unwrap_or_else(|error| panic!("注册 TRDP runtime contribution 失败: {error}"));
    runtime
        .register_contribution(&trdp_id, plugins::trdp::session_config_handler())
        .unwrap_or_else(|error| panic!("注册 TRDP Session 配置 contribution 失败: {error}"));

    runtime
}'''
text = replace_once(text, needle, replacement, "lib TRDP contributions")
text = replace_once(
    text,
    '            commands::resolve_local_shell_session_name,\n',
    '            plugins::local_shell::resolve_local_shell_session_name,\n',
    "lib local shell command ownership",
)
path.write_text(text, encoding="utf-8")

# ── Architecture tests cover all plugin files and common config policy ──
path = Path("src-tauri/src/architecture_contract.rs")
text = path.read_text(encoding="utf-8")
old = '''#[test]
fn plugin_session_runtime_indices_are_adapter_owned() {
    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugins");
    for entry in std::fs::read_dir(&plugins_dir).expect("plugins directory") {
        let entry = entry.expect("plugin entry");
        let module = entry.path().join("mod.rs");
        if !module.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&module).expect("plugin module source");
        assert!(
            !source.contains("fn runtime_registry("),
            "plugin module {} must keep Session runtime indices on its Adapter instance, not in process-global static state",
            module.display()
        );
    }
}
'''
new = '''#[test]
fn plugin_session_runtime_indices_are_plugin_owned() {
    fn visit(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("plugin directory") {
            let path = entry.expect("plugin entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugins");
    let mut files = Vec::new();
    visit(&plugins_dir, &mut files);
    for path in files {
        let source = std::fs::read_to_string(&path).expect("plugin source");
        assert!(
            !source.contains("fn runtime_registry(") && !source.contains("static REGISTRY: std::sync::OnceLock"),
            "plugin source {} must keep Session runtime indices on a registered plugin/Adapter instance",
            path.display()
        );
    }
}

#[test]
fn common_session_config_commands_are_plugin_driven() {
    let source = read_source("commands.rs");
    assert!(source.contains("SessionConfigHandler"));
    for forbidden in [
        r#"pid == "ssh""#,
        r#"pid == "trdp""#,
        r#"pid == "local-shell""#,
        "SSH_CREDENTIAL_ACCOUNT_KEY",
        "prepare_ssh_session_params",
        "scrub_ssh_secrets_from_saved_sessions",
    ] {
        assert!(
            !source.contains(forbidden),
            "common Session config commands must not own plugin policy: {forbidden}"
        );
    }
}
'''
text = replace_once(text, old, new, "architecture runtime ownership test")
path.write_text(text, encoding="utf-8")

print("final plugin-boundary migration applied")
