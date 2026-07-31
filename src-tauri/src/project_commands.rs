use std::path::PathBuf;

#[cfg(test)]
use crate::persistence::Database;
use crate::{
    domain::{ProjectLayoutRequest, ProjectSwitchRequest, ProjectTransitionResult},
    error::AppResult,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn create_project_from_picker(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    expected_active_project_id: Option<String>,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Open folder as project").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition());
    };
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    let mutation = execute_serialized_transition(&state, move || {
        operations.execute_project_transition(ticket, || {
            database
                .create_project_from_root_if_active(&root, expected_active_project_id.as_deref())
        })
    })
    .await;
    if let Err(error) = mutation {
        return Ok(failed_transition(error));
    }
    Ok(committed_projection(&state).await)
}

#[tauri::command]
pub async fn add_project_root_from_picker(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Add folder root").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition());
    };
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    let mutation = execute_serialized_transition(&state, move || {
        operations
            .execute_project_transition(ticket, || database.add_project_root(&project_id, &root))
    })
    .await;
    if let Err(error) = mutation {
        return Ok(failed_transition(error));
    }
    Ok(committed_projection(&state).await)
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
) -> Result<ProjectTransitionResult, String> {
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    let mutation = execute_serialized_transition(&state, move || {
        operations.execute_project_transition(ticket, || database.switch_project(&request))
    })
    .await;
    if let Err(error) = mutation {
        return Ok(failed_transition(error));
    }
    Ok(committed_projection(&state).await)
}

async fn execute_serialized_transition<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let _transition = state.project_transitions.lock().await;
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| crate::error::AppError::InvalidRequest(error.to_string()))?
}

async fn committed_projection(state: &AppState) -> ProjectTransitionResult {
    match build_bootstrap_snapshot(state).await {
        Ok(snapshot) => ProjectTransitionResult {
            committed: true,
            snapshot: Some(snapshot),
            recovery_error: None,
        },
        Err(error) => ProjectTransitionResult {
            committed: true,
            snapshot: None,
            recovery_error: Some(error.to_string()),
        },
    }
}

fn cancelled_transition() -> ProjectTransitionResult {
    ProjectTransitionResult {
        committed: false,
        snapshot: None,
        recovery_error: None,
    }
}

fn failed_transition(error: impl ToString) -> ProjectTransitionResult {
    ProjectTransitionResult {
        committed: false,
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
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use super::*;

    #[test]
    fn picker_cancellation_does_not_create_or_mutate_projects() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        assert!(!apply_create_selection(&database, None).unwrap());
        assert!(!apply_add_root_selection(&database, "unused", None).unwrap());
        assert_eq!(database.project_count().unwrap(), 0);
    }

    #[test]
    fn create_add_and_switch_mutations_share_one_serialized_gate() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let state = Arc::new(AppState::new(
                Database::open(Path::new(":memory:")).unwrap(),
            ));
            let active = Arc::new(AtomicUsize::new(0));
            let maximum = Arc::new(AtomicUsize::new(0));
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();

            let first_state = state.clone();
            let first_active = active.clone();
            let first_maximum = maximum.clone();
            let first = tokio::spawn(async move {
                execute_serialized_transition(&first_state, move || {
                    let current = first_active.fetch_add(1, Ordering::SeqCst) + 1;
                    first_maximum.fetch_max(current, Ordering::SeqCst);
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    first_active.fetch_sub(1, Ordering::SeqCst);
                    Ok("create")
                })
                .await
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

            let mut followers = Vec::new();
            for name in ["add-root", "switch"] {
                let follower_state = state.clone();
                let follower_active = active.clone();
                let follower_maximum = maximum.clone();
                followers.push(tokio::spawn(async move {
                    execute_serialized_transition(&follower_state, move || {
                        let current = follower_active.fetch_add(1, Ordering::SeqCst) + 1;
                        follower_maximum.fetch_max(current, Ordering::SeqCst);
                        follower_active.fetch_sub(1, Ordering::SeqCst);
                        Ok(name)
                    })
                    .await
                }));
            }
            tokio::task::yield_now().await;
            assert_eq!(maximum.load(Ordering::SeqCst), 1);
            release_tx.send(()).unwrap();
            assert_eq!(first.await.unwrap().unwrap(), "create");
            for follower in followers {
                follower.await.unwrap().unwrap();
            }
            assert_eq!(maximum.load(Ordering::SeqCst), 1);
        });
    }
}
