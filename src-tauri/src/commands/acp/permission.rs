//! Owner-landed permission-decision command stub. AGENT-1 replaces the body.

use tauri::State;

use crate::{
    acp::runtime::permissions::PermissionDecision,
    acp::service::PermissionDecisionIntent,
    acp::{
        contract::{
            RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
            SageTurnId,
        },
        intent::{AcpIpcError, AcpUnavailableReason},
    },
    state::AppState,
};

use super::errors::safe_message;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvePermissionDecisionRequest {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub request_id: SageServerRequestId,
    pub decision: PermissionDecision,
}

#[tauri::command]
pub async fn resolve_permission_decision(
    state: State<'_, AppState>,
    request: ResolvePermissionDecisionRequest,
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
            decision: request.decision,
        })
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The permission runtime is unavailable."),
        })?;
    serde_json::to_value(response).map_err(|_| AcpIpcError::Internal {
        message: safe_message("The permission response could not be encoded."),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ResolvePermissionDecisionRequest;

    #[test]
    fn decision_request_requires_the_exact_six_field_identity() {
        let request: ResolvePermissionDecisionRequest = serde_json::from_value(json!({
            "projectId": "project-one",
            "chatId": "chat-one",
            "sessionId": "session-one",
            "turnId": "turn-one",
            "runtimeEpoch": "7",
            "requestId": "request-one",
            "decision": "deny"
        }))
        .unwrap();

        assert_eq!(request.project_id.as_str(), "project-one");
        assert_eq!(request.chat_id.as_str(), "chat-one");
        assert_eq!(request.session_id.as_str(), "session-one");
        assert_eq!(request.turn_id.as_str(), "turn-one");
        assert_eq!(request.runtime_epoch.get(), 7);
        assert_eq!(request.request_id.as_str(), "request-one");
    }

    #[test]
    fn malformed_or_partial_decision_identity_is_rejected_at_deserialization() {
        let exact = json!({
            "projectId": "project-one",
            "chatId": "chat-one",
            "sessionId": "session-one",
            "turnId": "turn-one",
            "runtimeEpoch": "7",
            "requestId": "request-one",
            "decision": "deny"
        });
        for malformed in [
            json!({"projectId": "project-one", "chatId": "chat-one", "sessionId": "session-one", "runtimeEpoch": "7", "requestId": "request-one", "decision": "deny"}),
            json!({"projectId": "project-one", "chatId": "chat-one", "sessionId": "session-one", "turnId": "turn-one", "requestId": "request-one", "decision": "deny"}),
            json!({"projectId": "", "chatId": "chat-one", "sessionId": "session-one", "turnId": "turn-one", "runtimeEpoch": "7", "requestId": "request-one", "decision": "deny"}),
            json!({"projectId": "project-one", "chatId": "chat-one", "sessionId": "session-one", "turnId": "turn-one", "runtimeEpoch": 7, "requestId": "request-one", "decision": "deny"}),
            json!({"projectId": "project-one", "chatId": "chat-one", "sessionId": "session-one", "turnId": "turn-one", "runtimeEpoch": "9223372036854775808", "requestId": "request-one", "decision": "deny"}),
            json!({"projectId": "project-one", "chatId": "chat-one", "sessionId": "session-one", "turnId": "turn-one", "runtimeEpoch": "7", "requestId": "request-one", "decision": "deny", "policy": "allow"}),
        ] {
            assert!(serde_json::from_value::<ResolvePermissionDecisionRequest>(malformed).is_err());
        }
        assert!(serde_json::from_value::<ResolvePermissionDecisionRequest>(exact).is_ok());
    }
}
