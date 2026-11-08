use super::*;
use crate::acp::{intent::Prompt, session_action::SessionAction};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunSessionActionRequest {
    #[serde(flatten)]
    pub(crate) session: ActivateAcpSessionRequest,
    pub(crate) action: SessionAction,
}

/// Runs a session action as a Turn on the idle session. The host refuses it
/// before any Turn is prepared unless the session negotiated the action, and
/// the binding refuses it again at dispatch.
#[tauri::command]
pub(crate) async fn run_session_action(
    state: State<'_, AppState>,
    request: RunSessionActionRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let RunSessionActionRequest { session, action } = request;
    let mut prompt = Prompt::new("sessionAction", action.prompt_text()).map_err(|_| {
        session_operations::prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message("The session action is invalid."),
        })
    })?;
    prompt.action = Some(action);
    session_operations::send_acp_turn_inner(
        &state,
        SendAcpTurnRequest::Session {
            project_id: session.project_id,
            project_generation: session.project_generation,
            chat_id: session.chat_id,
            expected_projection_revision: session.expected_projection_revision,
            client_intent_id: session.client_intent_id,
            session_id: session.session_id,
            runtime_epoch: session.runtime_epoch,
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
    fn session_action_request_requires_session_authority_and_a_known_action() {
        let value = serde_json::json!({"projectId":"p","projectGeneration":"1","chatId":"c","sessionId":"s","runtimeEpoch":"1","expectedProjectionRevision":"0","clientIntentId":"i","action":{"kind":"review","target":{"kind":"uncommittedChanges"}}});
        let request: RunSessionActionRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(request.action.prompt_text(), "/review");
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("runtimeEpoch");
        assert!(serde_json::from_value::<RunSessionActionRequest>(missing).is_err());
        let mut unknown = value;
        unknown["action"] = serde_json::json!({"kind":"revert"});
        assert!(serde_json::from_value::<RunSessionActionRequest>(unknown).is_err());
    }
}
