use super::*;
use crate::acp::attached_context::effective_prompt;

mod lifecycle;
mod prompt_images;
mod staged;
pub(crate) use lifecycle::{close_acp_session_inner, end_acp_session_inner};
use prompt_images::image_input_block;

pub(crate) async fn send_acp_turn_inner(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let (project_id, generation, chat_id) = send_owner(&request);
    let (transition, snapshot) =
        snapshot_for(state, project_id.clone(), generation, chat_id.clone())
            .await
            .map_err(prompt_not_dispatched)?;
    session_routes::refuse_archived_chat(state, project_id, chat_id)
        .map_err(prompt_not_dispatched)?;
    validate_send(state, &request, &snapshot).map_err(prompt_not_dispatched)?;
    // A batch staged in a tab that has since closed must never be sent.
    crate::acp::attached_context::files::release_detached(&state.acp.file_stages, &state.database);
    match &request {
        SendAcpTurnRequest::Session {
            attached_context, ..
        } => prompt_images::admit_elements(attached_context),
        SendAcpTurnRequest::NewSession {
            attached_context, ..
        } => image_input_block(attached_context),
    }
    .map_err(prompt_not_dispatched)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if let SendAcpTurnRequest::Session {
        session_id,
        attached_context,
        staged,
        prompt,
        ..
    } = &request
    {
        let binding_id = session_routes::durable_binding_id(state, session_id)
            .map_err(store_error)
            .map_err(prompt_not_dispatched)?;
        if state.acp.accepts_characterization(&binding_id) {
            image_input_block(attached_context).map_err(prompt_not_dispatched)?;
            let selection = staged::resolve(state, &project_id, &chat_id, staged.clone(), prompt)
                .map_err(prompt_not_dispatched)?;
            staged::characterization_carries_none(state, &project_id, &chat_id, &selection)
                .map_err(prompt_not_dispatched)?;
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
    // Only the person's own text names files, never attached context.
    let mentioned = mentions::tokens(&prompt);
    let binding_id = session_routes::durable_binding_id(state, &session_id)
        .map_err(store_error)
        .map_err(prompt_not_dispatched)?;
    let staged = staged::resolve(state, &project_id, &chat_id, staged, &prompt)
        .map_err(prompt_not_dispatched)?;
    let mut prompt = effective_prompt(&prompt, &attached_context)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        })
        .map_err(prompt_not_dispatched)?;
    let sent_images = prompt_images::attach(
        state,
        &mut prompt,
        &attached_context,
        &project_id,
        &chat_id,
        &session_id,
    )
    .await
    .map_err(prompt_not_dispatched)?;
    let files = state
        .acp
        .file_stages
        .lock()
        .map_err(|_| prompt_not_dispatched(internal_error()))?
        .selected(project_id.as_str(), chat_id.as_str(), &staged)
        .map_err(|message| {
            prompt_not_dispatched(AcpIpcError::InvalidRequest {
                message: safe_message(message),
            })
        })?;
    if !files.0.is_empty() || prompt.action.is_some() {
        let capabilities =
            session_routes::session_capabilities(state, &project_id, &chat_id, &session_id)
                .map_err(store_error)
                .map_err(prompt_not_dispatched)?;
        if !files.0.is_empty() && !capabilities.as_ref().is_some_and(|c| c.prompt_resources) {
            return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
                message: safe_message(
                    "This harness does not advertise embedded-resource prompt support. Remove the file attachments or choose another harness.",
                ),
            }));
        }
        if !prompt.action_admitted(capabilities.as_ref()) {
            return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
                message: safe_message("This session did not negotiate that action."),
            }));
        }
    }
    super::turn_workspace::mark_before_prompt(state, transition, &client_intent_id, &session_id)
        .await;
    // Mentioned files are read after the transition guard is released, so a
    // slow disk cannot hold it; authority is reacquired below before dispatch.
    let budget = prompt.mention_budget(&files);
    let owner = (&binding_id, &session_id);
    let mentioned =
        mentions::resolve_mentions(state, &project_id, &chat_id, owner, mentioned, budget).await;
    prompt.mentions = match mentioned {
        Ok(mentioned) => mentioned,
        Err(failure) => {
            super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
            return Err(failure);
        }
    };
    let _transition = super::turn_workspace::relock_after_capture(
        state,
        &project_id,
        project_generation,
        &session_id,
        &client_intent_id,
    )
    .await
    .map_err(prompt_not_dispatched)?;
    let current_files = state
        .acp
        .file_stages
        .lock()
        .map_err(|_| prompt_not_dispatched(internal_error()))?
        .selected(project_id.as_str(), chat_id.as_str(), &staged);
    if !current_files.is_ok_and(|current| files.same_selection(&current)) {
        super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message("Attachments changed while preparing the prompt; send again."),
        }));
    }
    if !files.0.is_empty() && !prompt.fits_dispatch(&files) {
        super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message(
                crate::acp::attached_context::files::limits::DISPATCH_OVER.as_str(),
            ),
        }));
    }
    // Read the access only now, under the reacquired authority, so a
    // choice made while the prompt was being prepared is the one it runs in.
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session_id.clone(),
        runtime_epoch,
    };
    let access_mode = super::session_access::select_turn_access(state, binding_id.as_str(), owner)
        .map_err(|error| {
            super::turn_workspace::forget_unprepared(state, &session_id, &client_intent_id);
            prompt_not_dispatched(error)
        })?;
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
        staged::record_sent(state, &project_id, &chat_id, &tracked_intent, &files);
        prompt_images::retire(state, &project_id, &chat_id, &sent_images);
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
    // Steer sends no `@` mentions; `session_operations/mentions.rs` says why.
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

mod mentions;
