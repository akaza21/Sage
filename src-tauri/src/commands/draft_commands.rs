//! A new-chat draft's command list, kept current from each harness's live
//! catalog and passive probe. The WebView re-reads it on the change event.

use sage_acp_protocol::scheduler::RuntimeOwner;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{acp::slash_commands::CommandsChangedV1, state::AppState};

const CHANGED_EVENT: &str = "draft-commands-changed-v1";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DraftCommandsChanged<'a> {
    harness_id: &'a str,
}

/// The commands a draft offers for `harness_id`, as the live list shapes them.
/// Reads the settings store only: no harness process starts.
#[tauri::command]
pub(crate) fn list_draft_commands(
    state: State<'_, AppState>,
    harness_id: String,
    project_id: Option<String>,
) -> Vec<serde_json::Value> {
    state
        .harnesses
        .draft_commands(&state.settings, &harness_id, project_id.as_deref())
}

/// Keeps a live session's new catalog revision for its harness's drafts, off
/// the runtime's thread.
pub(crate) fn follow_live_catalog(app: &AppHandle, notice: &CommandsChangedV1) {
    let owner = RuntimeOwner {
        project_id: notice.sage_project_id.clone(),
        chat_id: notice.sage_chat_id.clone(),
        session_id: notice.sage_agent_session_id.clone(),
        runtime_epoch: notice.runtime_epoch,
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let Some(list) = state.acp.available_commands(&owner) else {
            return;
        };
        let Some(harness_id) = session_harness(&state, &owner) else {
            return;
        };
        let commands = list.snapshot().commands;
        let project = Some(owner.project_id.as_str());
        if crate::harnesses::record_draft_commands(&state.settings, &harness_id, project, &commands)
        {
            changed(&app, &harness_id);
        }
    });
}

/// Keeps what the harness's model probe also read of its commands.
pub(crate) fn follow_probe(app: &AppHandle, state: &AppState, harness_id: &str) {
    if state
        .harnesses
        .record_probed_commands(&state.settings, harness_id)
    {
        changed(app, harness_id);
    }
}

fn changed(app: &AppHandle, harness_id: &str) {
    let _ = app.emit(CHANGED_EVENT, DraftCommandsChanged { harness_id });
}

fn session_harness(state: &AppState, owner: &RuntimeOwner) -> Option<String> {
    let binding: String = state
        .database
        .connection()
        .query_row(
            "SELECT binding_id FROM agent_sessions WHERE id = ?1",
            [owner.session_id.as_str()],
            |row| row.get(0),
        )
        .ok()?;
    state.harnesses.harness_id_for_binding(&binding)
}
