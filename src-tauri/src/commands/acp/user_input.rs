//! Answers a harness question request (`PermissionRequest` with
//! `userInput`). Cancelling one uses `resolve_permission_decision` with
//! `deny`, exactly like declining an approval.

use tauri::State;

use crate::{
    acp::{
        contract::{
            RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
            SageTurnId,
        },
        intent::{AcpIpcError, AcpUnavailableReason, user_input::UserInputAnswers},
        runtime::permissions::PermissionDecision,
        service::PermissionDecisionIntent,
    },
    state::AppState,
};

use super::errors::safe_message;

/// `Debug` is safe: `UserInputAnswers` never renders answer text.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnswerUserInputRequest {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub request_id: SageServerRequestId,
    pub answers: UserInputAnswers,
}

#[tauri::command]
pub async fn answer_user_input(
    state: State<'_, AppState>,
    request: AnswerUserInputRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    let response = state
        .acp
        .resolve_permission_decision(PermissionDecisionIntent {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            turn_id: request.turn_id,
            runtime_epoch: request.runtime_epoch,
            request_id: request.request_id,
            decision: PermissionDecision::Answer(request.answers),
        })
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message(
                "The question request is unavailable or the answers do not match it.",
            ),
        })?;
    serde_json::to_value(response).map_err(|_| AcpIpcError::Internal {
        message: safe_message("The answer response could not be encoded."),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AnswerUserInputRequest;

    fn request(answers: serde_json::Value) -> serde_json::Value {
        json!({
            "projectId": "project-one",
            "chatId": "chat-one",
            "sessionId": "session-one",
            "turnId": "turn-one",
            "runtimeEpoch": "7",
            "requestId": "request-one",
            "answers": answers
        })
    }

    #[test]
    fn answers_decode_bounded_and_never_render_in_debug() {
        let decoded: AnswerUserInputRequest = serde_json::from_value(request(json!([
            {"questionId": "token", "text": "s3cret"}
        ])))
        .unwrap();
        assert_eq!(decoded.request_id.as_str(), "request-one");
        assert!(!format!("{decoded:?}").contains("s3cret"));
        for malformed in [
            json!({"token": "s3cret"}),
            json!([{"questionId": "", "text": "x"}]),
            json!([{"questionId": "q", "text": "x", "extra": true}]),
            json!(vec![json!({"questionId": "q", "text": "x"}); 9]),
        ] {
            assert!(serde_json::from_value::<AnswerUserInputRequest>(request(malformed)).is_err());
        }
    }
}
