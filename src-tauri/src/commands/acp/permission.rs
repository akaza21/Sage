//! Owner-landed permission-decision command stub. AGENT-1 replaces the body.

use tauri::State;

use crate::{
    acp::browser::permission::BrowserPermissionChoice,
    acp::runtime::permissions::PermissionDecision,
    acp::service::PermissionDecisionIntent,
    acp::service::browser_permission::BrowserPermissionOwner,
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
    pub decision: PermissionChoice,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionChoice {
    Ask,
    AllowOnce,
    AllowAlways,
    AllowForOriginSession,
    Deny,
}

#[tauri::command]
pub async fn resolve_permission_decision(
    state: State<'_, AppState>,
    request: ResolvePermissionDecisionRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    let owner = BrowserPermissionOwner {
        project_id: request.project_id.clone(),
        chat_id: request.chat_id.clone(),
        session_id: request.session_id.clone(),
        turn_id: request.turn_id.clone(),
        runtime_epoch: request.runtime_epoch,
        binding_digest: String::new(),
    };
    let kind = state
        .acp
        .permission_request_kind(&owner, &request.request_id)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The permission request is unavailable."),
        })?;
    if kind == "browserPermission" {
        let choice = match request.decision {
            PermissionChoice::AllowOnce => BrowserPermissionChoice::AllowOnce,
            PermissionChoice::AllowAlways => {
                return Err(AcpIpcError::Unavailable {
                    reason: AcpUnavailableReason::CapabilityUnavailable,
                    message: safe_message("This browser permission choice is unavailable."),
                });
            }
            PermissionChoice::AllowForOriginSession => {
                BrowserPermissionChoice::AllowForOriginSession
            }
            PermissionChoice::Deny => BrowserPermissionChoice::Deny,
            PermissionChoice::Ask => {
                return Err(AcpIpcError::Unavailable {
                    reason: AcpUnavailableReason::CapabilityUnavailable,
                    message: safe_message("This browser permission choice is unavailable."),
                });
            }
        };
        let response = state
            .acp
            .resolve_browser_permission_decision(owner, request.request_id, choice)
            .map_err(|_| AcpIpcError::Unavailable {
                reason: AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message("The browser permission request is unavailable."),
            })?;
        return serde_json::to_value(response).map_err(|_| AcpIpcError::Internal {
            message: safe_message("The permission response could not be encoded."),
        });
    }
    let decision = match request.decision {
        PermissionChoice::Ask => PermissionDecision::Ask,
        PermissionChoice::AllowOnce => PermissionDecision::AllowOnce,
        PermissionChoice::AllowAlways => PermissionDecision::AllowAlways,
        PermissionChoice::Deny => PermissionDecision::Deny,
        PermissionChoice::AllowForOriginSession => {
            return Err(AcpIpcError::Unavailable {
                reason: AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message("This permission choice is unavailable."),
            });
        }
    };
    let response = state
        .acp
        .resolve_permission_decision(PermissionDecisionIntent {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            turn_id: request.turn_id,
            runtime_epoch: request.runtime_epoch,
            request_id: request.request_id,
            decision,
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

    #[test]
    fn browser_origin_session_choice_is_accepted_on_the_wire() {
        let request: ResolvePermissionDecisionRequest = serde_json::from_value(json!({
            "projectId": "project-one",
            "chatId": "chat-one",
            "sessionId": "session-one",
            "turnId": "turn-one",
            "runtimeEpoch": "7",
            "requestId": "request-one",
            "decision": "allowForOriginSession"
        }))
        .unwrap();
        assert!(matches!(
            request.decision,
            super::PermissionChoice::AllowForOriginSession
        ));
    }
}
