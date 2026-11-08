use super::*;
use crate::acp::attached_context::effective_prompt;

// The over-budget message names the budget as static text.
const _: () = assert!(
    crate::acp::attached_context::files::DISPATCH_BYTES == 120 * 1024,
    "update the dispatch budget in the prompt-too-large message"
);

mod lifecycle;
pub(crate) use lifecycle::{close_acp_session_inner, end_acp_session_inner};

/// Context a prompt cannot carry yet: images, and page elements that
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
    let (transition, snapshot) =
        snapshot_for(state, project_id.clone(), generation, chat_id.clone())
            .await
            .map_err(prompt_not_dispatched)?;
    validate_send(state, &request, &snapshot).map_err(prompt_not_dispatched)?;
    // A batch staged in a tab that has since closed must never be sent.
    crate::acp::attached_context::files::release_detached(&state.acp.file_stages, &state.database);
    if matches!(request, SendAcpTurnRequest::NewSession { .. })
        && !state
            .acp
            .file_stages
            .lock()
            .map_err(|_| prompt_not_dispatched(internal_error()))?
            .get(project_id.as_str(), chat_id.as_str())
            .0
            .is_empty()
    {
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message(
                "Start a harness session before sending staged file attachments.",
            ),
        }));
    }
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
    if let SendAcpTurnRequest::Session {
        session_id, staged, ..
    } = &request
    {
        let binding_id = session_routes::durable_binding_id(state, session_id)
            .map_err(store_error)
            .map_err(prompt_not_dispatched)?;
        if state.acp.accepts_characterization(&binding_id) {
            if !state
                .acp
                .file_stages
                .lock()
                .map_err(|_| prompt_not_dispatched(internal_error()))?
                .selected(project_id.as_str(), chat_id.as_str(), staged.as_ref())
                .is_ok_and(|files| files.0.is_empty())
            {
                return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
                    message: safe_message("This harness cannot accept staged file attachments."),
                }));
            }
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
        staged,
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
            staged,
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
            staged,
        ),
    };
    let binding_id = session_routes::durable_binding_id(state, &session_id)
        .map_err(store_error)
        .map_err(prompt_not_dispatched)?;
    let mut prompt = effective_prompt(&prompt, &attached_context)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        })
        .map_err(prompt_not_dispatched)?;
    let files = state
        .acp
        .file_stages
        .lock()
        .map_err(|_| prompt_not_dispatched(internal_error()))?
        .selected(project_id.as_str(), chat_id.as_str(), staged.as_ref())
        .map_err(|message| {
            prompt_not_dispatched(AcpIpcError::InvalidRequest {
                message: safe_message(message),
            })
        })?;
    if !files.0.is_empty() {
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .map_err(store_error)
            .map_err(prompt_not_dispatched)?;
        if !page
            .sessions
            .as_slice()
            .iter()
            .any(|s| s.id == session_id && s.capabilities.prompt_resources)
        {
            return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
                message: safe_message(
                    "This harness does not advertise embedded-resource prompt support. Remove the file attachments or choose another harness.",
                ),
            }));
        }
    }
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
    super::turn_workspace::mark_before_prompt(state, transition, &client_intent_id, &session_id)
        .await;
    // Capture can outlive a project switch. Reacquire authority before dispatch.
    let _transition = match lock_acp_context(state, &project_id, project_generation).await {
        Ok(transition) => transition,
        Err(error) => {
            super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
            return Err(prompt_not_dispatched(error));
        }
    };
    let current_files = state
        .acp
        .file_stages
        .lock()
        .map_err(|_| prompt_not_dispatched(internal_error()))?
        .selected(project_id.as_str(), chat_id.as_str(), staged.as_ref());
    if !current_files.is_ok_and(|current| files.same_selection(&current)) {
        super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message("Attachments changed while preparing the prompt; send again."),
        }));
    }
    if !files.0.is_empty()
        && prompt.retained_bytes() + files.retained_bytes()
            > crate::acp::attached_context::files::DISPATCH_BYTES
    {
        super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message(
                "Prompt text and file attachments exceed the 120 KiB native dispatch budget. Shorten the prompt or remove attachments.",
            ),
        }));
    }
    prompt.resources = files.clone();
    let tracked_session = session_id.clone();
    let tracked_intent = client_intent_id.clone();
    let dispatch = state
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
        });
    if match &dispatch {
        Ok(_) => true,
        Err(failure) => failure.prompt_dispatch != PromptDispatch::NotStarted,
    } {
        state
            .acp
            .file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .consume(project_id.as_str(), chat_id.as_str(), &files);
    }
    if dispatch.is_err() {
        super::turn_workspace::forget_unprepared(state, &tracked_session, &tracked_intent);
    }
    dispatch?;
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
