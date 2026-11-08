use super::*;
use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId},
    usage::UsageState,
};
use serde::Deserialize;

/// Names one live runtime. A read needs no projection revision: usage is
/// harness state, not Chat state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AcpUsageRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
}

#[tauri::command]
pub(crate) async fn get_acp_usage(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: AcpUsageRequest,
) -> Result<UsageState, AcpIpcError> {
    let (transition, snapshot) = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    if snapshot.session_id.as_ref() != Some(&request.session_id)
        || snapshot.runtime_epoch != Some(request.runtime_epoch)
    {
        return Ok(UsageState::Offline);
    }
    drop(transition);
    let owner = RuntimeOwner {
        project_id: request.project_id,
        chat_id: request.chat_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
    };
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<AppState>().acp.read_usage(owner)
    })
    .await
    .map_err(|_| internal_error())
}
