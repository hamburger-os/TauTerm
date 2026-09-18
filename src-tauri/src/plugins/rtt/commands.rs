use super::backend;
use super::error::{RttCommandError, RttError, RttErrorCode};
use super::model::{RttChannelInfo, RttHistoryResponse, RttProbeInfo, RttSnapshot};
use super::{RttPlugin, PLUGIN_ID};
use crate::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::sync::Arc;
use tauri::State;

const MAX_WRITE_BYTES: usize = 64 * 1024;

fn runtime(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<Arc<super::runtime::RttRuntime>, RttCommandError> {
    state
        .plugin::<RttPlugin>(PLUGIN_ID)
        .runtime(session_id)
        .ok_or_else(|| {
            RttCommandError::from(RttError::new(
                RttErrorCode::Cancelled,
                "RTT 会话未连接或运行时已释放",
            ))
        })
}

#[tauri::command]
pub async fn rtt_discover_probes() -> Result<Vec<RttProbeInfo>, RttCommandError> {
    tokio::task::spawn_blocking(backend::list_probes)
        .await
        .map_err(|error| {
            RttCommandError::from(RttError::backend(format!("枚举调试探针失败: {error}")))
        })
}

#[tauri::command]
pub fn rtt_snapshot(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<RttSnapshot, RttCommandError> {
    Ok(runtime(&state, &session_id)?.snapshot())
}

#[tauri::command]
pub fn rtt_history(
    state: State<'_, AppState>,
    session_id: String,
    channel_index: u32,
    after_sequence: Option<u64>,
) -> Result<RttHistoryResponse, RttCommandError> {
    Ok(runtime(&state, &session_id)?.history(channel_index, after_sequence))
}

#[tauri::command]
pub async fn rtt_write(
    state: State<'_, AppState>,
    session_id: String,
    channel_index: u32,
    data_b64: String,
) -> Result<usize, RttCommandError> {
    let data = BASE64.decode(data_b64).map_err(|error| {
        RttCommandError::from(RttError::invalid_config(format!(
            "RTT 写入数据不是有效 Base64: {error}"
        )))
    })?;
    if data.len() > MAX_WRITE_BYTES {
        return Err(RttCommandError::from(RttError::invalid_config(format!(
            "单次 RTT 写入不能超过 {MAX_WRITE_BYTES} bytes"
        ))));
    }
    let runtime = runtime(&state, &session_id)?;
    tokio::task::spawn_blocking(move || runtime.write(channel_index, data))
        .await
        .map_err(|error| RttCommandError::from(RttError::backend(error.to_string())))?
        .map_err(RttCommandError::from)
}

#[tauri::command]
pub async fn rtt_refresh_channels(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<RttChannelInfo>, RttCommandError> {
    let runtime = runtime(&state, &session_id)?;
    tokio::task::spawn_blocking(move || runtime.refresh_channels())
        .await
        .map_err(|error| RttCommandError::from(RttError::backend(error.to_string())))?
        .map_err(RttCommandError::from)
}

#[tauri::command]
pub fn rtt_set_automation_source_channel(
    state: State<'_, AppState>,
    session_id: String,
    channel_index: u32,
) -> Result<(), RttCommandError> {
    runtime(&state, &session_id)?
        .set_automation_source_channel(channel_index)
        .map_err(RttCommandError::from)
}

#[tauri::command]
pub fn rtt_set_send_channel(
    state: State<'_, AppState>,
    session_id: String,
    channel_index: u32,
) -> Result<(), RttCommandError> {
    runtime(&state, &session_id)?
        .set_send_channel(channel_index)
        .map_err(RttCommandError::from)
}
