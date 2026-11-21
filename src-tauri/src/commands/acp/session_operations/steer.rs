use super::*;

pub(crate) async fn steer_acp_turn_inner(
    state: &AppState,
    request: SteerAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    steer_command_turn(state, request)
        .await
        .map_err(|failure| failure.failure)
}

/// Preserve dispatch certainty for command-backed mutations as well as plain steering.
pub(crate) async fn steer_command_turn(
    state: &AppState,
    request: SteerAcpTurnRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await
    .map_err(prompt_not_dispatched)?;
    validate_steer_intent(&request, &snapshot)
        .map_err(intent_error)
        .map_err(prompt_not_dispatched)?;
    image_input_block(&request.attached_context).map_err(prompt_not_dispatched)?;
    // Steer sends no `@` mentions; `session_operations/mentions.rs` says why.
    let binding_id = session_routes::durable_binding_id(state, &request.session_id)
        .map_err(store_error)
        .map_err(prompt_not_dispatched)?;
    let prompt = effective_prompt(&request.prompt, &request.attached_context)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        })
        .map_err(prompt_not_dispatched)?;
    let client_message_id = sage_acp_protocol::contract::RequiredText::new(
        "clientMessageId",
        request.client_intent_id.as_str().to_owned(),
    )
    .map_err(|_| AcpIpcError::InvalidRequest {
        message: safe_message("The Steer intent identifier is invalid."),
    })
    .map_err(prompt_not_dispatched)?;
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
        .map_err(dispatch_error)
        .map_err(prompt_dispatch_unknown)?;
    mutation_response(state, project_id, chat_id).map_err(prompt_dispatch_unknown)
}
