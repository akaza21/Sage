use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;

use crate::{
    domain::{CreateProjectFileRequest, CreateProjectFileResult, FileCreationOutcome},
    error::{AppError, AppResult},
    persistence::Database,
    state::AppState,
    workspace::Workspace,
};

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_FILE_CREATE_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn create_project_file(
    state: State<'_, AppState>,
    request: CreateProjectFileRequest,
) -> Result<CreateProjectFileResult, String> {
    execute(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn execute(
    state: &AppState,
    request: CreateProjectFileRequest,
) -> AppResult<CreateProjectFileResult> {
    validate_request_id("Project ID", &request.project_id)?;
    validate_request_id("root ID", &request.parent.root_id)?;
    if request.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        || request.expected_transition_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "file creation intent metadata is outside the safe integer range".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before file creation could run".into(),
        ));
    }
    let database = state.database.clone();
    let recovery_database = database.clone();
    let recovery_request = request.clone();
    let committed = Arc::new(AtomicBool::new(false));
    let operation_committed = committed.clone();
    let response_epoch = *transition_epoch;
    match tauri::async_runtime::spawn_blocking(move || {
        execute_blocking(database, request, response_epoch, operation_committed)
    })
    .await
    {
        Ok(result) => result,
        Err(_) if committed.load(Ordering::Acquire) => {
            tauri::async_runtime::spawn_blocking(move || {
                recover_effect_unknown(recovery_database, recovery_request, response_epoch)
            })
            .await
            .map_err(|_| {
                AppError::InvalidRequest(
                    "file creation outcome is unknown; refresh Explorer before retrying".into(),
                )
            })?
        }
        Err(error) => Err(AppError::InvalidRequest(error.to_string())),
    }
}

fn execute_blocking(
    database: Database,
    request: CreateProjectFileRequest,
    transition_epoch: u64,
    committed: Arc<AtomicBool>,
) -> AppResult<CreateProjectFileResult> {
    let workspace = resolve_workspace(&database, &request)?;
    let commit =
        workspace.create_empty_file_observed(&request.parent, &request.basename, committed)?;
    let outcome = if commit.resource_confirmed {
        FileCreationOutcome::Committed
    } else {
        FileCreationOutcome::EffectUnknown
    };
    Ok(result_from_commit(
        request,
        transition_epoch,
        outcome,
        commit,
    ))
}

fn recover_effect_unknown(
    database: Database,
    request: CreateProjectFileRequest,
    transition_epoch: u64,
) -> AppResult<CreateProjectFileResult> {
    let workspace = resolve_workspace(&database, &request)?;
    let commit = workspace.confirm_created_file(&request.parent, &request.basename)?;
    Ok(result_from_commit(
        request,
        transition_epoch,
        FileCreationOutcome::EffectUnknown,
        commit,
    ))
}

fn resolve_workspace(
    database: &Database,
    request: &CreateProjectFileRequest,
) -> AppResult<Workspace> {
    if database.active_project_id()?.as_deref() != Some(&request.project_id) {
        return Err(AppError::InvalidRequest(
            "file creation Project is not active".into(),
        ));
    }
    let root = database.root_record(&request.parent.root_id)?;
    if root.project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "file creation root does not belong to the active Project".into(),
        ));
    }
    Workspace::open(&root.project_id, &root.id, &root.absolute_path)
}

fn result_from_commit(
    request: CreateProjectFileRequest,
    transition_epoch: u64,
    outcome: FileCreationOutcome,
    commit: crate::workspace::file_create::FileCreateCommit,
) -> CreateProjectFileResult {
    CreateProjectFileResult {
        outcome,
        resource_confirmed: commit.resource_confirmed,
        project_id: request.project_id,
        project_generation: request.project_generation,
        transition_epoch,
        resource: commit.resource,
        parent_listing: commit.parent_listing,
        warning: commit.warning,
    }
}

fn validate_request_id(label: &str, value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > MAX_FILE_CREATE_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "{label} must be 1-{MAX_FILE_CREATE_ID_BYTES} UTF-8 bytes without control characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
