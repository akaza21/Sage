use super::*;
use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId},
    usage::UsageState,
};
use serde::Deserialize;

/// A read needs no projection revision: usage is harness state, not Chat state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AcpUsageRequest {
    pub(super) project_id: SageProjectId,
    pub(super) project_generation: ProjectGeneration,
    pub(super) chat_id: SageChatId,
    pub(super) session_id: SageAgentSessionId,
    pub(super) runtime_epoch: RuntimeEpoch,
}

#[tauri::command]
pub(crate) async fn get_acp_usage(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: AcpUsageRequest,
) -> Result<UsageState, AcpIpcError> {
    let Some(owner) = current_owner(&state, request).await? else {
        return Ok(UsageState::Offline);
    };
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<AppState>().acp.read_usage(owner)
    })
    .await
    .map_err(|_| internal_error())
}

pub(super) async fn current_owner(
    state: &AppState,
    request: AcpUsageRequest,
) -> Result<Option<RuntimeOwner>, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    Ok((snapshot.session_id.as_ref() == Some(&request.session_id)
        && snapshot.runtime_epoch == Some(request.runtime_epoch))
    .then_some(RuntimeOwner {
        project_id: request.project_id,
        chat_id: request.chat_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::acp::goal::integration_tests::fixture;

    #[tokio::test]
    async fn only_the_current_runtime_is_read_and_one_without_usage_is_unsupported() {
        let (state, goal, calls) = fixture(Ok(()));
        let request = |session_id: &SageAgentSessionId, epoch| AcpUsageRequest {
            project_id: goal.project_id.clone(),
            project_generation: goal.project_generation,
            chat_id: goal.chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        };
        let foreign = SageAgentSessionId::parse("foreign-session").unwrap();
        for stale in [request(&goal.session_id, 2), request(&foreign, 1)] {
            assert_eq!(current_owner(&state, stale).await.unwrap(), None);
        }
        let owner = current_owner(&state, request(&goal.session_id, 1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.acp.read_usage(owner.clone()), UsageState::Unsupported);
        let retired = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner
        };
        assert_eq!(state.acp.read_usage(retired), UsageState::Offline);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        state.acp.shutdown_blocking().unwrap();
    }
}
