use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};

use tauri::State;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, oneshot},
    time::timeout,
};

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

use super::{
    helper_process,
    helper_protocol::{HelperRequest, HelperRoot},
    types::{
        MAX_PROJECT_ROOTS, WorkspaceSearchRequest, WorkspaceSearchResponse, valid_request_id,
        validate_search_request,
    },
};

const SEARCH_QUEUE_WAIT: Duration = Duration::from_millis(250);
const MAX_PRE_CANCELLED_REQUESTS: usize = 16;
static SEARCH_PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
static CANCELLATIONS: OnceLock<Mutex<CancellationRegistry>> = OnceLock::new();

#[derive(Default)]
struct CancellationRegistry {
    active: Option<(String, oneshot::Sender<()>)>,
    pre_cancelled: VecDeque<String>,
}

#[tauri::command]
pub async fn search_workspace(
    state: State<'_, AppState>,
    request: WorkspaceSearchRequest,
) -> Result<WorkspaceSearchResponse, String> {
    execute_with_runner(&state, request, helper_process::run)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_workspace_search(request_id: String) -> Result<bool, String> {
    if !valid_request_id(&request_id) {
        return Err("invalid request: workspace search request ID is invalid".into());
    }
    let mut registry = cancellation_registry().lock().await;
    if registry
        .active
        .as_ref()
        .is_some_and(|(active_id, _)| active_id == &request_id)
    {
        if let Some((_, sender)) = registry.active.take() {
            let _ = sender.send(());
        }
        return Ok(true);
    }
    if !registry.pre_cancelled.contains(&request_id) {
        if registry.pre_cancelled.len() == MAX_PRE_CANCELLED_REQUESTS {
            registry.pre_cancelled.pop_front();
        }
        registry.pre_cancelled.push_back(request_id);
    }
    Ok(false)
}

async fn execute_with_runner<F, Fut>(
    state: &AppState,
    request: WorkspaceSearchRequest,
    runner: F,
) -> AppResult<WorkspaceSearchResponse>
where
    F: FnOnce(HelperRequest, oneshot::Receiver<()>) -> Fut,
    Fut: Future<Output = AppResult<WorkspaceSearchResponse>>,
{
    validate_search_request(&request)?;
    let _permit = acquire_permit().await?;
    let helper_request = prepare_request(state, &request).await?;
    let cancellation = register_cancellation(&request.request_id).await?;
    let result = runner(helper_request, cancellation).await;
    clear_cancellation(&request.request_id).await;
    let response = result?;
    validate_completion(state, &request, &response).await?;
    Ok(response)
}

async fn acquire_permit() -> AppResult<OwnedSemaphorePermit> {
    timeout(SEARCH_QUEUE_WAIT, search_permits().acquire_owned())
        .await
        .map_err(|_| {
            AppError::ResourceLimit(
                "workspace search is busy; retry after the active scan finishes".into(),
            )
        })?
        .map_err(|_| AppError::InvalidRequest("workspace search is unavailable".into()))
}

async fn prepare_request(
    state: &AppState,
    request: &WorkspaceSearchRequest,
) -> AppResult<HelperRequest> {
    let transition_epoch = state.project_transitions.lock().await;
    validate_active_project(state, request, *transition_epoch)?;
    let summary = state.database.project_summary(&request.project_id)?;
    let root_names = summary
        .roots
        .into_iter()
        .map(|root| (root.id, root.name))
        .collect::<HashMap<_, _>>();
    let records = state.database.root_records(&request.project_id)?;
    if records.is_empty() || records.len() > MAX_PROJECT_ROOTS {
        return Err(AppError::ResourceLimit(
            "workspace search root snapshot exceeds its bound".into(),
        ));
    }
    let roots = records
        .into_iter()
        .map(|root| HelperRoot {
            root_name: root_names
                .get(&root.id)
                .cloned()
                .unwrap_or_else(|| "Root".into()),
            root_id: root.id,
            project_id: root.project_id,
            absolute_path: root.absolute_path,
        })
        .collect();
    Ok(HelperRequest::new(
        request.clone(),
        roots,
        crate::workspace::ignore_rules::global_excludes_path(),
    ))
}

fn validate_active_project(
    state: &AppState,
    request: &WorkspaceSearchRequest,
    actual_epoch: u64,
) -> AppResult<()> {
    if actual_epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before workspace search could run".into(),
        ));
    }
    if state.database.active_project_id()?.as_deref() != Some(&request.project_id) {
        return Err(AppError::InvalidRequest(
            "workspace search project is not active".into(),
        ));
    }
    Ok(())
}

async fn validate_completion(
    state: &AppState,
    request: &WorkspaceSearchRequest,
    response: &WorkspaceSearchResponse,
) -> AppResult<()> {
    let transition_epoch = state.project_transitions.lock().await;
    validate_active_project(state, request, *transition_epoch)?;
    if response.request_id != request.request_id
        || response.project_id != request.project_id
        || response.transition_epoch != request.expected_transition_epoch
        || response.query != request.query.trim()
    {
        return Err(AppError::InvalidRequest(
            "workspace search helper response identity is invalid".into(),
        ));
    }
    Ok(())
}

async fn register_cancellation(request_id: &str) -> AppResult<oneshot::Receiver<()>> {
    let mut registry = cancellation_registry().lock().await;
    if let Some(index) = registry
        .pre_cancelled
        .iter()
        .position(|candidate| candidate == request_id)
    {
        registry.pre_cancelled.remove(index);
        return Err(AppError::InvalidRequest(
            "workspace search was cancelled".into(),
        ));
    }
    if registry.active.is_some() {
        return Err(AppError::ResourceLimit(
            "workspace search cancellation registry is busy".into(),
        ));
    }
    let (sender, receiver) = oneshot::channel();
    registry.active = Some((request_id.into(), sender));
    Ok(receiver)
}

async fn clear_cancellation(request_id: &str) {
    let mut registry = cancellation_registry().lock().await;
    if registry
        .active
        .as_ref()
        .is_some_and(|(active_id, _)| active_id == request_id)
    {
        registry.active.take();
    }
}

fn search_permits() -> Arc<Semaphore> {
    SEARCH_PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn cancellation_registry() -> &'static Mutex<CancellationRegistry> {
    CANCELLATIONS.get_or_init(|| Mutex::new(CancellationRegistry::default()))
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc, time::Duration};

    use tokio::sync::Notify;
    use uuid::Uuid;

    use super::*;
    use crate::{
        persistence::Database,
        state::AppState,
        workspace::search::{helper_protocol, tests::request},
    };

    #[test]
    fn rejects_a_completion_after_the_project_epoch_changes() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("sage-search-race-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("file.txt"), "needle\n").unwrap();
            let database = Database::open(std::path::Path::new(":memory:")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let state = Arc::new(AppState::new(database));
            let mut search_request = request("needle");
            search_request.project_id = project.id;
            let started = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let runner_started = started.clone();
            let runner_release = release.clone();
            let search_state = state.clone();
            let search = tokio::spawn(async move {
                execute_with_runner(&search_state, search_request, move |helper, _| async move {
                    runner_started.notify_one();
                    runner_release.notified().await;
                    helper_protocol::execute(helper)
                })
                .await
            });

            tokio::time::timeout(Duration::from_secs(2), started.notified())
                .await
                .unwrap();
            *state.project_transitions.lock().await = 1;
            release.notify_one();
            let error = search.await.unwrap().unwrap_err();
            assert!(error.to_string().contains("project changed"));
            fs::remove_dir_all(root).unwrap();
        });
    }
}
