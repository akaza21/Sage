use super::*;

pub(crate) async fn start_acp_session_inner(
    state: &AppState,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (transition, snapshot) = snapshot_for(
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
    refuse_archived_chat(state, &request.project_id, &request.chat_id)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&request.binding_id) {
        return characterization::start(state, request).await;
    }
    let admitted = state
        .acp
        .resolve_session_binding(&request.binding_id)
        .map_err(|_| binding_unavailable())?;
    let project_root = admitted
        .project_root(&state.database, &transition)
        .map_err(|_| root_unavailable())?;
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .start_session(
            admitted,
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
        .map_err(session_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(crate) async fn activate_acp_session_inner(
    state: &AppState,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_session_intent(&request, &snapshot).map_err(intent_error)?;
    refuse_archived_chat(state, &request.project_id, &request.chat_id)?;
    let binding_id = durable_binding_id(state, &request.session_id).map_err(store_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&binding_id) {
        let project_root = state
            .database
            .selected_project_root_authority(&transition)
            .map_err(|_| AcpIpcError::Unavailable {
                reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message(
                    "The selected project root is unavailable or changed; reselect it and retry.",
                ),
            })?;
        return characterization::activate(state, request, project_root).await;
    }
    let admitted = state
        .acp
        .resolve_session_binding(&binding_id)
        .map_err(|_| binding_unavailable())?;
    let project_root = admitted
        .project_root(&state.database, &transition)
        .map_err(|_| binding_unavailable())?;
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .activate_session(
            admitted,
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
        .map_err(session_error)?;
    mutation_response(state, project_id, chat_id)
}

fn root_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
        message: safe_message("Codex beta requires one unchanged Project root."),
    }
}

pub(super) fn durable_binding_id(
    state: &AppState,
    session_id: &crate::acp::contract::SageAgentSessionId,
) -> Result<crate::acp::intent::BindingId, crate::acp::store::StoreError> {
    let binding_id: String = state
        .database
        .connection()
        .query_row(
            "SELECT binding_id FROM agent_sessions WHERE id=?1",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .map_err(crate::acp::store::StoreError::Database)?;
    crate::acp::intent::BindingId::new("bindingId", binding_id)
        .map_err(|_| crate::acp::store::StoreError::Corrupt("session binding identity"))
}

/// An archived Chat runs no agent. Prompt, session start and activation
/// refuse it here with a typed message; the store refuses it again inside
/// the transaction that would add the Turn or runtime epoch.
pub(super) fn refuse_archived_chat(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> Result<(), AcpIpcError> {
    match crate::chat_archive::is_archived_chat(
        &state.database.connection(),
        project_id.as_str(),
        chat_id.as_str(),
    ) {
        Ok(false) => Ok(()),
        Ok(true) => Err(archived_chat()),
        Err(_) => Err(internal_error()),
    }
}
