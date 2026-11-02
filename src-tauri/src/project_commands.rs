use std::path::PathBuf;

#[cfg(test)]
use crate::persistence::Database;
use crate::{
    acp::contract::SageProjectId,
    browser::BrowserState,
    domain::{ProjectLayoutRequest, ProjectSwitchRequest, ProjectTransitionResult},
    error::{AppError, AppResult},
    preview_lifecycle::on_committed_project_transition_epoch,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
};
use tauri::{AppHandle, State, Window};
use tauri_plugin_dialog::DialogExt;
use tracing::info;

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum CloseStage {
    CloseRequested,
    EditorFlush,
    AppearanceFlush,
    WorkspaceUnwatch,
    PersistAndDestroy,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum CloseStageStatus {
    Start,
    End,
    Error,
}

#[tauri::command]
pub fn trace_close_stage(stage: CloseStage, status: CloseStageStatus) {
    info!(?stage, ?status, "Sage window close stage");
}

const MAX_TRANSITION_EPOCH: u64 = 9_007_199_254_740_991;

#[tauri::command]
pub async fn create_project_from_picker(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    expected_active_project_id: Option<String>,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Open folder as project").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition(&state).await);
    };
    let database = state.database.clone();
    let preflight_root = root.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(execute_committed_transition_checked(
        &state,
        expected_transition_epoch,
        || {
            browser
                .validate_project_roots(std::slice::from_ref(&preflight_root))
                .map_err(AppError::InvalidRequest)
        },
        move || {
            operations.execute_project_transition(ticket, || {
                database.create_project_from_root_if_active(
                    &root,
                    expected_active_project_id.as_deref(),
                )
            })
        },
    )
    .await)
}

#[tauri::command]
pub async fn add_project_root_from_picker(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    project_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let selected = pick_folder(&app, &window, "Add folder root").await?;
    let Some(root) = selected else {
        return Ok(cancelled_transition(&state).await);
    };
    let database = state.database.clone();
    let preflight_root = root.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(execute_committed_transition_checked(
        &state,
        expected_transition_epoch,
        || {
            browser
                .validate_project_roots(std::slice::from_ref(&preflight_root))
                .map_err(AppError::InvalidRequest)
        },
        move || {
            operations.execute_project_transition(ticket, || {
                database.add_project_root(&project_id, &root)
            })
        },
    )
    .await)
}

#[tauri::command]
pub async fn ensure_local_project(
    state: State<'_, AppState>,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(
        execute_committed_transition(&state, expected_transition_epoch, move || {
            operations.execute_project_transition(ticket, || database.ensure_local_project())
        })
        .await,
    )
}

#[tauri::command]
pub async fn persist_project_layout(
    state: State<'_, AppState>,
    request: ProjectLayoutRequest,
    expected_transition_epoch: u64,
) -> Result<(), String> {
    persist_project_layout_inner(&state, Some(request), expected_transition_epoch)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn persist_project_layout_and_close(
    window: Window,
    state: State<'_, AppState>,
    request: Option<ProjectLayoutRequest>,
    expected_transition_epoch: u64,
) -> Result<(), String> {
    info!("Sage window close persist start");
    if let Err(error) =
        persist_project_layout_inner(&state, request, expected_transition_epoch).await
    {
        info!("Sage window close persist error");
        return Err(error.to_string());
    }
    info!("Sage window close persist end");
    info!("Sage window destroy start");
    let result = window.destroy().map_err(|error| error.to_string());
    info!(
        status = if result.is_ok() { "end" } else { "error" },
        "Sage window destroy"
    );
    result
}

pub(crate) async fn persist_project_layout_inner(
    state: &AppState,
    request: Option<ProjectLayoutRequest>,
    expected_transition_epoch: u64,
) -> AppResult<()> {
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(crate::error::AppError::InvalidRequest(
            "project changed before layout persistence could run".into(),
        ));
    }
    let Some(request) = request else {
        return Ok(());
    };
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.persist_project_layout(&request))
        .await
        .map_err(|error| crate::error::AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
pub async fn switch_project(
    state: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    request: ProjectSwitchRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let database = state.database.clone();
    let preflight_database = database.clone();
    let target_project_id = request.target_project_id.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    Ok(execute_committed_transition_checked(
        &state,
        expected_transition_epoch,
        || {
            let roots = preflight_database
                .root_records(&target_project_id)?
                .into_iter()
                .map(|root| root.absolute_path)
                .collect::<Vec<_>>();
            browser
                .validate_project_roots(&roots)
                .map_err(AppError::InvalidRequest)
        },
        move || operations.execute_project_transition(ticket, || database.switch_project(&request)),
    )
    .await)
}

async fn execute_committed_transition<T: Send + 'static>(
    state: &AppState,
    expected_transition_epoch: u64,
    operation: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> ProjectTransitionResult {
    execute_committed_transition_checked(state, expected_transition_epoch, || Ok(()), operation)
        .await
}

async fn execute_committed_transition_checked<T: Send + 'static>(
    state: &AppState,
    expected_transition_epoch: u64,
    preflight: impl FnOnce() -> AppResult<()>,
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
    if let Err(error) = preflight() {
        return failed_transition(*transition_epoch, error);
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
    on_committed_project_transition_epoch(*transition_epoch);
    let active_after = state.database.active_project_id().ok().flatten();
    let terminal_cleanup = active_after
        .and_then(|id| SageProjectId::parse(id).ok())
        .zip(crate::acp::contract::ProjectGeneration::new(committed_epoch).ok())
        .map_or_else(
            || state.terminals.shutdown(),
            |(project, generation)| state.terminals.project_changed(&project, generation),
        );
    let terminal_recovery_error = terminal_cleanup
        .err()
        .map(|_| "terminal cleanup did not converge; restart Sage before retrying".to_owned());
    let acp_recovery_error = if let Ok(Some(active_project)) = state.database.active_project_id()
        && let Ok(active_project) = SageProjectId::parse(active_project)
    {
        state.acp.project_switched(&active_project).err().map(|_| {
            "Codex discovery cleanup did not converge; restart Sage before retrying.".to_owned()
        })
    } else {
        None
    };
    let mut result = committed_projection(state, committed_epoch).await;
    if result.recovery_error.is_none() {
        result.recovery_error = terminal_recovery_error.or(acp_recovery_error);
    }
    result
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
    window: &Window,
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
    fn window_commands_accept_multi_webview_hosts() {
        let source = include_str!("project_commands.rs");
        for command in [
            "create_project_from_picker",
            "add_project_root_from_picker",
            "persist_project_layout_and_close",
        ] {
            let signature = source
                .split_once(&format!("pub async fn {command}("))
                .unwrap()
                .1
                .split_once(')')
                .unwrap()
                .0;
            assert!(
                signature.contains("window: Window,"),
                "{command} must inject the owning Window, not a WebviewWindow"
            );
        }
    }

    #[test]
    fn picker_cancellation_does_not_create_or_mutate_projects() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        assert!(!apply_create_selection(&database, None).unwrap());
        assert!(!apply_add_root_selection(&database, "unused", None).unwrap());
        assert_eq!(database.project_count().unwrap(), 0);
    }

    #[test]
    fn layout_persistence_is_epoch_checked_without_advancing_the_epoch() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("sage-layout-epoch-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let database = Database::open(Path::new(":memory:")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let selected = crate::domain::ResourceRef {
                root_id: project.roots[0].id.clone(),
                relative_path: "selected.txt".into(),
            };
            let layout = ProjectLayoutRequest {
                project_id: project.id,
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: Some(project.roots[0].id.clone()),
                selected_resource: Some(selected.clone()),
            };
            let state = AppState::new(database);

            persist_project_layout_inner(&state, Some(layout.clone()), 0)
                .await
                .unwrap();
            assert_eq!(*state.project_transitions.lock().await, 0);
            *state.project_transitions.lock().await = 1;
            let mut stale = layout;
            stale.selected_resource = None;
            assert!(
                persist_project_layout_inner(&state, Some(stale), 0)
                    .await
                    .is_err()
            );
            assert_eq!(
                state
                    .database
                    .bootstrap_snapshot()
                    .unwrap()
                    .selected_resource,
                Some(selected)
            );
            std::fs::remove_dir_all(root).unwrap();
        });
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

    #[test]
    fn overlapping_project_selection_fails_before_transition_mutation() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let root = std::env::temp_dir()
                .join(format!("sage-browser-project-switch-{}", Uuid::new_v4()));
            let ui = root.join("ui");
            let durable = root.join("durable");
            std::fs::create_dir_all(&ui).unwrap();
            std::fs::create_dir_all(&durable).unwrap();
            let browser = crate::browser::BrowserState::new(
                &crate::launch_profile::LaunchProfile::Normal {
                    durable_dir: durable,
                },
                ui,
                &[],
                1,
            )
            .unwrap();
            let state = AppState::new(Database::open(Path::new(":memory:")).unwrap());
            let selected = root.clone();
            let result = execute_committed_transition_checked(
                &state,
                0,
                || {
                    browser
                        .validate_project_roots(&[selected])
                        .map_err(crate::error::AppError::InvalidRequest)
                },
                || -> AppResult<()> { panic!("overlapping project was committed") },
            )
            .await;
            assert!(!result.committed);
            assert_eq!(result.transition_epoch, 0);
            assert!(result.recovery_error.unwrap().contains("overlaps"));
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn committed_project_transition_stops_the_previous_epoch_preview_but_a_failed_one_does_not() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let state = AppState::new(Database::open(Path::new(":memory:")).unwrap());
            let root =
                std::env::temp_dir().join(format!("sage-transition-preview-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let preview = crate::app_preview_commands::ServingPreview::start(0);

            let failed = execute_committed_transition(&state, 0, || -> AppResult<()> {
                Err(AppError::InvalidRequest("injected mutation failure".into()))
            })
            .await;
            assert!(!failed.committed);
            assert!(
                preview.is_serving(),
                "a transition that did not commit must not stop the current Preview"
            );

            let database = state.database.clone();
            let operation_root = root.clone();
            let committed = execute_committed_transition(&state, 0, move || {
                database.create_project_from_root(&operation_root)
            })
            .await;
            assert!(committed.committed);
            assert_eq!(committed.transition_epoch, 1);
            assert!(!preview.is_serving());
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}
