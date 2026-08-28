use std::sync::Arc;

use tauri::State;

use crate::{
    domain::{FileRenameOutcome, RenameProjectFileRequest, RenameProjectFileResult, ResourceRef},
    error::{AppError, AppResult},
    persistence::Database,
    preview_lifecycle::on_committed_project_transition_epoch,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
    workbench_persistence::validate_layout,
    workspace::file_rename::{FileRenameCommit, FileRenameMarker},
};

use validation::{
    MAX_JAVASCRIPT_SAFE_INTEGER, ensure_request_resources_closed, ensure_resources_closed,
    migrate_selection, resolve_workspace, validate_request,
};

mod validation;

#[tauri::command]
pub async fn rename_project_file(
    state: State<'_, AppState>,
    request: RenameProjectFileRequest,
) -> Result<RenameProjectFileResult, String> {
    execute(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rename_project_folder(
    state: State<'_, AppState>,
    request: RenameProjectFileRequest,
) -> Result<RenameProjectFileResult, String> {
    execute_folder(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
pub(super) enum RenameKind {
    File,
    Folder,
}

async fn execute(
    state: &AppState,
    request: RenameProjectFileRequest,
) -> AppResult<RenameProjectFileResult> {
    execute_with_kind(
        state,
        request,
        RenameKind::File,
        |_| Ok(()),
        || Ok(()),
        || Ok(()),
    )
    .await
}

async fn execute_folder(
    state: &AppState,
    request: RenameProjectFileRequest,
) -> AppResult<RenameProjectFileResult> {
    execute_with_kind(
        state,
        request,
        RenameKind::Folder,
        |_| Ok(()),
        || Ok(()),
        || Ok(()),
    )
    .await
}

#[cfg(test)]
async fn execute_with<F>(
    state: &AppState,
    request: RenameProjectFileRequest,
    after_rename: F,
) -> AppResult<RenameProjectFileResult>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()> + Send + 'static,
{
    execute_with_hooks(state, request, after_rename, || Ok(()), || Ok(())).await
}

#[cfg(test)]
async fn execute_with_hooks<F, R, S>(
    state: &AppState,
    request: RenameProjectFileRequest,
    after_rename: F,
    before_recovery: R,
    before_snapshot: S,
) -> AppResult<RenameProjectFileResult>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()> + Send + 'static,
    R: FnOnce() -> AppResult<()>,
    S: FnOnce() -> AppResult<()>,
{
    execute_with_kind(
        state,
        request,
        RenameKind::File,
        after_rename,
        before_recovery,
        before_snapshot,
    )
    .await
}

async fn execute_with_kind<F, R, S>(
    state: &AppState,
    request: RenameProjectFileRequest,
    kind: RenameKind,
    after_rename: F,
    before_recovery: R,
    before_snapshot: S,
) -> AppResult<RenameProjectFileResult>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()> + Send + 'static,
    R: FnOnce() -> AppResult<()>,
    S: FnOnce() -> AppResult<()>,
{
    validate_request(&request)?;
    let mut transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before file rename could run".into(),
        ));
    }
    if *transition_epoch >= MAX_JAVASCRIPT_SAFE_INTEGER {
        return Err(AppError::InvalidRequest(
            "project transition epoch exhausted before file rename".into(),
        ));
    }
    let database = state.database.clone();
    let recovery_database = database.clone();
    let operation_request = request.clone();
    let recovery_request = request.clone();
    let marker = Arc::new(FileRenameMarker::default());
    let operation_marker = Arc::clone(&marker);
    let operations = state.editor_operations.clone();
    let operation_ticket = operations.issue_ticket()?;
    let operation = tauri::async_runtime::spawn_blocking(move || {
        let coordinated_marker = Arc::clone(&operation_marker);
        let coordinated = operations.execute_project_transition(operation_ticket, || {
            let result = execute_blocking(
                database,
                operation_request,
                kind,
                operation_marker,
                after_rename,
            );
            if result.is_ok() || coordinated_marker.committed() {
                Ok(result)
            } else {
                Err(result.expect_err("failed rename result exists"))
            }
        });
        coordinated.and_then(std::convert::identity)
    })
    .await;
    let filesystem_committed = marker.committed();
    if filesystem_committed {
        *transition_epoch += 1;
        on_committed_project_transition_epoch(*transition_epoch);
    }
    let response_epoch = *transition_epoch;
    let (mut outcome, mut commit) = match operation {
        Ok(Ok(commit)) if commit.resource_confirmed => (FileRenameOutcome::Committed, commit),
        Ok(Ok(commit)) => (FileRenameOutcome::EffectUnknown, commit),
        Ok(Err(_)) | Err(_) if filesystem_committed => {
            let recovered = before_recovery().and_then(|()| {
                recover_effect_unknown(recovery_database, recovery_request, kind, &marker)
            });
            (
                FileRenameOutcome::EffectUnknown,
                recovered.unwrap_or_else(|_| {
                    fallback_commit(
                        &request,
                        "File rename changed the filesystem, but retained-authority recovery failed; Explorer is refreshing.",
                    )
                }),
            )
        }
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(AppError::InvalidRequest(error.to_string())),
    };
    let snapshot = if before_snapshot().is_ok() {
        build_bootstrap_snapshot(state, response_epoch).await.ok()
    } else {
        None
    };
    if snapshot.is_none() {
        outcome = FileRenameOutcome::EffectUnknown;
        append_warning(
            &mut commit,
            "The authoritative layout snapshot was unavailable after file rename; live UI state was preserved and Explorer is refreshing.",
        );
    }
    Ok(result_from_commit(
        request,
        response_epoch,
        outcome,
        commit,
        snapshot,
    ))
}

fn execute_blocking<F>(
    database: Database,
    request: RenameProjectFileRequest,
    kind: RenameKind,
    marker: Arc<FileRenameMarker>,
    after_rename: F,
) -> AppResult<FileRenameCommit>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()>,
{
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    let target = match kind {
        RenameKind::File => workspace.rename_file_target(&request.source, &request.basename)?,
        RenameKind::Folder => workspace.rename_file_target(&request.source, &request.basename)?,
    };
    ensure_resources_closed(
        &transaction,
        &request.project_id,
        &request.source,
        &target,
        kind,
    )?;
    validate_layout(&transaction, &request.current_layout)?;
    ensure_request_resources_closed(&request, &target, kind)?;
    let commit = match kind {
        RenameKind::File => {
            workspace.rename_file_observed(&request.source, &request.basename, marker)?
        }
        RenameKind::Folder => {
            workspace.rename_empty_folder_observed(&request.source, &request.basename, marker)?
        }
    };
    if commit.resource_confirmed {
        migrate_selection(
            &transaction,
            &request.project_id,
            &commit.source,
            &commit.target,
        )?;
    }
    after_rename(&transaction)?;
    transaction.commit()?;
    Ok(commit)
}

fn recover_effect_unknown(
    database: Database,
    request: RenameProjectFileRequest,
    kind: RenameKind,
    marker: &FileRenameMarker,
) -> AppResult<FileRenameCommit> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    let target = match kind {
        RenameKind::File => workspace.rename_file_target(&request.source, &request.basename)?,
        RenameKind::Folder => workspace.rename_file_target(&request.source, &request.basename)?,
    };
    ensure_resources_closed(
        &transaction,
        &request.project_id,
        &request.source,
        &target,
        kind,
    )?;
    validate_layout(&transaction, &request.current_layout)?;
    ensure_request_resources_closed(&request, &target, kind)?;
    let commit = match kind {
        RenameKind::File => {
            workspace.confirm_renamed_file(&request.source, &request.basename, marker)?
        }
        RenameKind::Folder => {
            return Err(AppError::InvalidRequest(
                "folder rename recovery authority is unavailable".into(),
            ));
        }
    };
    if commit.resource_confirmed {
        migrate_selection(
            &transaction,
            &request.project_id,
            &commit.source,
            &commit.target,
        )?;
    }
    transaction.commit()?;
    Ok(commit)
}

fn fallback_commit(request: &RenameProjectFileRequest, warning: &str) -> FileRenameCommit {
    let parent = request
        .source
        .relative_path
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let relative_path = if parent.is_empty() {
        request.basename.clone()
    } else {
        format!("{parent}/{}", request.basename)
    };
    FileRenameCommit::unconfirmed(
        request.source.clone(),
        ResourceRef {
            root_id: request.source.root_id.clone(),
            relative_path,
        },
        warning,
    )
}

fn append_warning(commit: &mut FileRenameCommit, warning: &str) {
    commit.warning = Some(match commit.warning.take() {
        Some(existing) => format!("{existing} {warning}"),
        None => warning.into(),
    });
}

fn result_from_commit(
    request: RenameProjectFileRequest,
    transition_epoch: u64,
    outcome: FileRenameOutcome,
    commit: FileRenameCommit,
    snapshot: Option<crate::domain::BootstrapSnapshot>,
) -> RenameProjectFileResult {
    RenameProjectFileResult {
        outcome,
        resource_confirmed: commit.resource_confirmed,
        project_id: request.project_id,
        project_generation: request.project_generation,
        transition_epoch,
        source: commit.source,
        target: commit.target,
        parent_listing: commit.parent_listing,
        snapshot,
        warning: commit.warning,
    }
}

#[cfg(test)]
mod tests;
