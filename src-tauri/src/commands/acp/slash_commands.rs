use super::*;
use crate::acp::{contract::RequiredText, intent::Prompt, slash_commands::CommandList};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunCommandRequest {
    #[serde(flatten)]
    pub(crate) session: ActivateAcpSessionRequest,
    pub(crate) expected_commands_revision: u64,
    pub(crate) name: RequiredText<128>,
    pub(crate) input: Option<RequiredText<16384>>,
}

fn unavailable() -> AcpIpcError {
    AcpIpcError::Rejected {
        message: safe_message(
            "The live command list changed or is unavailable; reload it before running a command.",
        ),
    }
}
async fn list(
    state: &AppState,
    request: &ActivateAcpSessionRequest,
) -> Result<std::sync::Arc<crate::acp::slash_commands::CommandState>, AcpIpcError> {
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
    if snapshot.session_id.as_ref() != Some(&request.session_id)
        || snapshot.runtime_epoch != Some(request.runtime_epoch)
    {
        return Err(unavailable());
    }
    state
        .acp
        .available_commands(&RuntimeOwner {
            project_id: request.project_id.clone(),
            chat_id: request.chat_id.clone(),
            session_id: request.session_id.clone(),
            runtime_epoch: request.runtime_epoch,
        })
        .ok_or_else(unavailable)
}

#[tauri::command]
pub(crate) async fn list_acp_commands(
    state: State<'_, AppState>,
    request: ActivateAcpSessionRequest,
) -> Result<CommandList, AcpIpcError> {
    list(&state, &request).await.map(|v| v.snapshot())
}

#[tauri::command]
pub(crate) async fn run_acp_command(
    state: State<'_, AppState>,
    request: RunCommandRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    run(&state, request).await
}
pub(super) async fn run(
    state: &AppState,
    request: RunCommandRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    use session_operations::prompt_not_dispatched;
    let commands = list(state, &request.session)
        .await
        .map_err(prompt_not_dispatched)?;
    let name = request.name.as_str();
    if name
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '/')
    {
        return Err(prompt_not_dispatched(unavailable()));
    }
    let text = match request.input {
        Some(input) => format!("/{name} {}", input.as_str()),
        None => format!("/{name}"),
    };
    commands
        .validate(request.expected_commands_revision, &text)
        .map_err(|_| prompt_not_dispatched(unavailable()))?;
    let mut prompt =
        Prompt::new("command", text).map_err(|_| prompt_not_dispatched(unavailable()))?;
    prompt.command_revision = Some(request.expected_commands_revision);
    let r = request.session;
    session_operations::send_acp_turn_inner(
        state,
        SendAcpTurnRequest::Session {
            project_id: r.project_id,
            project_generation: r.project_generation,
            chat_id: r.chat_id,
            expected_projection_revision: r.expected_projection_revision,
            client_intent_id: r.client_intent_id,
            session_id: r.session_id,
            runtime_epoch: r.runtime_epoch,
            prompt,
            attached_context: AttachedContexts::default(),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_request_requires_session_authority_and_bounded_input() {
        let value = serde_json::json!({"projectId":"p","projectGeneration":"1","chatId":"c","sessionId":"s","runtimeEpoch":"1","expectedProjectionRevision":"0","clientIntentId":"i","expectedCommandsRevision":1,"name":"plan","input":"task"});
        let request: RunCommandRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(request.name.as_str(), "plan");
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("runtimeEpoch");
        assert!(serde_json::from_value::<RunCommandRequest>(missing).is_err());
        let mut oversized = value;
        oversized["input"] = serde_json::json!("x".repeat(16385));
        assert!(serde_json::from_value::<RunCommandRequest>(oversized).is_err());
    }
}
