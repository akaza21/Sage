//! AP-B B5 IPC: a terminal turn's workspace (Git) changes.

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
    transition: &crate::state::ActiveProjectContext<'_>,
    client_intent_id: &crate::acp::contract::ClientIntentId,
    session_id: &crate::acp::contract::SageAgentSessionId,
) -> Result<(), AcpIpcError> {
    let root = state
        .database
        .selected_project_root_authority(transition)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The selected project root is unavailable."),
        })?;
    let registered = turn_workspace::remember(
        &mut state
            .database
            .write_connection()
            .map_err(|_| AcpIpcError::RecoveryRequired {
                message: safe_message("Workspace tracking could not be saved."),
            })?,
        session_id.as_str(),
        client_intent_id.as_str(),
        root.scope().2.as_str(),
        root.as_path(),
        None,
    )
    .map_err(store_error)?;
    if registered {
        let mark = turn_workspace::capture(root.as_path()).await;
        turn_workspace::publish_baseline(
            &state.database.connection(),
            session_id.as_str(),
            client_intent_id.as_str(),
            mark,
        )
        .map_err(store_error)?;
    }
    Ok(())
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
