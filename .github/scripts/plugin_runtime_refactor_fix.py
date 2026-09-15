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
write(path, text)

# Fail immediately if the two legacy AppState access paths ever reappear during this migration.
for source in (ROOT / "src-tauri/src").rglob("*.rs"):
    content = source.read_text(encoding="utf-8")
    if ".plugin_host" in content:
        raise RuntimeError(f"legacy PluginHost access remains in {source.relative_to(ROOT)}")
    if ".modbus_adapter" in content:
        raise RuntimeError(f"legacy Modbus adapter AppState access remains in {source.relative_to(ROOT)}")

print("plugin runtime follow-up fixes applied")
