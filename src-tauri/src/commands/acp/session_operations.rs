use super::*;
use crate::acp::attached_context::effective_prompt;

/// Context a prompt cannot carry: images (until CP-26) and page elements that
/// fail admission.
fn image_input_block(contexts: &AttachedContexts) -> Result<(), AcpIpcError> {
    if contexts.as_slice().iter().any(|context| {
        matches!(
            context,
            crate::acp::attached_context::AttachedContext::Image(_)
        )
    }) {
        return Err(AcpIpcError::Unavailable {
            reason: crate::acp::intent::AcpUnavailableReason::ImageInputUnsupported,
            message: safe_message("This harness cannot accept image attachments yet."),
        });
    }
    crate::acp::attached_context::admit_browser_elements(contexts).map_err(|refusal| {
        AcpIpcError::InvalidRequest {
            message: safe_message(refusal.message()),
        }
    })
}

pub(crate) async fn send_acp_turn_inner(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let (project_id, generation, chat_id) = send_owner(&request);
    let (_transition, snapshot) =
        snapshot_for(state, project_id.clone(), generation, chat_id.clone())
            .await
            .map_err(prompt_not_dispatched)?;
    validate_send(state, &request, &snapshot).map_err(prompt_not_dispatched)?;
    match &request {
        SendAcpTurnRequest::Session {
            attached_context, ..
        }
        | SendAcpTurnRequest::NewSession {
            attached_context, ..
        } => {
            image_input_block(attached_context).map_err(prompt_not_dispatched)?;
        }
    }
    #[cfg(all(windows, feature = "acp-fixture"))]
    if let SendAcpTurnRequest::Session { session_id, .. } = &request {
        let binding_id = session_routes::durable_binding_id(state, session_id)
            .map_err(store_error)
            .map_err(prompt_not_dispatched)?;
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
        attached_context,
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
            attached_context,
        } => {
            #[cfg(all(windows, feature = "acp-fixture"))]
            {
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
                            attached_context,
                        },
                    );
                }
            }
            #[cfg(not(all(windows, feature = "acp-fixture")))]
            let _ = (
                project_id,
                project_generation,
                chat_id,
                expected_projection_revision,
                client_intent_id,
                binding_id,
                model_id,
                prompt,
                attached_context,
            );
            return Err(prompt_not_dispatched(binding_unavailable()));
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
            attached_context,
        } => (
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            client_intent_id,
            session_id,
            runtime_epoch,
            prompt,
            attached_context,
        ),
    };
    let binding_id = session_routes::durable_binding_id(state, &session_id)
        .map_err(store_error)
        .map_err(prompt_not_dispatched)?;
    let prompt = effective_prompt(&prompt, &attached_context)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        })
        .map_err(prompt_not_dispatched)?;
    let requested_access = state.settings.session_access_modes().map_err(|_| {
        prompt_not_dispatched(AcpIpcError::RecoveryRequired {
            message: safe_message("Session access mode could not be loaded; reload Sage."),
        })
    })?;
    let access_mode = crate::acp::bindings::access_policy::effective_for_binding(
        state.acp.binding_registry(),
        binding_id.as_str(),
        requested_access.get(binding_id.as_str()).copied(),
    )
    .ok_or_else(|| {
        prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message("Choose a supported access mode before sending."),
        })
    })?;
    state
        .acp
        .dispatch_session_turn(
            &binding_id,
            crate::acp::runtime::HostPromptIntent {
                command: PrepareTurnCommand {
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
                access_mode,
            },
        )
        .map_err(|failure| SendAcpTurnFailure {
            failure: dispatch_error(failure.error),
            prompt_dispatch: failure.prompt_dispatch,
        })?;
    mutation_response(state, project_id, chat_id).map_err(prompt_dispatch_unknown)
}

pub(super) fn prompt_not_dispatched(failure: AcpIpcError) -> SendAcpTurnFailure {
    SendAcpTurnFailure {
        failure,
        prompt_dispatch: PromptDispatch::NotStarted,
    }
}

pub(super) fn prompt_dispatch_unknown(failure: AcpIpcError) -> SendAcpTurnFailure {
    SendAcpTurnFailure {
        failure,
        prompt_dispatch: PromptDispatch::Unknown,
    }
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

pub(crate) async fn steer_acp_turn_inner(
    state: &AppState,
    request: SteerAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_steer_intent(&request, &snapshot).map_err(intent_error)?;
    image_input_block(&request.attached_context)?;
    let binding_id =
        session_routes::durable_binding_id(state, &request.session_id).map_err(store_error)?;
    let prompt = effective_prompt(&request.prompt, &request.attached_context).map_err(|_| {
        AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        }
    })?;
    let client_message_id = crate::acp::contract::RequiredText::new(
        "clientMessageId",
        request.client_intent_id.as_str().to_owned(),
    )
    .map_err(|_| AcpIpcError::InvalidRequest {
        message: safe_message("The Steer intent identifier is invalid."),
    })?;
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .dispatch_session_steer(
            &binding_id,
            HostSteerIntent {
                owner: RuntimeOwner {
                    project_id: request.project_id,
                    chat_id: request.chat_id,
                    session_id: request.session_id,
                    runtime_epoch: request.runtime_epoch,
                },
                turn_id: request.turn_id,
                prompt,
                client_message_id,
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
