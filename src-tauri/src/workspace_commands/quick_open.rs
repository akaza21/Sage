//! The `quick_open_query` command: one bounded query against the active
//! project's path index.

use sage_core::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::{settings_store::EditorValue, state::AppState};
use sage_domain::resource::ResourceRef;
use sage_workspace::{Workspace, ignore_rules::global_excludes_path};

use sage_workspace::path_index::{
    IndexInputs, Ticket,
    arena::MAX_INDEXED_PATH_BYTES,
    query::{Outcome, QuickOpenQuery, run},
    ticket,
};

pub const MAX_QUICK_OPEN_RESULTS: u32 = 50;
const MAX_QUICK_OPEN_QUERY_BYTES: usize = 1024;
const MAX_RECENT_FILES: usize = 50;
const MAX_PROJECT_ROOTS: usize = 8;
const MAX_ROOT_ID_BYTES: usize = 512;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuickOpenRequest {
    pub project_id: String,
    pub expected_transition_epoch: u64,
    pub query: String,
    pub limit: u32,
    /// Increases with each query; a newer one stops older ones. It must keep
    /// increasing across webview reloads, so the UI derives it from a clock.
    pub generation: u64,
    /// Identifies one opening of Quick Open. The first query of a new session
    /// refreshes the index in the background.
    pub session: u64,
    /// Recently opened Files, most recent first, ranked higher.
    #[serde(default)]
    pub recent: Vec<ResourceRef>,
    /// An empty query lists other Files after the recent ones, shortest
    /// paths first, for pickers that browse rather than reopen.
    #[serde(default)]
    pub browse: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuickOpenState {
    Ready,
    Superseded,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenMatch {
    pub root_id: String,
    pub relative_path: String,
    pub positions: Vec<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenResponse {
    pub state: QuickOpenState,
    pub results: Vec<QuickOpenMatch>,
    pub indexed_files: usize,
    /// The index stopped at its File limit, so some Files can't be found.
    pub truncated: bool,
    /// Whether this query had to walk the Project first.
    pub built: bool,
    /// A fresh index is being built; asking again shortly sees it.
    pub refreshing: bool,
    pub index_bytes: usize,
    pub ready_ms: u64,
}

#[tauri::command]
pub async fn quick_open_query(
    state: State<'_, AppState>,
    request: QuickOpenRequest,
) -> Result<QuickOpenResponse, String> {
    let (inputs, ticket) = prepare(&state, &request)
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let query = QuickOpenQuery {
            text: &request.query,
            limit: request.limit as usize,
            generation: request.generation,
            session: request.session,
            recent: &request.recent,
            browse: request.browse,
        };
        respond(run(&inputs, &query, ticket))
    })
    .await
    .map_err(|error| format!("Quick Open stopped unexpectedly: {error}"))
}

/// Checks the request and reads what the index is built from. The ticket is
/// taken before the active-Project check, so a Project closed after that
/// check keeps no index.
async fn prepare(state: &AppState, request: &QuickOpenRequest) -> AppResult<(IndexInputs, Ticket)> {
    validate(request)?;
    let epoch = state.project_transitions.lock().await;
    let ticket = ticket();
    check_active(state, request, *epoch)?;
    Ok((inputs(state, &request.project_id)?, ticket))
}

fn validate(request: &QuickOpenRequest) -> AppResult<()> {
    if request.project_id.is_empty() || request.project_id.len() > 512 {
        return Err(AppError::InvalidRequest(
            "Quick Open project ID is invalid".into(),
        ));
    }
    if request.query.len() > MAX_QUICK_OPEN_QUERY_BYTES {
        return Err(AppError::InvalidRequest(
            "Quick Open query is too long".into(),
        ));
    }
    if request.limit == 0 || request.limit > MAX_QUICK_OPEN_RESULTS {
        return Err(AppError::InvalidRequest(
            "Quick Open limit must be between 1 and 50".into(),
        ));
    }
    if request.recent.len() > MAX_RECENT_FILES
        || request.recent.iter().any(|resource| {
            resource.root_id.len() > MAX_ROOT_ID_BYTES
                || resource.relative_path.len() > MAX_INDEXED_PATH_BYTES
        })
    {
        return Err(AppError::InvalidRequest(
            "Quick Open recent Files exceed their bounds".into(),
        ));
    }
    Ok(())
}

fn check_active(state: &AppState, request: &QuickOpenRequest, epoch: u64) -> AppResult<()> {
    if epoch != request.expected_transition_epoch
        || state.database.active_project_id()?.as_deref() != Some(&request.project_id)
    {
        return Err(AppError::InvalidRequest(
            "Quick Open project is not active".into(),
        ));
    }
    Ok(())
}

/// The roots and settings the index is built from, read on the trusted side.
fn inputs(state: &AppState, project_id: &str) -> AppResult<IndexInputs> {
    let records = state.database.root_records(project_id)?;
    if records.is_empty() || records.len() > MAX_PROJECT_ROOTS {
        return Err(AppError::ResourceLimit(
            "Quick Open root count exceeds its bound".into(),
        ));
    }
    let roots = records
        .iter()
        // An unavailable root is left out rather than failing the query.
        .filter_map(|record| Workspace::open(project_id, &record.id, &record.absolute_path).ok())
        .map(|workspace| (workspace.root_id().to_owned(), workspace.root().to_owned()))
        .collect();
    let settings = state.settings.editor_settings()?;
    let use_ignore_files = !matches!(
        settings.get("search.useIgnoreFiles"),
        Some(EditorValue::Bool(false))
    );
    let files_exclude = match settings.get("files.exclude") {
        Some(EditorValue::Globs(patterns)) => patterns.clone(),
        _ => sage_workspace::path_glob::default_files_exclude(),
    };
    Ok(IndexInputs {
        project_id: project_id.to_owned(),
        roots,
        use_ignore_files,
        files_exclude,
        global_excludes: global_excludes_path(),
    })
}

fn respond(outcome: Outcome) -> QuickOpenResponse {
    match outcome {
        Outcome::Superseded => QuickOpenResponse {
            state: QuickOpenState::Superseded,
            results: Vec::new(),
            indexed_files: 0,
            truncated: false,
            built: false,
            refreshing: false,
            index_bytes: 0,
            ready_ms: 0,
        },
        Outcome::Ready {
            results,
            files,
            truncated,
            heap_bytes,
            readied,
        } => QuickOpenResponse {
            state: QuickOpenState::Ready,
            results: results
                .into_iter()
                .map(|ranked| QuickOpenMatch {
                    root_id: ranked.root_id,
                    relative_path: ranked.relative_path,
                    positions: ranked.positions,
                })
                .collect(),
            indexed_files: files,
            truncated,
            built: readied.built,
            refreshing: readied.refreshing,
            index_bytes: heap_bytes,
            ready_ms: readied.took.as_millis() as u64,
        },
    }
}
