use super::{StagedFiles, read::read_paths};
use crate::state::AppState;
use serde::Deserialize;
use tauri::State;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FileOwner {
    project_id: String,
    chat_id: String,
    project_generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StageFilesRequest {
    owner: FileOwner,
    paths: crate::acp::contract::BoundedVec<crate::acp::contract::RequiredText<4096>, 16>,
    expected_revision: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveFileRequest {
    owner: FileOwner,
    revision: String,
    id: String,
}

fn validate(state: &AppState, owner: &FileOwner, generation: u64) -> Result<(), String> {
    if owner.project_id.len() > 512
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
    let present: bool = state
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_tabs WHERE project_id=?1 AND chat_id=?2)",
            rusqlite::params![owner.project_id, owner.chat_id],
            |row| row.get(0),
        )
        .map_err(|_| "Cannot validate attachment chat.")?;
    if !present {
        return Err("Attachment chat is unavailable.".into());
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn stage_prompt_files(
    state: State<'_, AppState>,
    request: StageFilesRequest,
) -> Result<StagedFiles, String> {
    let admission = std::sync::Arc::clone(&state.acp.file_stage_reader)
        .try_lock_owned()
        .map_err(|_| "Another attachment selection is being read.")?;
    let transition = state.project_transitions.lock().await;
    validate(&state, &request.owner, *transition)?;
    // One native reader at a time. No queued file buffers and no filesystem I/O
    // on the async runtime thread. The project guard prevents archive/switch races.
    let resources = tauri::async_runtime::spawn_blocking(move || {
        let _admission = admission;
        read_paths(
            &request
                .paths
                .as_slice()
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|_| "Attachment reader failed.")??;
    state
        .acp
        .file_stages
        .lock()
        .map_err(|_| "Attachment state is unavailable.")?
        .stage(
            &request.owner.project_id,
            &request.owner.chat_id,
            request.expected_revision.as_deref(),
            resources,
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
mod tests {
    use super::*;
    #[test]
    fn prompt_file_requests_enforce_path_count_and_byte_bounds() {
        let request = |paths| serde_json::json!({"owner":{"projectId":"p","chatId":"c","projectGeneration":0},"paths":paths,"expectedRevision":null});
        assert!(serde_json::from_value::<StageFilesRequest>(request(vec!["/picked"; 16])).is_ok());
        assert!(serde_json::from_value::<StageFilesRequest>(request(vec!["/picked"; 17])).is_err());
        assert!(
            serde_json::from_value::<StageFilesRequest>(request(vec![&"x".repeat(4097)])).is_err()
        );
    }
}
