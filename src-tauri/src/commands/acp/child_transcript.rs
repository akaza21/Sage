use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    acp::{
        bindings::codex::ChildTranscriptSnapshot,
        contract::{
            ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId,
            SageItemId, SageProjectId,
        },
        intent::{
            AcpIpcError, AcpUnavailableReason, ExternalId, IntentError, validate_base_intent,
        },
        scheduler::RuntimeOwner,
        service::{ChildTranscriptRead, ChildTranscriptReadError},
        store::child_transcript::ChildTranscriptScope,
    },
    state::AppState,
};

use super::{intent_error, safe_message, snapshot_for};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChildTranscriptRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    expected_projection_revision: ProjectionRevision,
    session_id: SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
    item_id: SageItemId,
    child_thread_id: ExternalId,
    parent_thread_id: ExternalId,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ChildTranscriptResponse {
    Current { snapshot: ChildTranscriptSnapshot },
    FromEarlierSession { snapshot: ChildTranscriptSnapshot },
    TooLarge,
}

#[tauri::command]
pub(crate) async fn get_acp_child_transcript(
    state: State<'_, AppState>,
    request: ChildTranscriptRequest,
) -> Result<ChildTranscriptResponse, AcpIpcError> {
    get_acp_child_transcript_inner(&state, request).await
}

pub(crate) async fn get_acp_child_transcript_inner(
    state: &AppState,
    request: ChildTranscriptRequest,
) -> Result<ChildTranscriptResponse, AcpIpcError> {
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
    if snapshot.runtime_epoch.is_some() && snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    let scope = ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            runtime_epoch: request.runtime_epoch,
        },
        item_id: request.item_id.as_str().to_owned(),
        child_thread_id: request.child_thread_id.as_str().to_owned(),
        parent_thread_id: request.parent_thread_id.as_str().to_owned(),
    };
    match state.acp.read_child_transcript(scope).map_err(read_error)? {
        ChildTranscriptRead::Current(snapshot) => Ok(ChildTranscriptResponse::Current { snapshot }),
        ChildTranscriptRead::FromEarlierSession(snapshot) => {
            Ok(ChildTranscriptResponse::FromEarlierSession { snapshot })
        }
        ChildTranscriptRead::TooLarge => Ok(ChildTranscriptResponse::TooLarge),
    }
}

#[cfg(test)]
#[path = "child_transcript_tests.rs"]
mod tests;

fn read_error(error: ChildTranscriptReadError) -> AcpIpcError {
    match error {
        ChildTranscriptReadError::Unavailable => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The child transcript is unavailable for this session."),
        },
        ChildTranscriptReadError::Stale
        | ChildTranscriptReadError::Store(
            crate::acp::store::child_transcript::ChildTranscriptStoreError::Stale,
        ) => intent_error(IntentError::StaleRuntimeEpoch),
        ChildTranscriptReadError::RecoveryRequired | ChildTranscriptReadError::Store(_) => {
            AcpIpcError::RecoveryRequired {
                message: safe_message(
                    "The native child transcript could not be verified; reload the session.",
                ),
            }
        }
    }
}
