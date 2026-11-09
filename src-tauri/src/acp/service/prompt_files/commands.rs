use super::{Resources, StagedFiles, attachment, read::read_paths, release_detached};
use crate::state::AppState;
use serde::Deserialize;
use tauri::{AppHandle, State, Window};
use tauri_plugin_dialog::DialogExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FileOwner {
    project_id: String,
    chat_id: String,
    project_generation: u64,
}
/// What the native picker offers. The WebView never names a path: every
/// staged file comes from a picker this command opens itself, so injected
/// content cannot stage an arbitrary file through this IPC.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PickKind {
    Files,
    Folder,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StageFilesRequest {
    owner: FileOwner,
    pick: PickKind,
    expected_revision: Option<String>,
}
/// Composer text too long to type inline. The WebView supplies the text the
/// person pasted, never a path, so nothing outside the paste is read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StageTextRequest {
    owner: FileOwner,
    expected_revision: Option<String>,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveFileRequest {
    owner: FileOwner,
    revision: String,
    id: String,
}

/// Checks the owner under the transition guard and returns the attachment
/// identity (`chat_tabs.attached_at`) of its chat tab.
fn validate(state: &AppState, owner: &FileOwner, generation: u64) -> Result<i64, String> {
    // An owner always names its chat; nothing falls back to another tab.
    if owner.project_id.is_empty()
        || owner.chat_id.is_empty()
        || owner.project_id.len() > 512
        || owner.chat_id.len() > 512
        || owner.project_generation != generation
    {
        return Err("Attachment owner is stale or invalid.".into());
    }
    if state
        .database
        .active_project_id()
        .map_err(|_| "Cannot validate project.")?
        .as_deref()
        != Some(&owner.project_id)
    {
        return Err("Attachment project is not active.".into());
    }
    attachment(&state.database, &owner.project_id, &owner.chat_id)
        .map_err(|_| "Cannot validate attachment chat.")?
        .ok_or_else(|| "Attachment chat is unavailable.".into())
}

#[tauri::command]
pub(crate) async fn stage_prompt_files(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    request: StageFilesRequest,
) -> Result<Option<StagedFiles>, String> {
    let dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Attach to prompt");
    let pick = request.pick;
    stage_with(&state, request, move || {
        let picked = match pick {
            PickKind::Files => dialog.blocking_pick_files().unwrap_or_default(),
            PickKind::Folder => dialog.blocking_pick_folder().into_iter().collect(),
        };
        if picked.is_empty() {
            return Ok(None);
        }
        let paths = picked
            .into_iter()
            .map(|path| {
                path.into_path()
                    .ok()
                    .and_then(|path| path.to_str().map(str::to_owned))
                    .ok_or("Attachment path is not a UTF-8 local path.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        read_paths(&paths).map(Some)
    })
    .await
}

/// Runs the native pick and read without holding the project transition
/// guard, so a waiting picker, a slow folder or a large bounded walk never
/// blocks project switching, chat selection or shutdown persistence. The owner
/// is checked before and again under the guard before staging: a switch,
/// archive or tab close meanwhile rejects the snapshot. `None` is a cancelled
/// pick.
async fn stage_with<P>(
    state: &AppState,
    request: StageFilesRequest,
    pick_and_read: P,
) -> Result<Option<StagedFiles>, String>
where
    P: FnOnce() -> Result<Option<Resources>, &'static str> + Send + 'static,
{
    let admission = std::sync::Arc::clone(&state.acp.file_stage_reader)
        .try_lock_owned()
        .map_err(|_| "Another attachment selection is being read.")?;
    let attached = validate(
        state,
        &request.owner,
        *state.project_transitions.lock().await,
    )?;
    // One native picker and reader at a time. No queued file buffers and no
    // filesystem I/O on the async runtime thread.
    let resources = tauri::async_runtime::spawn_blocking(move || {
        let _admission = admission;
        pick_and_read()
    })
    .await
    .map_err(|_| "Attachment reader failed.")??;
    let Some(resources) = resources else {
        return Ok(None);
    };
    let transition = state.project_transitions.lock().await;
    if validate(state, &request.owner, *transition)? != attached {
        return Err("Attachment chat tab changed during selection.".into());
    }
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .stage(
            &request.owner.project_id,
            &request.owner.chat_id,
            attached,
            request.expected_revision.as_deref(),
            resources,
        )
        .map(Some)
        .map_err(str::to_owned)
}
#[tauri::command]
pub(crate) async fn stage_prompt_text(
    state: State<'_, AppState>,
    request: StageTextRequest,
) -> Result<StagedFiles, String> {
    stage_text(&state, request).await
}
pub(super) async fn stage_text(
    state: &AppState,
    request: StageTextRequest,
) -> Result<StagedFiles, String> {
    let transition = state.project_transitions.lock().await;
    let attached = validate(state, &request.owner, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .append_text(
            &request.owner.project_id,
            &request.owner.chat_id,
            attached,
            request.expected_revision.as_deref(),
            request.text,
        )
        .map_err(str::to_owned)
}
#[tauri::command]
pub(crate) async fn list_prompt_files(
    state: State<'_, AppState>,
    owner: FileOwner,
) -> Result<Option<StagedFiles>, String> {
    let transition = state.project_transitions.lock().await;
    validate(&state, &owner, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    Ok(state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .list(&owner.project_id, &owner.chat_id))
}
#[tauri::command]
pub(crate) async fn remove_prompt_file(
    state: State<'_, AppState>,
    request: RemoveFileRequest,
) -> Result<Option<StagedFiles>, String> {
    let transition = state.project_transitions.lock().await;
    validate(&state, &request.owner, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .remove(
            &request.owner.project_id,
            &request.owner.chat_id,
            &request.revision,
            &request.id,
        )
        .map_err(str::to_owned)
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
