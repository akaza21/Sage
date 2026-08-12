use super::*;

pub(crate) async fn start_acp_session_inner(
    state: &AppState,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        request.expected_projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    if request.binding_id.as_str() == "codex-app-server" {
        let project_root = state
            .database
            .codex_project_root_authority(&_transition)
            .map_err(|_| AcpIpcError::Unavailable {
                reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message("Codex beta requires one unchanged Project root."),
            })?;
        let project_id = request.project_id.clone();
        let chat_id = request.chat_id.clone();
        state
            .acp
            .start_codex_session(
                &project_root,
                StartSessionCommand {
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
            )
            .await
            .map_err(codex_session_error)?;
        return mutation_response(state, project_id, chat_id);
    }
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&request.binding_id) {
        return characterization::start(state, request).await;
    }
    Err(binding_unavailable())
}

pub(crate) async fn activate_acp_session_inner(
    state: &AppState,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_session_intent(&request, &snapshot).map_err(intent_error)?;
    let codex_root = state
        .database
        .codex_project_root_authority(&_transition)
        .ok();
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.characterization_enabled() {
        let project_root = state
            .database
            .selected_project_root_authority(&_transition)
            .map_err(|_| AcpIpcError::Unavailable {
                reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message(
                    "The selected project root is unavailable or changed; reselect it and retry.",
                ),
            })?;
        return characterization::activate(state, request, project_root).await;
    }
    if let Some(project_root) = codex_root {
        let project_id = request.project_id.clone();
        let chat_id = request.chat_id.clone();
        state
            .acp
            .activate_codex_session(
                ActivateSessionCommand {
                    project_id: request.project_id,
                    chat_id: request.chat_id,
                    expected_projection_revision: request.expected_projection_revision,
                    client_intent_id: request.client_intent_id,
                    session_id: request.session_id,
                    runtime_epoch: request.runtime_epoch,
                    artifact_digest: String::new(),
                    now_millis: crate::persistence::now_millis(),
                },
                project_root,
            )
            .await
            .map_err(codex_session_error)?;
        return mutation_response(state, project_id, chat_id);
    }
    Err(binding_unavailable())
}
