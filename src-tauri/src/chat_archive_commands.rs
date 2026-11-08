use tauri::State;

use crate::{
    acp::{
        contract::{SageChatId, SageProjectId},
        service::ChatRuntimeRelease,
    },
    chat_commands::{execute_chat_transition, failed_transition},
    domain::{ChatDeleteRequest, ChatLifecycleRequest, ProjectTransitionResult},
    state::AppState,
};

#[tauri::command]
pub async fn archive_chat(
    state: State<'_, AppState>,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(archive_chat_inner(&state, request, expected_transition_epoch).await)
}

#[tauri::command]
pub async fn restore_chat(
    state: State<'_, AppState>,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(restore_chat_inner(&state, request, expected_transition_epoch).await)
}

/// Permanently deletes an archived Chat and forgets its read mark.
#[tauri::command]
pub async fn delete_chat(
    state: State<'_, AppState>,
    request: ChatDeleteRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(delete_chat_inner(&state, request, expected_transition_epoch).await)
}

/// Parks the Chat's live runtime through the lanes' explicit-close park,
/// handing this thread to Tokio's blocking pool while the harness exits.
fn release_chat_runtime(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<(), ChatRuntimeRelease> {
    let release = || state.acp.release_chat_runtime(project, chat);
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(release)
    } else {
        release()
    }
}

/// The Chat's typed identity when it is an archived Chat of that Project.
fn archived_identity(
    state: &AppState,
    request: &ChatDeleteRequest,
) -> Option<(SageProjectId, SageChatId)> {
    let archived = crate::chat_archive::is_archived_chat(
        &state.database.connection(),
        &request.project_id,
        &request.chat_id,
    )
    .unwrap_or(false);
    archived
        .then(|| {
            Some((
                SageProjectId::parse(request.project_id.clone()).ok()?,
                SageChatId::parse(request.chat_id.clone()).ok()?,
            ))
        })
        .flatten()
}

async fn delete_chat_inner(
    state: &AppState,
    request: ChatDeleteRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    // An archived Chat may still hold an idle runtime from before it was
    // archived; release it the way archive does instead of refusing. Only
    // an archived Chat's runtime is touched; anything else is refused below.
    if let Some((project, chat)) = archived_identity(state, &request)
        && let Err(refusal) = release_chat_runtime(state, &project, &chat)
    {
        let epoch = *state.project_transitions.lock().await;
        return failed_transition(epoch, refusal.message("deleting"));
    }
    let chat_id = request.chat_id.clone();
    let result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat delete",
        move |database| database.delete_archived_chat(&request),
    )
    .await;
    if result.committed
        && let Err(error) = state.settings.mark_chat_unread(&chat_id)
    {
        tracing::warn!(?error, "deleted Chat's read mark remains");
    }
    result
}

async fn archive_chat_inner(
    state: &AppState,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat archive",
        move |database| database.archive_chat(&request),
    )
    .await;
    if result.committed
        && let (Ok(project), Ok(chat)) =
            (SageProjectId::parse(project_id), SageChatId::parse(chat_id))
    {
        // An archived Chat holds no harness process. Archive refused a running
        // Turn, so this parks an idle runtime; a park that does not settle in
        // time is retried when the Chat is deleted.
        if let Err(refusal) = release_chat_runtime(state, &project, &chat) {
            tracing::warn!(?refusal, "archived Chat's runtime is not yet released");
        }
        if let Err(error) = state.terminals.close_chat(&project, &chat) {
            tracing::warn!(?error, "terminal Chat-close cleanup remains unsettled");
        }
        state
            .acp
            .file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_chat(project.as_str(), chat.as_str());
        state.acp.revoke_browser_chat(&project, &chat);
    }
    result
}

async fn restore_chat_inner(
    state: &AppState,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat restore",
        move |database| database.restore_chat(&request),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::{
        acp::contract::{SageChatId, SageProjectId},
        domain::ProjectLayoutRequest,
        persistence::Database,
    };

    #[test]
    fn native_archive_and_restore_are_epoch_checked_transitions() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root =
                std::env::temp_dir().join(format!("sage-chat-command-archive-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let database = Database::open(&root.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let initial = database.bootstrap_snapshot().unwrap();
            let first_chat_id = initial.active_tab_id.clone().unwrap();
            database
                .create_chat(&ProjectLayoutRequest {
                    project_id: project.id.clone(),
                    tabs: initial.tabs,
                    active_tab_id: initial.active_tab_id,
                    selected_root_id: initial.selected_root_id,
                    selected_resource: initial.selected_resource,
                })
                .unwrap();
            let attached = database.bootstrap_snapshot().unwrap();
            let request = ChatLifecycleRequest {
                project_id: project.id.clone(),
                chat_id: first_chat_id.clone(),
                current_layout: ProjectLayoutRequest {
                    project_id: project.id.clone(),
                    tabs: attached.tabs,
                    active_tab_id: attached.active_tab_id,
                    selected_root_id: attached.selected_root_id,
                    selected_resource: attached.selected_resource,
                },
            };
            let state = AppState::new(database);

            let browser_project = SageProjectId::parse(project.id.clone()).unwrap();
            let browser_chat = SageChatId::parse(first_chat_id.clone()).unwrap();
            state
                .acp
                .attach_browser_tab(
                    browser_project.clone(),
                    browser_chat.clone(),
                    "browser-7",
                    "https://example.com",
                    1,
                )
                .unwrap();
            let browser_lease = state
                .acp
                .browser_attachment(&browser_project, &browser_chat)
                .unwrap();

            state
                .acp
                .file_stages
                .lock()
                .unwrap()
                .stage(
                    &project.id,
                    &first_chat_id,
                    crate::acp::attached_context::files::attachment(
                        &state.database,
                        &project.id,
                        &first_chat_id,
                    )
                    .unwrap()
                    .unwrap(),
                    None,
                    crate::acp::attached_context::files::Resources(std::sync::Arc::new(vec![
                        crate::acp::attached_context::files::Resource {
                            id: "file-archive".into(),
                            name: "notes.txt".into(),
                            bytes: b"context".to_vec(),
                        },
                    ])),
                )
                .unwrap();
            let archived = archive_chat_inner(&state, request.clone(), 0).await;
            assert!(
                state
                    .acp
                    .file_stages
                    .lock()
                    .unwrap()
                    .get(&project.id, &first_chat_id)
                    .0
                    .is_empty()
            );
            assert!(archived.committed);
            assert!(!browser_lease.is_live());
            assert!(
                state
                    .acp
                    .browser_attachment(&browser_project, &browser_chat)
                    .is_none()
            );
            assert_eq!(archived.transition_epoch, 1);
            let archived_snapshot = archived.snapshot.unwrap();
            assert_eq!(archived_snapshot.archived_chats[0].id, first_chat_id);
            let restore_request = ChatLifecycleRequest {
                project_id: project.id,
                chat_id: first_chat_id,
                current_layout: ProjectLayoutRequest {
                    project_id: request.project_id,
                    tabs: archived_snapshot.tabs,
                    active_tab_id: archived_snapshot.active_tab_id,
                    selected_root_id: archived_snapshot.selected_root_id,
                    selected_resource: archived_snapshot.selected_resource,
                },
            };
            let stale = restore_chat_inner(&state, restore_request.clone(), 0).await;
            assert!(!stale.committed);
            assert_eq!(stale.transition_epoch, 1);
            let restored = restore_chat_inner(&state, restore_request, 1).await;
            assert!(restored.committed);
            assert_eq!(restored.transition_epoch, 2);
            assert!(restored.snapshot.unwrap().archived_chats.is_empty());
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
    }
}
