use std::path::PathBuf;

#[cfg(test)]
use crate::persistence::Database;
use crate::{
    acp::contract::SageProjectId,
    domain::{ProjectLayoutRequest, ProjectSwitchRequest, ProjectTransitionResult},
    error::AppResult,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

const MAX_TRANSITION_EPOCH: u64 = 9_007_199_254_740_991;

#[tauri::command]
pub async fn create_project_from_picker(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    expected_active_project_id: Option<String>,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Open folder as project").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition(&state).await);
    };
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(
        execute_committed_transition(&state, expected_transition_epoch, move || {
            operations.execute_project_transition(ticket, || {
                database.create_project_from_root_if_active(
                    &root,
                    expected_active_project_id.as_deref(),
                )
            })
        })
        .await,
    )
}

#[tauri::command]
pub async fn add_project_root_from_picker(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    project_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Add folder root").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition(&state).await);
    };
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(
        execute_committed_transition(&state, expected_transition_epoch, move || {
            operations.execute_project_transition(ticket, || {
                database.add_project_root(&project_id, &root)
            })
        })
        .await,
    )
}

#[tauri::command]
pub fn persist_project_layout(
    state: State<'_, AppState>,
    request: ProjectLayoutRequest,
) -> Result<(), String> {
    state
        .database
        .persist_project_layout(&request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn persist_project_layout_and_close(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request: Option<ProjectLayoutRequest>,
) -> Result<(), String> {
    if let Some(request) = request {
        state
            .database
            .persist_project_layout(&request)
            .map_err(|error| error.to_string())?;
    }
    window.destroy().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn switch_project(
    state: State<'_, AppState>,
    request: ProjectSwitchRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(
        execute_committed_transition(&state, expected_transition_epoch, move || {
            operations.execute_project_transition(ticket, || database.switch_project(&request))
        })
        .await,
    )
}

async fn execute_committed_transition<T: Send + 'static>(
    state: &AppState,
    expected_transition_epoch: u64,
    operation: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> ProjectTransitionResult {
    let mut transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return failed_transition(
            *transition_epoch,
            "project changed before the requested transition",
        );
    }
    if *transition_epoch >= MAX_TRANSITION_EPOCH {
        return failed_transition(*transition_epoch, "project transition epoch exhausted");
    }
    let committed_epoch = *transition_epoch + 1;
    let mutation = tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| crate::error::AppError::InvalidRequest(error.to_string()))
        .and_then(std::convert::identity);
    if let Err(error) = mutation {
        return failed_transition(*transition_epoch, error);
    }
    *transition_epoch = committed_epoch;
    if let Ok(Some(active_project)) = state.database.active_project_id()
        && let Ok(active_project) = SageProjectId::parse(active_project)
    {
        state.acp.project_switched(&active_project);
    }
    committed_projection(state, committed_epoch).await
}

async fn committed_projection(state: &AppState, transition_epoch: u64) -> ProjectTransitionResult {
    match build_bootstrap_snapshot(state, transition_epoch).await {
        Ok(snapshot) => ProjectTransitionResult {
            committed: true,
            transition_epoch,
            snapshot: Some(snapshot),
            recovery_error: None,
        },
        Err(error) => ProjectTransitionResult {
            committed: true,
            transition_epoch,
            snapshot: None,
            recovery_error: Some(error.to_string()),
        },
    }
}

async fn cancelled_transition(state: &AppState) -> ProjectTransitionResult {
    let transition_epoch = *state.project_transitions.lock().await;
    ProjectTransitionResult {
        committed: false,
        transition_epoch,
        snapshot: None,
        recovery_error: None,
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

async fn pick_folder(
    app: &AppHandle,
    window: &WebviewWindow,
    title: &str,
) -> Result<Option<PathBuf>, String> {
    let dialog = app
        .dialog()
        .file()
        .set_parent(window)
        .set_title(title.to_owned());
    let selection = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|error| format!("native folder picker task failed: {error}"))?;
    selection
        .map(|path| path.into_path().map_err(|error| error.to_string()))
        .transpose()
}

#[cfg(test)]
fn apply_create_selection(database: &Database, selected: Option<PathBuf>) -> AppResult<bool> {
    let Some(root) = selected else {
        return Ok(false);
    };
    database.create_project_from_root(&root)?;
    Ok(true)
}

#[cfg(test)]
fn apply_add_root_selection(
    database: &Database,
    project_id: &str,
    selected: Option<PathBuf>,
) -> AppResult<bool> {
    let Some(root) = selected else {
        return Ok(false);
    };
    database.add_project_root(project_id, &root)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, mpsc},
        time::Duration,
    };

    use super::*;
    use uuid::Uuid;

    #[test]
    fn picker_cancellation_does_not_create_or_mutate_projects() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        assert!(!apply_create_selection(&database, None).unwrap());
        assert!(!apply_add_root_selection(&database, "unused", None).unwrap());
        assert_eq!(database.project_count().unwrap(), 0);
    }

    #[test]
    fn serialized_transition_owns_its_authoritative_projection() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let roots =
                std::env::temp_dir().join(format!("sage-transition-projection-{}", Uuid::new_v4()));
            let first_root = roots.join("first");
            let second_root = roots.join("second");
            std::fs::create_dir_all(&first_root).unwrap();
            std::fs::create_dir_all(&second_root).unwrap();
            let state = Arc::new(AppState::new(
                Database::open(Path::new(":memory:")).unwrap(),
            ));
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();

            let first_state = state.clone();
            let first_database = state.database.clone();
            let first = tokio::spawn(async move {
                execute_committed_transition(&first_state, 0, move || {
                    first_database.create_project_from_root(&first_root)?;
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                })
                .await
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

            let second_state = state.clone();
            let second_database = state.database.clone();
            let second = tokio::spawn(async move {
                execute_committed_transition(&second_state, 1, move || {
                    second_database.create_project_from_root(&second_root)
                })
                .await
            });
            tokio::task::yield_now().await;
            release_tx.send(()).unwrap();
            let first = first.await.unwrap();
            let second = second.await.unwrap();

            assert!(first.committed);
            assert_eq!(first.transition_epoch, 1);
            assert_eq!(first.snapshot.unwrap().project.unwrap().name, "first");
            assert!(second.committed);
            assert_eq!(second.transition_epoch, 2);
            assert_eq!(second.snapshot.unwrap().project.unwrap().name, "second");
            std::fs::remove_dir_all(roots).unwrap();
        });
    }

    #[test]
    fn transition_epoch_rejects_a_stale_aba_intent_before_mutation() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let state = AppState::new(Database::open(Path::new(":memory:")).unwrap());
            let roots =
                std::env::temp_dir().join(format!("sage-transition-aba-{}", Uuid::new_v4()));
            let first_root = roots.join("project-a");
            let second_root = roots.join("project-b");
            std::fs::create_dir_all(&first_root).unwrap();
            std::fs::create_dir_all(&second_root).unwrap();
            let project_a = state
                .database
                .create_project_from_root(&first_root)
                .unwrap();
            let database = state.database.clone();
            let to_b = execute_committed_transition(&state, 0, move || {
                database.create_project_from_root(&second_root)
            })
            .await;
            let project_b = to_b.snapshot.unwrap();
            let database = state.database.clone();
            let to_a = execute_committed_transition(&state, 1, move || {
                database.switch_project(&ProjectSwitchRequest {
                    target_project_id: project_a.id.clone(),
                    current_layout: ProjectLayoutRequest {
                        project_id: project_b.project.unwrap().id,
                        tabs: project_b.tabs,
                        active_tab_id: project_b.active_tab_id,
                        selected_root_id: project_b.selected_root_id,
                        selected_resource: project_b.selected_resource,
                    },
                })
            })
            .await;
            assert!(to_a.committed);
            let active_project_id = to_a.snapshot.unwrap().project.unwrap().id;

            let stale = execute_committed_transition(&state, 0, || -> AppResult<()> {
                panic!("stale ABA intent reached the mutation")
            })
            .await;
            assert!(!stale.committed);
            assert_eq!(stale.transition_epoch, 2);
            assert!(stale.recovery_error.unwrap().contains("project changed"));
            assert_eq!(
                state.database.active_project_id().unwrap().unwrap(),
                active_project_id
            );
            std::fs::remove_dir_all(roots).unwrap();
        });
    }

    #[test]
    fn committed_projection_failure_preserves_the_advanced_epoch() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let state = AppState::new(Database::open(Path::new(":memory:")).unwrap());
            let database = state.database.clone();
            let root =
                std::env::temp_dir().join(format!("sage-transition-recovery-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let operation_root = root.clone();
            let result = execute_committed_transition(&state, 0, move || {
                database.create_project_from_root(&operation_root)?;
                database
                    .connection()
                    .execute_batch("DROP TABLE workbench_tabs")?;
                Ok(())
            })
            .await;

            assert!(result.committed);
            assert_eq!(result.transition_epoch, 1);
            assert!(result.snapshot.is_none());
            assert!(result.recovery_error.is_some());
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}
