use tauri::State;

use crate::{
    chat_commands::execute_chat_transition,
    domain::{ChatLifecycleRequest, ProjectTransitionResult},
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

async fn archive_chat_inner(
    state: &AppState,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat archive",
        move |database| database.archive_chat(&request),
    )
    .await
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
    use crate::{domain::ProjectLayoutRequest, persistence::Database};

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

            let archived = archive_chat_inner(&state, request.clone(), 0).await;
            assert!(archived.committed);
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
