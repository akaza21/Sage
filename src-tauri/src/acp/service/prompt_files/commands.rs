use super::{Resources, StagedFiles, read::read_paths};
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
    stage_with(&state, request, read_paths).await
}

/// Reads the picked paths without holding the project transition guard, so a
/// slow folder or a large bounded walk never blocks project switching, chat
/// selection or shutdown persistence. Ownership is checked before the read and
/// again under the guard before staging: a switch, archive or chat close during
/// the read rejects the snapshot instead of admitting it to the new state.
async fn stage_with<R>(
    state: &AppState,
    request: StageFilesRequest,
    read: R,
) -> Result<StagedFiles, String>
where
    R: FnOnce(&[String]) -> Result<Resources, &'static str> + Send + 'static,
{
    let admission = std::sync::Arc::clone(&state.acp.file_stage_reader)
        .try_lock_owned()
        .map_err(|_| "Another attachment selection is being read.")?;
    validate(
        state,
        &request.owner,
        *state.project_transitions.lock().await,
    )?;
    let paths = request
        .paths
        .as_slice()
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect::<Vec<_>>();
    // One native reader at a time. No queued file buffers and no filesystem I/O
    // on the async runtime thread.
    let resources = tauri::async_runtime::spawn_blocking(move || {
        let _admission = admission;
        read(&paths)
    })
    .await
    .map_err(|_| "Attachment reader failed.")??;
    let transition = state.project_transitions.lock().await;
    validate(state, &request.owner, *transition)?;
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

    #[test]
    fn slow_reads_release_the_project_guard_and_stale_snapshots_are_not_staged() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("sage-stage-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let database =
                crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let chat = database
                .bootstrap_snapshot()
                .unwrap()
                .active_tab_id
                .unwrap();
            let state = AppState::new(database);
            let request = || {
                serde_json::from_value::<StageFilesRequest>(serde_json::json!({
                    "owner": {"projectId": project.id, "chatId": chat, "projectGeneration": 0},
                    "paths": ["/picked"],
                    "expectedRevision": null,
                }))
                .unwrap()
            };
            let snapshot = || {
                Resources(std::sync::Arc::new(vec![super::super::Resource {
                    id: "file-1".into(),
                    name: "picked.txt".into(),
                    bytes: b"context".to_vec(),
                }]))
            };
            for switch_during_read in [false, true] {
                *state.project_transitions.lock().await = 0;
                let (started, read_started) = tokio::sync::oneshot::channel();
                let (release, released) = std::sync::mpsc::channel::<()>();
                let reader = move |_: &[String]| {
                    started.send(()).unwrap();
                    released.recv().unwrap();
                    Ok(snapshot())
                };
                let transition = async {
                    read_started.await.unwrap();
                    // The guard is free while the reader is blocked in I/O.
                    let mut epoch = state.project_transitions.try_lock().unwrap();
                    if switch_during_read {
                        *epoch += 1;
                    }
                    drop(epoch);
                    release.send(()).unwrap();
                };
                let (staged, ()) = tokio::join!(stage_with(&state, request(), reader), transition);
                let listed = state
                    .acp
                    .file_stages
                    .lock()
                    .unwrap()
                    .list(&project.id, &chat);
                if switch_during_read {
                    assert_eq!(
                        staged.err().as_deref(),
                        Some("Attachment owner is stale or invalid.")
                    );
                    assert!(listed.is_none());
                } else {
                    assert_eq!(staged.err(), None);
                    assert!(listed.is_some());
                    state.acp.file_stages.lock().unwrap().clear();
                }
            }
            let _ = state.acp.shutdown_blocking();
            let _ = std::fs::remove_dir_all(root);
        });
    }
}
