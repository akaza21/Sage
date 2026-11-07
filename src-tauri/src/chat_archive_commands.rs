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
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat archive",
        move |database| database.archive_chat(&request),
    )
    .await;
    if result.committed {
        // An archived chat is off the rail, so its read mark goes with it; a
        // restored chat starts without one.
        if let Err(error) = state.settings.mark_chat_unread(&chat_id) {
            tracing::warn!(?error, "archived Chat keeps its read mark");
        }
    }
    if result.committed
        && let (Ok(project), Ok(chat)) = (
            crate::acp::contract::SageProjectId::parse(project_id),
            crate::acp::contract::SageChatId::parse(chat_id),
        )
    {
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
            state
                .settings
                .mark_chat_read(
                    &crate::settings_store::ChatReadMark {
                        chat_id: first_chat_id.clone(),
                        read_through: 5_000,
                    },
                    &[],
                )
                .unwrap();

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
            assert!(state.settings.chat_read_marks().unwrap().is_empty());
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
