use super::*;

pub(crate) async fn send_acp_turn_inner(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (project_id, generation, chat_id) = send_owner(&request);
    let (_transition, snapshot) =
        snapshot_for(state, project_id.clone(), generation, chat_id.clone()).await?;
    validate_send(&request, &snapshot)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if let SendAcpTurnRequest::Session { session_id, .. } = &request {
        let binding_id =
            session_routes::durable_binding_id(state, session_id).map_err(store_error)?;
        if state.acp.accepts_characterization(&binding_id) {
            return characterization::send(state, request);
        }
    }
    let (
        project_id,
        project_generation,
        chat_id,
        expected_projection_revision,
        client_intent_id,
        session_id,
        runtime_epoch,
        prompt,
    ) = match request {
        SendAcpTurnRequest::NewSession {
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            client_intent_id,
            binding_id,
            model_id,
            prompt,
        } => {
            #[cfg(all(windows, feature = "acp-fixture"))]
            if state.acp.accepts_characterization_new_session(&binding_id) {
                return characterization::send(
                    state,
                    SendAcpTurnRequest::NewSession {
                        project_id,
                        project_generation,
                        chat_id,
                        expected_projection_revision,
                        client_intent_id,
                        binding_id,
                        model_id,
                        prompt,
                    },
                );
            }
            return Err(binding_unavailable());
        }
        SendAcpTurnRequest::Session {
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            client_intent_id,
            session_id,
            runtime_epoch,
            prompt,
        } => (
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            client_intent_id,
            session_id,
            runtime_epoch,
            prompt,
        ),
    };
    let binding_id = session_routes::durable_binding_id(state, &session_id).map_err(store_error)?;
    state
        .acp
        .dispatch_session_turn(
            &binding_id,
            PrepareTurnCommand {
                project_id: project_id.clone(),
                project_generation,
                chat_id: chat_id.clone(),
                session_id,
                runtime_epoch,
                expected_projection_revision,
                client_intent_id,
                prompt,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .map_err(dispatch_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(crate) async fn cancel_acp_turn_inner(
    state: &AppState,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_cancel_intent(&request, &snapshot).map_err(intent_error)?;
    let binding_id =
        session_routes::durable_binding_id(state, &request.session_id).map_err(store_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&binding_id) {
        return characterization::cancel(state, request);
    }
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .dispatch_session_cancel(
            &binding_id,
            HostCancelIntent {
                owner: RuntimeOwner {
                    project_id: request.project_id,
                    chat_id: request.chat_id,
                    session_id: request.session_id,
                    runtime_epoch: request.runtime_epoch,
                },
                turn_id: request.turn_id,
                client_intent_id: request.client_intent_id,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .map_err(dispatch_error)?;
    mutation_response(state, project_id, chat_id)
}

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
