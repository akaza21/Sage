//! `/mcp` for the chat's running session: list its MCP servers, reload
//! their configuration, and start a server's sign-in in the system browser.

use super::{
    session_control::control_error,
    usage::{AcpUsageRequest, current_owner},
    *,
};
use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::AcpUnavailableReason,
    mcp::inventory::McpInventory,
    service::SessionControlError,
};
use serde::Deserialize;

/// Names a server of one live runtime.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpSignInRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
    name: RequiredText<128>,
}

#[tauri::command]
pub(crate) async fn read_acp_mcp_servers(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: AcpUsageRequest,
) -> Result<McpInventory, AcpIpcError> {
    let owner = live_owner(&state, request).await?;
    blocking(app, move |state| state.acp.read_mcp_servers(owner)).await
}

#[tauri::command]
pub(crate) async fn reload_acp_mcp_servers(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: AcpUsageRequest,
) -> Result<McpInventory, AcpIpcError> {
    let owner = live_owner(&state, request).await?;
    blocking(app, move |state| state.acp.reload_mcp_servers(owner)).await
}

/// Opens the server's sign-in page in the user's browser. The page itself
/// never reaches the WebView; the list shows the result once the harness
/// reconnects the server.
#[tauri::command]
pub(crate) async fn sign_in_acp_mcp_server(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: McpSignInRequest,
) -> Result<(), AcpIpcError> {
    let McpSignInRequest {
        project_id,
        project_generation,
        chat_id,
        session_id,
        runtime_epoch,
        name,
    } = request;
    let runtime = AcpUsageRequest {
        project_id,
        project_generation,
        chat_id,
        session_id,
        runtime_epoch,
    };
    // Codex starts a login only when Sage can open its page.
    if !crate::system_browser::available() {
        return Err(no_browser());
    }
    let owner = live_owner(&state, runtime).await?;
    let url = blocking(app, move |state| state.acp.sign_in_mcp_server(owner, name)).await?;
    crate::system_browser::open_https(url.as_str()).map_err(|_| no_browser())
}

fn no_browser() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::CapabilityUnavailable,
        message: safe_message("Sage could not open your browser to sign in."),
    }
}

async fn live_owner(
    state: &AppState,
    request: AcpUsageRequest,
) -> Result<RuntimeOwner, AcpIpcError> {
    current_owner(state, request)
        .await?
        .ok_or_else(|| control_error(SessionControlError::Unavailable))
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    run: impl FnOnce(&AppState) -> Result<T, SessionControlError> + Send + 'static,
) -> Result<T, AcpIpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        run(&app.state::<AppState>())
    })
    .await
    .map_err(|_| internal_error())?
    .map_err(control_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_sign_in_names_one_live_runtime_and_a_bounded_server_name() {
        let request = json!({
            "projectId":"project-one", "projectGeneration":"1", "chatId":"chat-one",
            "sessionId":"session-one", "runtimeEpoch":"1", "name":"notion"
        });
        assert!(serde_json::from_value::<McpSignInRequest>(request.clone()).is_ok());
        for (key, value) in [
            ("name", json!("")),
            ("name", json!("x".repeat(129))),
            ("url", json!("https://x")),
        ] {
            let mut changed = request.clone();
            changed[key] = value;
            assert!(
                serde_json::from_value::<McpSignInRequest>(changed).is_err(),
                "{key}"
            );
        }
    }
}
