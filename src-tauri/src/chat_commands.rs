use tauri::State;

use crate::{
    domain::{ProjectLayoutRequest, ProjectTransitionResult},
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

async fn create_chat_inner(
    state: &AppState,
    current_layout: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    let mut transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return failed_transition(
            *transition_epoch,
            "project changed before the requested Chat creation",
        );
    }
    if *transition_epoch >= MAX_TRANSITION_EPOCH {
        return failed_transition(*transition_epoch, "project transition epoch exhausted");
    }
    let database = state.database.clone();
    let mutation =
        tauri::async_runtime::spawn_blocking(move || database.create_chat(&current_layout))
            .await
            .map_err(|error| crate::error::AppError::InvalidRequest(error.to_string()))
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
    use std::{fs, path::Path};

    use uuid::Uuid;

    use super::*;
    use crate::{domain::ProjectLayoutRequest, persistence::Database, state::AppState};

    #[test]
    fn native_chat_creation_returns_the_committed_projection_and_rejects_stale_intents() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("sage-chat-command-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let database = Database::open(Path::new(":memory:")).unwrap();
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

            let stale = create_chat_inner(&state, layout, 0).await;
            assert!(!stale.committed);
            assert_eq!(stale.transition_epoch, 1);
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
            fs::remove_dir_all(root).unwrap();
        });
    }
}
