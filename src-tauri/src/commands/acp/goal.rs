use super::*;
use crate::acp::goal::GoalAction;
use sage_acp_protocol::contract::{
    ClientIntentId, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExistingGoalRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

#[tauri::command]
pub(crate) async fn pause_existing_goal(
    state: State<'_, AppState>,
    request: ExistingGoalRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    mutate(&state, request, GoalAction::PauseExisting).await
}

#[tauri::command]
pub(crate) async fn clear_existing_goal(
    state: State<'_, AppState>,
    request: ExistingGoalRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    mutate(&state, request, GoalAction::ClearExisting).await
}

async fn mutate(
    state: &AppState,
    request: ExistingGoalRequest,
    action: GoalAction,
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
    let _intent_id = request.client_intent_id;
    state
        .acp
        .mutate_existing_goal(
            RuntimeOwner {
                project_id: request.project_id.clone(),
                chat_id: request.chat_id.clone(),
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
            },
            action,
        )
        .map_err(goal_error)?;
    mutation_response(state, request.project_id, request.chat_id)
}

fn goal_error(error: crate::acp::runtime::HostError) -> AcpIpcError {
    match error {
        crate::acp::runtime::HostError::GoalMutationRejected => AcpIpcError::Rejected {
            message: safe_message("The harness did not apply the native Goal action."),
        },
        crate::acp::runtime::HostError::Intent | crate::acp::runtime::HostError::State => {
            AcpIpcError::Unavailable {
                reason: crate::acp::ipc::AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message("This session has no mutable native Goal."),
            }
        }
        _ => AcpIpcError::RecoveryRequired {
            message: safe_message(
                "The Goal action outcome is unknown; reload the session before trying again.",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn request() -> Value {
        json!({
            "projectId":"project-goal", "projectGeneration":"1", "chatId":"chat-goal",
            "expectedProjectionRevision":"1", "clientIntentId":"intent-goal",
            "sessionId":"session-goal", "runtimeEpoch":"1"
        })
    }

    #[test]
    fn goal_ipc_accepts_only_owner_fields_and_no_status_objective_or_budget() {
        assert!(serde_json::from_value::<ExistingGoalRequest>(request()).is_ok());
        for (key, value) in [
            ("status", json!("active")),
            ("objective", json!("invented")),
            ("tokenBudget", json!(100)),
            ("action", json!("set")),
        ] {
            let mut extra = request();
            extra[key] = value;
            assert!(serde_json::from_value::<ExistingGoalRequest>(extra).is_err());
        }
    }

    #[test]
    fn correlated_goal_refusal_is_not_reported_as_success() {
        assert!(matches!(
            goal_error(crate::acp::runtime::HostError::GoalMutationRejected),
            AcpIpcError::Rejected { .. }
        ));
        assert!(matches!(
            goal_error(crate::acp::runtime::HostError::DeliveryUnknown),
            AcpIpcError::RecoveryRequired { .. }
        ));
    }
}

#[cfg(test)]
#[path = "goal_tests.rs"]
pub(super) mod integration_tests;
