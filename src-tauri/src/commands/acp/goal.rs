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

#[tauri::command]
pub(crate) async fn resume_existing_goal(
    state: State<'_, AppState>,
    request: ResumeGoalRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    mutate(
        &state,
        request.session,
        GoalAction::ResumeExisting {
            expected_goal: request.expected_goal,
        },
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResumeGoalRequest {
    #[serde(flatten)]
    pub session: ExistingGoalRequest,
    pub expected_goal: crate::acp::goal::GoalIdentity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetGoalRequest {
    #[serde(flatten)]
    pub session: ExistingGoalRequest,
    pub objective: sage_acp_protocol::contract::RequiredText<16000>,
}

#[tauri::command]
pub(crate) async fn set_goal(
    state: State<'_, AppState>,
    request: SetGoalRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    if request.objective.as_str().trim().is_empty()
        || !crate::acp::goal::valid_objective(request.objective.as_str())
    {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("Goal objective must contain 1 to 4,000 characters."),
        });
    }
    mutate(
        &state,
        request.session,
        GoalAction::Set {
            objective: request.objective.as_str().to_owned(),
        },
    )
    .await
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
    let command = state
        .acp
        .mutate_goal(
            RuntimeOwner {
                project_id: request.project_id.clone(),
                chat_id: request.chat_id.clone(),
                session_id: request.session_id.clone(),
                runtime_epoch: request.runtime_epoch,
            },
            action,
        )
        .map_err(goal_error)?;
    let current_turn = if snapshot
        .turn_state
        .is_some_and(|state| !state.is_terminal())
    {
        snapshot.turn_id.zip(snapshot.turn_revision)
    } else {
        None
    };
    drop(_transition);
    if let Some(command) = command {
        return super::slash_commands::run_command_turn(
            state,
            super::slash_commands::RunCommandRequest {
                session: ActivateAcpSessionRequest {
                    project_id: request.project_id,
                    project_generation: request.project_generation,
                    chat_id: request.chat_id,
                    expected_projection_revision: request.expected_projection_revision,
                    client_intent_id: request.client_intent_id,
                    session_id: request.session_id,
                    runtime_epoch: request.runtime_epoch,
                },
                expected_commands_revision: command.revision,
                name: command.name,
                input: Some(command.input),
            },
            current_turn,
        )
        .await
        .map_err(|error| match error.prompt_dispatch {
            crate::acp::ipc::PromptDispatch::NotStarted => error.failure,
            _ => AcpIpcError::RecoveryRequired {
                message: safe_message(
                    "Goal command outcome is unknown; reload this chat to verify.",
                ),
            },
        });
    }
    mutation_response(state, request.project_id, request.chat_id)
}

fn goal_error(error: crate::acp::runtime::HostError) -> AcpIpcError {
    match error {
        crate::acp::runtime::HostError::GoalChanged => AcpIpcError::Rejected {
            message: safe_message(
                "The goal changed before Resume could start. Refresh the goal before resuming.",
            ),
        },
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
        let mut set = request();
        set["objective"] = json!("Ship the release");
        assert!(serde_json::from_value::<SetGoalRequest>(set.clone()).is_ok());
        let mut budget = set.clone();
        budget["tokenBudget"] = json!(2000);
        assert!(serde_json::from_value::<SetGoalRequest>(budget).is_err());
        set["objective"] = json!("x".repeat(16001));
        assert!(serde_json::from_value::<SetGoalRequest>(set).is_err());
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
    fn resume_ipc_requires_bounded_displayed_identity_and_rejects_set_fields() {
        assert!(serde_json::from_value::<ResumeGoalRequest>(request()).is_err());
        let mut resume = request();
        resume["expectedGoal"] = json!({"objective":"Original", "createdAt":"1795300000"});
        assert!(serde_json::from_value::<ResumeGoalRequest>(resume.clone()).is_ok());
        for (key, value) in [
            ("objective", json!("x".repeat(16001))),
            ("createdAt", json!("x".repeat(65))),
        ] {
            let mut invalid = resume.clone();
            invalid["expectedGoal"][key] = value;
            assert!(serde_json::from_value::<ResumeGoalRequest>(invalid).is_err());
        }
        for (key, value) in [
            ("objective", json!("Edited")),
            ("tokenBudget", json!(2000)),
            ("status", json!("active")),
        ] {
            let mut invalid = resume.clone();
            invalid[key] = value;
            assert!(serde_json::from_value::<ResumeGoalRequest>(invalid).is_err());
        }
        let mut set = request();
        set["objective"] = json!("Ship");
        set["expectedGoal"] = resume["expectedGoal"].clone();
        assert!(serde_json::from_value::<SetGoalRequest>(set).is_err());
    }

    #[test]
    fn correlated_goal_refusal_is_not_reported_as_success() {
        let changed =
            serde_json::to_value(goal_error(crate::acp::runtime::HostError::GoalChanged)).unwrap();
        assert_eq!(changed["kind"], "rejected");
        assert!(
            changed["message"]
                .as_str()
                .unwrap()
                .contains("goal changed")
        );
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
