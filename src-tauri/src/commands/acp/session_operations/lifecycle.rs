use super::*;

// Session close and native end, kept beside session_operations.rs so each
// command module stays focused.

pub(crate) async fn close_acp_session_inner(
    state: &AppState,
    request: CloseAcpSessionRequest,
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
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    let binding_id =
        session_routes::durable_binding_id(state, &request.session_id).map_err(store_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&binding_id) {
        return characterization::close(state, request);
    }
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .dispatch_session_close(
            &binding_id,
            CloseSessionCommand {
                project_id: request.project_id,
                chat_id: request.chat_id,
                expected_projection_revision: request.expected_projection_revision,
                client_intent_id: request.client_intent_id,
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .map_err(dispatch_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(crate) async fn end_acp_session_inner(
    state: &AppState,
    request: CloseAcpSessionRequest,
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
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    #[cfg(all(windows, feature = "acp-fixture"))]
    {
        let binding_id =
            session_routes::durable_binding_id(state, &request.session_id).map_err(store_error)?;
        if state.acp.accepts_characterization(&binding_id) {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message(
                    "Characterization sessions cannot be ended by native recovery.",
                ),
            });
        }
    }
    let page = state
        .acp
        .load_chat_page(request.project_id.clone(), request.chat_id.clone(), None)
        .map_err(store_error)?;
    let session = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| session.id == request.session_id)
        .ok_or_else(|| intent_error(IntentError::ForeignSession))?;
    if session.capabilities.close_session {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message(
                "Native session recovery is available only when close is unproven.",
            ),
        });
    }
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .end_session_natively(RuntimeOwner {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            runtime_epoch: request.runtime_epoch,
        })
        .map_err(|error| {
            dispatch_error(crate::acp::service::SessionDispatchError::Operation(error))
        })?;
    mutation_response(state, project_id, chat_id)
}
