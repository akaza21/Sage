//! IPC: a terminal turn's workspace (Git) changes.

use rusqlite::OptionalExtension;

use super::*;
use crate::acp::contract::{ClientIntentId, SageAgentSessionId};
use crate::turn_workspace::{self, TurnWorkspaceChange};

pub(super) fn forget_unprepared(
    state: &AppState,
    session: &SageAgentSessionId,
    intent: &ClientIntentId,
) {
    let _ = turn_workspace::forget_unprepared(
        &state.database.connection(),
        session.as_str(),
        intent.as_str(),
    );
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnWorkspaceChangesRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
    turn_id: crate::acp::contract::SageTurnId,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnWorkspaceChangesResponse {
    root_id: String,
    tracked: bool,
    truncated: bool,
    changes: Vec<TurnWorkspaceChange>,
}

/// Captures the workspace mark a prompt's turn is later diffed against.
pub(super) async fn mark_before_prompt(
    state: &AppState,
    transition: crate::state::ActiveProjectContext<'_>,
    client_intent_id: &ClientIntentId,
    session_id: &SageAgentSessionId,
) {
    mark_before_prompt_with(
        state,
        transition,
        client_intent_id,
        session_id,
        |path| async move { turn_workspace::capture_baseline(&path).await },
    )
    .await;
}

async fn mark_before_prompt_with<
    F: std::future::Future<Output = turn_workspace::BaselineCapture>,
>(
    state: &AppState,
    transition: crate::state::ActiveProjectContext<'_>,
    client_intent_id: &ClientIntentId,
    session_id: &SageAgentSessionId,
    capture: impl FnOnce(std::path::PathBuf) -> F,
) {
    let root = state.database.selected_project_root_authority(&transition);
    // Capturing Git evidence is optional and must never own the project lock.
    drop(transition);
    let Ok(root) = root else { return };
    let registered = state
        .database
        .write_connection()
        .ok()
        .and_then(|mut connection| {
            turn_workspace::remember(
                &mut connection,
                session_id.as_str(),
                client_intent_id.as_str(),
                root.scope().2.as_str(),
                root.as_path(),
                None,
            )
            .ok()
        });
    if registered != Some(true) {
        return;
    }
    let captured = capture(root.as_path().to_path_buf()).await;
    let connection = state.database.connection();
    let result = match captured {
        turn_workspace::BaselineCapture::Captured(mark) => turn_workspace::publish_baseline(
            &connection,
            session_id.as_str(),
            client_intent_id.as_str(),
            Some(*mark),
        ),
        turn_workspace::BaselineCapture::Unsupported => turn_workspace::publish_baseline(
            &connection,
            session_id.as_str(),
            client_intent_id.as_str(),
            None,
        ),
        turn_workspace::BaselineCapture::Failed => turn_workspace::publish_baseline_failure(
            &connection,
            session_id.as_str(),
            client_intent_id.as_str(),
        ),
    };
    drop(connection);
    if let Err(error) = result {
        tracing::warn!(?error, "Workspace baseline was not saved");
        forget_unprepared(state, session_id, client_intent_id);
    }
}

/// Reacquires project authority after the capture, which ran without it. A
/// project switch or a Chat archive during the capture advanced the
/// transition epoch, so the send fails here as stale, undispatched, and its
/// workspace mark is forgotten.
pub(super) async fn relock_after_capture<'a>(
    state: &'a AppState,
    project_id: &SageProjectId,
    generation: ProjectGeneration,
    session_id: &SageAgentSessionId,
    client_intent_id: &ClientIntentId,
) -> Result<crate::state::ActiveProjectContext<'a>, AcpIpcError> {
    lock_acp_context(state, project_id, generation)
        .await
        .inspect_err(|_| forget_unprepared(state, session_id, client_intent_id))
}

#[tauri::command]
pub async fn get_acp_turn_workspace_changes(
    state: State<'_, AppState>,
    request: TurnWorkspaceChangesRequest,
) -> Result<TurnWorkspaceChangesResponse, AcpIpcError> {
    let (transition, _snapshot) = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    let root = state
        .database
        .selected_project_root_authority(&transition)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The selected project root is unavailable."),
        })?;
    let turn: Option<(String, String)> = state
        .database
        .connection()
        .query_row(
            "SELECT client_intent_id,session_id FROM turns WHERE project_id=?1 AND chat_id=?2 AND id=?3 AND terminal_at IS NOT NULL",
            rusqlite::params![
                request.project_id.as_str(),
                request.chat_id.as_str(),
                request.turn_id.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| store_error(crate::acp::store::StoreError::Database(error)))?;
    let changes = match turn {
        Some((key, session)) => turn_workspace::load(
            &state.database.connection(),
            &session,
            &key,
            root.scope().2.as_str(),
        )
        .map_err(store_error)?,
        _ => turn_workspace::TurnWorkspaceChanges::untracked(),
    };
    Ok(TurnWorkspaceChangesResponse {
        root_id: root.scope().2.as_str().to_owned(),
        tracked: changes.tracked,
        truncated: changes.truncated,
        changes: changes.changes,
    })
}

#[cfg(test)]
mod tests;
