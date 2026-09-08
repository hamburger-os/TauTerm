//! Narrow user-mediated file commands.
//!
//! The WebView never receives a generic filesystem capability or an arbitrary selected path.
//! Rust opens the native picker itself and only reads/writes the specific Command Set payload.

use crate::kernel::persistence::atomic_write;
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

const MAX_COMMAND_SET_BYTES: usize = 4 * 1024 * 1024;

fn validate_command_set_json(content: &str) -> Result<(), String> {
    if content.len() > MAX_COMMAND_SET_BYTES {
        return Err("Command Set file exceeds the 4 MiB limit".to_string());
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("invalid Command Set JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "Command Set root must be a JSON object".to_string())?;

    if object.get("version").and_then(Value::as_u64).is_none() {
        return Err("Command Set version is missing or invalid".to_string());
    }
    if object.get("name").and_then(Value::as_str).is_none() {
        return Err("Command Set name is missing or invalid".to_string());
    }
    if object.get("commands").and_then(Value::as_array).is_none() {
        return Err("Command Set commands must be an array".to_string());
    }

    Ok(())
}

fn safe_json_file_name(suggested_name: &str) -> String {
    let mut sanitized = suggested_name
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();

    sanitized = sanitized
        .trim()
        .trim_matches('.')
        .trim_end_matches(' ')
        .chars()
        .take(120)
        .collect();

    if sanitized.is_empty() {
        sanitized = "commands.json".to_string();
    } else if !sanitized.to_ascii_lowercase().ends_with(".json") {
        sanitized.push_str(".json");
    }

    sanitized
}

#[tauri::command]
pub async fn import_command_set_file(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };

        let path = selected
            .into_path()
            .map_err(|error| format!("selected file is not a local path: {error}"))?;
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("read Command Set metadata failed: {error}"))?;
        if metadata.len() > MAX_COMMAND_SET_BYTES as u64 {
            return Err("Command Set file exceeds the 4 MiB limit".to_string());
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("read Command Set failed: {error}"))?;
        validate_command_set_json(&content)?;
        Ok(Some(content))
    })
    .await
    .map_err(|error| format!("Command Set import worker failed: {error}"))?
}

#[tauri::command]
pub async fn export_command_set_file(
    app: AppHandle,
    suggested_name: String,
    content: String,
) -> Result<bool, String> {
    validate_command_set_json(&content)?;
    let file_name = safe_json_file_name(&suggested_name);

    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("JSON", &["json"])
            .set_file_name(file_name)
            .blocking_save_file()
        else {
            return Ok(false);
        };

        let path = selected
            .into_path()
            .map_err(|error| format!("selected file is not a local path: {error}"))?;
        atomic_write(&path, content.as_bytes())
            .map_err(|error| format!("write Command Set failed: {error}"))?;
        Ok(true)
    })
    .await
    .map_err(|error| format!("Command Set export worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_set_validation_rejects_non_command_json() {
        assert!(validate_command_set_json(r#"{"version":1,"name":"demo","commands":[]}"#).is_ok());
        assert!(validate_command_set_json(r#"{"version":1,"name":"demo"}"#).is_err());
        assert!(validate_command_set_json(r#"["not","a","command","set"]"#).is_err());
    }

    #[test]
    fn suggested_file_name_is_sanitized_and_json_suffixed() {
        assert_eq!(safe_json_file_name("demo"), "demo.json");
        assert_eq!(safe_json_file_name("../bad:name.json"), "_bad_name.json");
        assert_eq!(safe_json_file_name("   "), "commands.json");
    }
}
