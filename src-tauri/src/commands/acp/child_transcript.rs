use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    acp::{
        ipc::{AcpIpcError, AcpUnavailableReason, IntentError, validate_base_intent},
        service::{ChildTranscriptRead, ChildTranscriptReadError},
        store::child_transcript::{ChildTranscriptScope, SpawnModel, load_spawn_model},
    },
    state::AppState,
};
use sage_acp_protocol::{
    codex_protocol::ChildTranscriptSnapshot,
    contract::{
        ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageItemId, SageProjectId,
    },
    intent::ExternalId,
    scheduler::RuntimeOwner,
};

use super::{intent_error, safe_message, snapshot_for};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChildTranscriptRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
    item_id: SageItemId,
    agent_key: ExternalId,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ChildTranscriptResponse {
    Current {
        snapshot: ChildTranscriptSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        spawn_model: Option<SpawnModel>,
    },
    FromEarlierSession {
        snapshot: ChildTranscriptSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        spawn_model: Option<SpawnModel>,
    },
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
        // Child reads key off the child projection epoch, not the revision.
        snapshot.projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch.is_some() && snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    let parent_thread_id: Option<String> = state
        .database
        .connection()
        .query_row(
            "SELECT o.parent_thread_id FROM acp_child_thread_bindings b
         JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
         WHERE b.item_id=?1 AND b.project_id=?2 AND b.chat_id=?3
           AND b.session_id=?4 AND b.runtime_epoch=?5
           AND b.native_child_thread_id=?6
           AND o.project_id=b.project_id AND o.chat_id=b.chat_id
           AND o.session_id=b.session_id AND o.runtime_epoch=b.runtime_epoch
           AND o.source='codexSubAgentActivity'",
            params![
                request.item_id.as_str(),
                request.project_id.as_str(),
                request.chat_id.as_str(),
                request.session_id.as_str(),
                request.runtime_epoch.get() as i64,
                request.agent_key.as_str()
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| read_error(ChildTranscriptReadError::RecoveryRequired))?;
    let parent_thread_id =
        parent_thread_id.ok_or_else(|| read_error(ChildTranscriptReadError::Unavailable))?;
    let parent_thread_id = ExternalId::new("parentThreadId", parent_thread_id)
        .map_err(|_| read_error(ChildTranscriptReadError::RecoveryRequired))?;
    let scope = ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            runtime_epoch: request.runtime_epoch,
        },
        item_id: request.item_id.as_str().to_owned(),
        child_thread_id: request.agent_key.as_str().to_owned(),
        parent_thread_id: parent_thread_id.as_str().to_owned(),
    };
    let owner = scope.owner.clone();
    let read = state.acp.read_child_transcript(scope).map_err(read_error)?;
    // The header's model label is optional chrome: an unreadable spawn row
    // never fails an otherwise verified transcript read.
    let spawn_model = || {
        load_spawn_model(
            &state.database.connection(),
            &owner,
            request.agent_key.as_str(),
        )
        .ok()
        .flatten()
    };
    Ok(match read {
        ChildTranscriptRead::Current(snapshot) => ChildTranscriptResponse::Current {
            snapshot,
            spawn_model: spawn_model(),
        },
        ChildTranscriptRead::FromEarlierSession(snapshot) => {
            ChildTranscriptResponse::FromEarlierSession {
                snapshot,
                spawn_model: spawn_model(),
            }
        }
        ChildTranscriptRead::TooLarge => ChildTranscriptResponse::TooLarge,
    })
}

#[cfg(test)]
#[path = "child_transcript_tests.rs"]
mod tests;

fn read_error(error: ChildTranscriptReadError) -> AcpIpcError {
    match error {
        ChildTranscriptReadError::Rejected => AcpIpcError::Rejected {
            message: safe_message("The native child transcript read was rejected."),
        },
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
