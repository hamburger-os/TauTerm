//! Configuration and theme Tauri commands.

use crate::AppState;
use serde_json::Value;
use tauri::State;

// ── ConfigStore 命令 ────────────────────────────────

#[tauri::command]
pub fn get_config(state: State<'_, AppState>, key: String) -> Result<Option<Value>, String> {
    if !state.config_store.persistence_ready() {
        return Err("ConfigStore persistence is unavailable".to_string());
    }
    Ok(state.config_store.get::<Value>(&key))
}

#[tauri::command]
pub async fn set_config(
    state: State<'_, AppState>,
    key: String,
    value: Value,
) -> Result<(), String> {
    state
        .config_store
        .set(&key, &value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_config(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state.config_store.delete(&key).map_err(|e| e.to_string())
}

// ── ThemeEngine 命令 ────────────────────────────────

#[tauri::command]
pub fn get_theme_list(state: State<'_, AppState>) -> Vec<String> {
    state.theme_engine.theme_names()
}

#[tauri::command]
pub fn get_active_theme(state: State<'_, AppState>) -> String {
    state.theme_engine.active_name()
}

#[tauri::command]
pub fn set_theme(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .theme_engine
        .apply_theme(&name)
        .map_err(|e| e.to_string())
}
