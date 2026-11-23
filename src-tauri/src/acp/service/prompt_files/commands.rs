use super::{
    Resources, StagedFiles, attachment, draft::DraftOwner, read::read_paths, release_detached,
};
use crate::project_registry::ProjectRegistryStore;
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
/// Where a batch is staged: a chat tab, or the new-chat draft before its
/// chat exists. Both use the same batch, caps and commands.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum StageOwner {
    Chat(FileOwner),
    Draft(DraftOwner),
}
type StageKey = (String, String, i64);
impl StageOwner {
    /// The batch key and its attachment identity, checked under the
    /// transition guard.
    fn key(&self, state: &AppState, generation: u64) -> Result<StageKey, String> {
        match self {
            Self::Chat(owner) => validate(state, owner, generation)
                .map(|at| (owner.project_id.clone(), owner.chat_id.clone(), at)),
            Self::Draft(draft) => draft.key(),
        }
    }
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
    owner: StageOwner,
    pick: PickKind,
    expected_revision: Option<String>,
}
/// Composer text too long to type inline. The WebView supplies the text the
/// person pasted, never a path, so nothing outside the paste is read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StageTextRequest {
    owner: StageOwner,
    expected_revision: Option<String>,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveFileRequest {
    owner: StageOwner,
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
    let attached = request
        .owner
        .key(state, *state.project_transitions.lock().await)?;
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
    if request.owner.key(state, *transition)? != attached {
        return Err("Attachment chat tab changed during selection.".into());
    }
    let (project, chat, attached) = attached;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .stage(
            &project,
            &chat,
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
    let (project, chat, attached) = request.owner.key(state, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .append_text(
            &project,
            &chat,
            attached,
            request.expected_revision.as_deref(),
            request.text,
        )
        .map_err(str::to_owned)
}
#[tauri::command]
pub(crate) async fn list_prompt_files(
    state: State<'_, AppState>,
    owner: StageOwner,
) -> Result<Option<StagedFiles>, String> {
    let transition = state.project_transitions.lock().await;
    let (project, chat, _) = owner.key(&state, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    Ok(state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .list(&project, &chat))
}
#[tauri::command]
pub(crate) async fn remove_prompt_file(
    state: State<'_, AppState>,
    request: RemoveFileRequest,
) -> Result<Option<StagedFiles>, String> {
    let transition = state.project_transitions.lock().await;
    let (project, chat, _) = request.owner.key(&state, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .remove(&project, &chat, &request.revision, &request.id)
        .map_err(str::to_owned)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HandOffFilesRequest {
    from: StageOwner,
    to: StageOwner,
    /// The revision the person last saw at `from`.
    revision: String,
}
/// Moves a draft's batch whole to the chat its first send created, or back
/// when that handoff fails, so the files go with exactly one first turn.
#[tauri::command]
pub(crate) async fn hand_off_prompt_files(
    state: State<'_, AppState>,
    request: HandOffFilesRequest,
) -> Result<StagedFiles, String> {
    hand_off(&state, request).await
}
pub(super) async fn hand_off(
    state: &AppState,
    request: HandOffFilesRequest,
) -> Result<StagedFiles, String> {
    let transition = state.project_transitions.lock().await;
    let from = request.from.key(state, *transition)?;
    let to = request.to.key(state, *transition)?;
    release_detached(&state.acp.file_stages, &state.database);
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .hand_off((&from.0, &from.1), (&to.0, &to.1, to.2), &request.revision)
        .map_err(str::to_owned)
}
/// A discarded or retargeted draft lets go of its files.
#[tauri::command]
pub(crate) fn release_draft_prompt_files(
    state: State<'_, AppState>,
    draft: DraftOwner,
) -> Result<(), String> {
    let (project, token, _) = draft.key()?;
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .release_draft(&project, &token);
    Ok(())
}

#[cfg(test)]
#[path = "draft_tests.rs"]
mod draft_tests;
#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
