use super::*;
use crate::acp::{
    intent::TransitionAcpSessionRequest, store::session_replace::ReplaceSessionCommand,
};

#[tauri::command]
pub async fn acp_transition_session(
    state: State<'_, AppState>,
    request: TransitionAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    transition_acp_session_inner(&state, request).await
}

pub(crate) async fn transition_acp_session_inner(
    state: &AppState,
    request: TransitionAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (context, _snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&request.binding_id) {
        return characterization::transition(state, request);
    }
    let admitted = state
        .acp
        .resolve_session_binding(&request.binding_id)
        .map_err(|_| binding_unavailable())?;
    let root = admitted
        .project_root(&state.database, &context)
        .map_err(|_| binding_unavailable())?;
    let project = request.project_id.clone();
    let chat = request.chat_id.clone();
    state
        .acp
        .transition_session(
            admitted,
            &root,
            ReplaceSessionCommand {
                start: StartSessionCommand {
                    project_id: request.project_id,
                    chat_id: request.chat_id,
                    expected_projection_revision: request.expected_projection_revision,
                    client_intent_id: request.client_intent_id,
                    binding_id: request.binding_id,
                    binding_digest: String::new(),
                    harness_version: None,
                    model_id: request.model_id,
                    now_millis: crate::persistence::now_millis(),
                },
                expected_head: request.expected_head,
                confirm_unknown_delivery: request.confirm_unknown_delivery,
            },
        )
        .map_err(store_error)?;
    mutation_response(state, project, chat)
}
