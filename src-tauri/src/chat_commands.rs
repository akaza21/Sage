use tauri::State;

use crate::{
    domain::{ProjectLayoutRequest, ProjectTransitionResult, RenameChatRequest},
    error::{AppError, AppResult},
    persistence::Database,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
};

const MAX_TRANSITION_EPOCH: u64 = 9_007_199_254_740_991;

#[tauri::command]
pub async fn create_chat(
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(create_chat_inner(&state, current_layout, expected_transition_epoch).await)
}

#[tauri::command]
pub async fn activate_chat(
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(activate_chat_inner(&state, current_layout, expected_transition_epoch).await)
}

#[tauri::command]
pub async fn rename_chat(
    state: State<'_, AppState>,
    request: RenameChatRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(rename_chat_inner(&state, request, expected_transition_epoch).await)
}

async fn create_chat_inner(
    state: &AppState,
    current_layout: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat creation",
        move |database| database.create_chat(&current_layout).map(drop),
    )
    .await
}

async fn activate_chat_inner(
    state: &AppState,
    current_layout: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat activation",
        move |database| {
            let Some(active_chat_id) = current_layout.active_tab_id.as_deref() else {
                return Err(AppError::InvalidRequest(
                    "Chat activation requires an active Chat tab".into(),
                ));
            };
            if !current_layout
                .tabs
                .iter()
                .any(|tab| tab.id == active_chat_id && tab.kind == "chat")
            {
                return Err(AppError::InvalidRequest(
                    "Chat activation target is not an attached Chat".into(),
                ));
            }
            database.persist_project_layout(&current_layout)
        },
    )
    .await
}

async fn rename_chat_inner(
    state: &AppState,
    request: RenameChatRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat rename",
        move |database| database.rename_chat(&request),
    )
    .await
}

async fn execute_chat_transition(
    state: &AppState,
    expected_transition_epoch: u64,
    stale_message: &'static str,
    operation: impl FnOnce(Database) -> AppResult<()> + Send + 'static,
) -> ProjectTransitionResult {
    let mut transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return failed_transition(*transition_epoch, stale_message);
    }
    if *transition_epoch >= MAX_TRANSITION_EPOCH {
        return failed_transition(*transition_epoch, "project transition epoch exhausted");
    }
    let database = state.database.clone();
    let mutation = tauri::async_runtime::spawn_blocking(move || operation(database))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))
        .and_then(std::convert::identity);
    if let Err(error) = mutation {
        return failed_transition(*transition_epoch, error);
    }
    *transition_epoch += 1;
    match build_bootstrap_snapshot(state, *transition_epoch).await {
        Ok(snapshot) => ProjectTransitionResult {
            committed: true,
            transition_epoch: *transition_epoch,
            snapshot: Some(snapshot),
            recovery_error: None,
        },
        Err(error) => ProjectTransitionResult {
            committed: true,
            transition_epoch: *transition_epoch,
            snapshot: None,
            recovery_error: Some(error.to_string()),
        },
    }
}

fn failed_transition(transition_epoch: u64, error: impl ToString) -> ProjectTransitionResult {
    ProjectTransitionResult {
        committed: false,
        transition_epoch,
        snapshot: None,
        recovery_error: Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::{
        domain::{ProjectLayoutRequest, RenameChatRequest},
        persistence::Database,
        state::AppState,
    };

    #[test]
    fn native_chat_creation_returns_the_committed_projection_and_rejects_stale_intents() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("sage-chat-command-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let database_path = root.join("sage.db");
            let database = Database::open(&database_path).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let initial = database.bootstrap_snapshot().unwrap();
            let layout = ProjectLayoutRequest {
                project_id: project.id,
                tabs: initial.tabs,
                active_tab_id: initial.active_tab_id,
                selected_root_id: initial.selected_root_id,
                selected_resource: initial.selected_resource,
            };
            let state = AppState::new(database);

            let created = create_chat_inner(&state, layout.clone(), 0).await;
            assert!(created.committed);
            assert_eq!(created.transition_epoch, 1);
            let snapshot = created.snapshot.unwrap();
            assert_eq!(snapshot.tabs.len(), 2);
            assert_eq!(
                snapshot.active_tab_id,
                snapshot.tabs.last().map(|tab| tab.id.clone())
            );
            let second_chat_id = snapshot.active_tab_id.clone().unwrap();

            let first_chat_id = layout.active_tab_id.clone().unwrap();
            let selected = activate_chat_inner(
                &state,
                ProjectLayoutRequest {
                    project_id: layout.project_id.clone(),
                    tabs: snapshot.tabs.clone(),
                    active_tab_id: Some(first_chat_id.clone()),
                    selected_root_id: snapshot.selected_root_id,
                    selected_resource: snapshot.selected_resource,
                },
                1,
            )
            .await;
            assert!(selected.committed);
            assert_eq!(selected.transition_epoch, 2);
            let selected = selected.snapshot.unwrap();
            assert_eq!(
                selected.active_tab_id.as_deref(),
                Some(first_chat_id.as_str())
            );
            assert_eq!(selected.messages.len(), 1);

            let renamed = rename_chat_inner(
                &state,
                RenameChatRequest {
                    project_id: layout.project_id.clone(),
                    chat_id: second_chat_id.clone(),
                    title: "  Inactive renamed  ".into(),
                },
                2,
            )
            .await;
            assert!(renamed.committed);
            assert_eq!(renamed.transition_epoch, 3);
            let renamed_snapshot = renamed.snapshot.unwrap();
            assert_eq!(
                renamed_snapshot.active_tab_id.as_deref(),
                Some(first_chat_id.as_str())
            );
            assert_eq!(renamed_snapshot.messages.len(), 1);
            assert_eq!(
                renamed_snapshot
                    .tabs
                    .iter()
                    .find(|tab| tab.id == second_chat_id)
                    .map(|tab| tab.title.as_str()),
                Some("Chat: Inactive renamed")
            );

            let stale_rename = rename_chat_inner(
                &state,
                RenameChatRequest {
                    project_id: layout.project_id.clone(),
                    chat_id: second_chat_id.clone(),
                    title: "Must not commit".into(),
                },
                2,
            )
            .await;
            assert!(!stale_rename.committed);
            assert_eq!(stale_rename.transition_epoch, 3);
            assert_eq!(
                state
                    .database
                    .connection()
                    .query_row(
                        "SELECT title FROM chats WHERE id=?1",
                        [&second_chat_id],
                        |row| row.get::<_, String>(0)
                    )
                    .unwrap(),
                "Chat: Inactive renamed"
            );

            let stale = create_chat_inner(&state, layout, 0).await;
            assert!(!stale.committed);
            assert_eq!(stale.transition_epoch, 3);
            assert!(stale.recovery_error.unwrap().contains("project changed"));
            assert_eq!(
                state
                    .database
                    .connection()
                    .query_row("SELECT COUNT(*) FROM chats", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                2
            );
            drop(state);

            let reopened = Database::open(&database_path).unwrap();
            let restarted = reopened.bootstrap_snapshot().unwrap();
            assert_eq!(
                restarted.active_tab_id.as_deref(),
                Some(first_chat_id.as_str())
            );
            assert_eq!(restarted.messages.len(), 1);
            assert_eq!(
                restarted
                    .tabs
                    .iter()
                    .find(|tab| tab.id == second_chat_id)
                    .map(|tab| tab.title.as_str()),
                Some("Chat: Inactive renamed")
            );
            drop(reopened);
            fs::remove_dir_all(root).unwrap();
        });
    }
}
