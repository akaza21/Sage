use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    acp::{
        errors::SessionControlError,
        ipc::{AcpIpcError, AcpUnavailableReason, IntentError, validate_base_intent},
    },
    state::AppState,
};
use sage_acp_protocol::{
    contract::{
        ProjectGeneration, RuntimeEpoch, SafeText, SageAgentSessionId, SageChatId, SageProjectId,
        SageTurnId,
    },
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

use super::{intent_error, safe_message, snapshot_for};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionControlRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
    control: ControlIntent,
}

/// A session control as the chat names it: a turn, or the key of a live
/// activity on its page.
#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ControlIntent {
    UndoTurn { turn_id: SageTurnId },
    StopActivity { activity_key: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionControlResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

/// Undoes the latest turn or stops a background task in the chat's running
/// session. The request names the session and runtime rather than a
/// projection revision: a stop leaves the transcript as it is, and an undo
/// only takes its turn out of the page once the harness has undone it.
#[tauri::command]
pub(crate) async fn control_acp_session(
    state: State<'_, AppState>,
    request: SessionControlRequest,
) -> Result<SessionControlResponse, AcpIpcError> {
    // The project transition lock is held to the end, through the control,
    // the record and the park: every prompt is prepared under the same lock
    // (`send_acp_turn_inner` dispatches under it, and the prepare commits
    // before the dispatch returns), so no turn can start between the
    // harness undoing a turn and the store recording it.
    let (_transition, snapshot) = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        snapshot.projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    let owner = RuntimeOwner {
        project_id: request.project_id.clone(),
        chat_id: request.chat_id.clone(),
        session_id: request.session_id.clone(),
        runtime_epoch: request.runtime_epoch,
    };
    let control = match request.control {
        ControlIntent::UndoTurn { turn_id } => {
            let summary = state
                .acp
                .undo_turn(&owner, turn_id)
                .await
                .map_err(control_error)?;
            return Ok(SessionControlResponse {
                summary: summary.map(|summary| summary.as_str().to_owned()),
            });
        }
        ControlIntent::StopActivity { activity_key } => {
            let activity_id = crate::acp::store::session_surface::activity_id(
                &state.database.connection(),
                request.project_id.as_str(),
                request.chat_id.as_str(),
                request.session_id.as_str(),
                &activity_key,
            )
            .map_err(|_| control_error(SessionControlError::RecoveryRequired))?
            .ok_or(control_error(SessionControlError::Unavailable))?;
            SessionControl::StopActivity { activity_id }
        }
    };
    let summary = state
        .acp
        .control_session(owner.clone(), control)
        .map_err(control_error)?;
    Ok(SessionControlResponse {
        summary: summary.map(|summary| summary.as_str().to_owned()),
    })
}

pub(super) fn control_error(error: SessionControlError) -> AcpIpcError {
    match error {
        // The harness's own reason, already redacted and bounded.
        SessionControlError::Refused(reason) => AcpIpcError::Rejected {
            message: reason
                .and_then(|reason| SafeText::new("message", reason.as_str()).ok())
                .unwrap_or_else(|| safe_message("The agent declined this request.")),
        },
        SessionControlError::Unavailable => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("This session cannot do that right now."),
        },
        SessionControlError::Late => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The agent did not answer in time; it may still finish."),
        },
        SessionControlError::RecoveryRequired => AcpIpcError::RecoveryRequired {
            message: safe_message(
                "The outcome is unknown; reload the session before trying again.",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_control_request_names_its_session_and_one_control() {
        let request: SessionControlRequest = serde_json::from_value(json!({
            "projectId":"project-one", "projectGeneration":"1", "chatId":"chat-one",
            "sessionId":"session-one", "runtimeEpoch":"1",
            "control":{"kind":"undoTurn","turnId":"turn-6f9619ff-8b86-d011-b42d-00cf4fc964ff"}
        }))
        .unwrap();
        assert!(matches!(request.control, ControlIntent::UndoTurn { .. }));
        let stop: SessionControlRequest = serde_json::from_value(json!({
            "projectId":"project-one", "projectGeneration":"1", "chatId":"chat-one",
            "sessionId":"session-one", "runtimeEpoch":"1",
            "control":{"kind":"stopActivity","activityKey":"0a1b2c"}
        }))
        .unwrap();
        assert!(matches!(stop.control, ControlIntent::StopActivity { .. }));
        assert!(
            serde_json::from_value::<SessionControlRequest>(json!({
                "projectId":"project-one", "projectGeneration":"1", "chatId":"chat-one",
                "sessionId":"session-one", "runtimeEpoch":"1",
                "control":{"kind":"deleteEverything"}
            }))
            .is_err()
        );
    }
}
