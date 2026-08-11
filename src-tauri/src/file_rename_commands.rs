use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use tauri::State;

use crate::{
    domain::{FileRenameOutcome, RenameProjectFileRequest, RenameProjectFileResult, ResourceRef},
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    project_registry::RootRecord,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
    workbench_persistence::validate_layout,
    workspace::{
        Workspace,
        file_rename::{FileRenameCommit, FileRenameMarker},
    },
};

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_FILE_RENAME_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn rename_project_file(
    state: State<'_, AppState>,
    request: RenameProjectFileRequest,
) -> Result<RenameProjectFileResult, String> {
    execute(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn execute(
    state: &AppState,
    request: RenameProjectFileRequest,
) -> AppResult<RenameProjectFileResult> {
    execute_with_hooks(state, request, |_| Ok(()), || Ok(()), || Ok(())).await
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
            let result =
                execute_blocking(database, operation_request, operation_marker, after_rename);
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
    }
    let response_epoch = *transition_epoch;
    let (mut outcome, mut commit) = match operation {
        Ok(Ok(commit)) if commit.resource_confirmed => (FileRenameOutcome::Committed, commit),
        Ok(Ok(commit)) => (FileRenameOutcome::EffectUnknown, commit),
        Ok(Err(_)) | Err(_) if filesystem_committed => {
            let recovered = before_recovery().and_then(|()| {
                recover_effect_unknown(recovery_database, recovery_request, &marker)
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
    marker: Arc<FileRenameMarker>,
    after_rename: F,
) -> AppResult<FileRenameCommit>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()>,
{
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    let target = workspace.rename_file_target(&request.source, &request.basename)?;
    ensure_resources_closed(&transaction, &request.project_id, &request.source, &target)?;
    validate_layout(&transaction, &request.current_layout)?;
    ensure_request_resources_closed(&request, &target)?;
    let commit = workspace.rename_file_observed(&request.source, &request.basename, marker)?;
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
    marker: &FileRenameMarker,
) -> AppResult<FileRenameCommit> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    let target = workspace.rename_file_target(&request.source, &request.basename)?;
    ensure_resources_closed(&transaction, &request.project_id, &request.source, &target)?;
    validate_layout(&transaction, &request.current_layout)?;
    ensure_request_resources_closed(&request, &target)?;
    let commit = workspace.confirm_renamed_file(&request.source, &request.basename, marker)?;
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

fn resolve_workspace(
    transaction: &rusqlite::Transaction<'_>,
    request: &RenameProjectFileRequest,
) -> AppResult<Workspace> {
    let active = transaction
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if active.as_deref() != Some(request.project_id.as_str()) {
        return Err(AppError::InvalidRequest(
            "file rename Project is not active".into(),
        ));
    }
    let root = transaction
        .query_row(
            "SELECT id,project_id,absolute_path,ordinal
             FROM project_roots WHERE id=?1 AND project_id=?2",
            params![request.source.root_id, request.project_id],
            |row| {
                Ok(RootRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    absolute_path: row.get::<_, String>(2)?.into(),
                    ordinal: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "file rename root does not belong to the active Project".into(),
            )
        })?;
    Workspace::open(&root.project_id, &root.id, &root.absolute_path)
}

fn ensure_resources_closed(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
    target: &ResourceRef,
) -> AppResult<()> {
    let tab_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM workbench_tabs
           WHERE project_id=?1 AND kind IN ('file','diff')
             AND json_extract(metadata_json,'$.resource.rootId')=?2
             AND json_extract(metadata_json,'$.resource.relativePath') IN (?3,?4)
         )",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path
        ],
        |row| row.get(0),
    )?;
    if tab_reference {
        return Err(AppError::InvalidRequest(
            "close File and Diff tabs for the source and target before renaming".into(),
        ));
    }
    let buffer_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers
           WHERE project_id=?1 AND root_id=?2 AND relative_path IN (?3,?4)
         )",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path
        ],
        |row| row.get(0),
    )?;
    if buffer_reference {
        return Err(AppError::InvalidRequest(
            "discard or save and close the source and target editor buffer before renaming".into(),
        ));
    }
    Ok(())
}

fn ensure_request_resources_closed(
    request: &RenameProjectFileRequest,
    target: &ResourceRef,
) -> AppResult<()> {
    let has_reference = request.current_layout.tabs.iter().any(|tab| {
        matches!(tab.kind.as_str(), "file" | "diff")
            && tab
                .resource
                .as_ref()
                .is_some_and(|resource| resource == &request.source || resource == target)
    });
    if has_reference {
        return Err(AppError::InvalidRequest(
            "close File and Diff tabs for the source and target before renaming".into(),
        ));
    }
    Ok(())
}

fn migrate_selection(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
    target: &ResourceRef,
) -> AppResult<()> {
    transaction.execute(
        "UPDATE project_layouts
         SET selected_relative_path=?4,updated_at=?5
         WHERE project_id=?1 AND selected_root_id=?2 AND selected_relative_path=?3",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path,
            now_millis()
        ],
    )?;
    Ok(())
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

fn validate_request(request: &RenameProjectFileRequest) -> AppResult<()> {
    validate_request_id("Project ID", &request.project_id)?;
    validate_request_id("root ID", &request.source.root_id)?;
    if request.current_layout.project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "file rename layout does not belong to the requested Project".into(),
        ));
    }
    if request.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        || request.expected_transition_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "file rename intent metadata is outside the safe integer range".into(),
        ));
    }
    Ok(())
}

fn validate_request_id(label: &str, value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > MAX_FILE_RENAME_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "{label} must be 1-{MAX_FILE_RENAME_ID_BYTES} UTF-8 bytes without control characters"
        )));
    }
    Ok(())
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
