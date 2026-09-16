//! Telnet application-layer Session connector.

use tauri::{AppHandle, Manager, State};

use crate::kernel::plugin_adapter::ProtocolAdapter;
use crate::plugin_application::{
    connect_simple_terminal_session, ConnectSessionRequest, SessionConnectFuture,
};
use crate::AppState;

pub(crate) fn session_connector(
    app: AppHandle,
    request: ConnectSessionRequest,
) -> SessionConnectFuture {
    Box::pin(async move {
        let state: State<'_, AppState> = app.state();
        connect_session(app.clone(), state, request).await
    })
}

/// Telnet 会话连接（TelnetAdapter → Channel → SessionStore）
///
/// 单连接/标签页模式（serial 式 Sync I/O），无文件传输、无容器/子会话。
/// 回显状态事件由通道内回调直接 emit（适配器持有 AppHandle，session_id
/// 经 `Channel::on_session_started` 注入），无需 relay 线程。
async fn connect_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ConnectSessionRequest,
) -> Result<String, String> {
    let conn = state
        .plugin::<crate::plugins::telnet::TelnetAdapter>(crate::plugins::telnet::PLUGIN_ID)
        .connect(&request.endpoint, &request.params)
        .await
        .map_err(|e| e.to_string())?;

    connect_simple_terminal_session(app, &state, request, "telnet", "Telnet", true, conn)
}
