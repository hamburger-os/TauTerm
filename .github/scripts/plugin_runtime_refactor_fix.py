from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


# Diagnostics reads the canonical PluginRuntime directly; there is no second manifest registry.
path = "src-tauri/src/diagnostics.rs"
text = read(path)
text = replace_once(
    text,
    '''    let plugins = {
        let host = state
            .plugin_host
            .lock()
            .map_err(|error| error.to_string())?;
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
    };''',
    '''    let plugins = {
        let mut plugins = state
            .plugins
            .manifests()
            .into_iter()
            .map(|manifest| PluginDiagnostic {
                id: manifest.id.to_string(),
                version: manifest.version.clone(),
                category: manifest.category.clone(),
            })
            .collect::<Vec<_>>();
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    };''',
    "diagnostics PluginRuntime",
)
write(path, text)

# Modbus is a plugin-local command, but its adapter instance is still owned by PluginRuntime.
path = "src-tauri/src/plugins/modbus/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''    let conn = state
        .modbus_adapter
        .connect(&endpoint, &params)
        .await
        .map_err(|error| error.to_string())?;''',
    '''    let conn = state
        .plugin::<ModbusAdapter>(PLUGIN_ID)
        .connect(&endpoint, &params)
        .await
        .map_err(|error| error.to_string())?;''',
    "Modbus PluginRuntime adapter",
)
text = replace_once(
    text,
    '''
    pub fn runtime(&self, session_id: &str) -> Option<Arc<ModbusRuntime>> {
        runtime(session_id)
    }
''',
    "\n",
    "obsolete Modbus adapter runtime helper",
)
write(path, text)

# Remove compatibility-style helpers that no longer have a consumer after centralizing runtime ownership.
path = "src-tauri/src/plugins/modbus/client.rs"
text = read(path)
text = replace_once(
    text,
    '''
    pub fn default_unit_id(&self) -> u8 {
        self.config.unit_id
    }
''',
    "\n",
    "unused Modbus default_unit_id helper",
)
write(path, text)

path = "src-tauri/src/plugins/modbus/config.rs"
text = read(path)
text = replace_once(
    text,
    '''
    pub fn serial(&self) -> Option<(SerialMode, &str, &SerialTransportConfig)> {
        match &self.endpoint {
            ModbusEndpointConfig::Serial {
                mode,
                port,
                transport,
            } => Some((*mode, port.as_str(), transport)),
            ModbusEndpointConfig::Tcp { .. } => None,
        }
    }

    pub fn tcp(&self) -> Option<(&str, u16, &TcpConnectConfig)> {
        match &self.endpoint {
            ModbusEndpointConfig::Tcp {
                host,
                port,
                transport,
            } => Some((host.as_str(), *port, transport)),
            ModbusEndpointConfig::Serial { .. } => None,
        }
    }
''',
    "\n",
    "unused Modbus endpoint convenience helpers",
)
write(path, text)

path = "src-tauri/src/plugins/ssh/mod.rs"
text = read(path)
text = replace_once(
    text,
    '''
    pub async fn cancel(&self, request_id: &str) {
        self.cancel_now(request_id);
    }
''',
    "\n",
    "unused SSH verifier compatibility helper",
)
write(path, text)

# TRDP has no ProtocolAdapter identity contract, so it does not need a standalone constant.
path = "src-tauri/src/plugins/trdp.rs"
text = read(path)
text = replace_once(
    text,
    'pub const PLUGIN_ID: &str = "trdp";\n\n',
    "",
    "unused TRDP plugin id constant",
)
write(path, text)

# Fail immediately if legacy AppState access paths ever reappear during this migration.
for source in (ROOT / "src-tauri/src").rglob("*.rs"):
    content = source.read_text(encoding="utf-8")
    if ".plugin_host" in content:
        raise RuntimeError(f"legacy PluginHost access remains in {source.relative_to(ROOT)}")
    if ".modbus_adapter" in content:
        raise RuntimeError(f"legacy Modbus adapter AppState access remains in {source.relative_to(ROOT)}")

print("plugin runtime follow-up fixes applied")
