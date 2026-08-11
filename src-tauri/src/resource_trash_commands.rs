use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use tauri::State;

use crate::{
    domain::{
        ResourceRef, ResourceTrashOutcome, TrashProjectResourceRequest, TrashProjectResourceResult,
    },
    error::{AppError, AppResult},
    persistence::Database,
    project_registry::RootRecord,
    resource_commands::build_bootstrap_snapshot,
    state::AppState,
    workbench_persistence::validate_layout,
    workspace::{
        Workspace,
        resource_trash::{
            ResourceTrashCommit, ResourceTrashMarker, supported as resource_trash_supported,
        },
    },
};

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_RESOURCE_TRASH_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn trash_project_resource(
    state: State<'_, AppState>,
    request: TrashProjectResourceRequest,
) -> Result<TrashProjectResourceResult, String> {
    execute(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn execute(
    state: &AppState,
    request: TrashProjectResourceRequest,
) -> AppResult<TrashProjectResourceResult> {
    if !resource_trash_supported() {
        return unsupported_result(state, request).await;
    }
    execute_with_operation(
        state,
        request,
        |workspace, request, marker| {
            workspace.trash_resource_observed(&request.source, request.kind, marker)
        },
        |_| Ok(()),
        || Ok(()),
    )
    .await
}

async fn unsupported_result(
    state: &AppState,
    request: TrashProjectResourceRequest,
) -> AppResult<TrashProjectResourceResult> {
    validate_request(&request)?;
    let transition_epoch = state.project_transitions.lock().await;
    validate_epoch(*transition_epoch, &request)?;
    let parent = parent_resource(&request.source);
    Ok(TrashProjectResourceResult {
        outcome: ResourceTrashOutcome::Unsupported,
        resource_confirmed: false,
        project_id: request.project_id,
        project_generation: request.project_generation,
        transition_epoch: *transition_epoch,
        source: request.source,
        parent,
        parent_listing: None,
        snapshot: None,
        warning: Some(
            "Moving resources to the operating-system trash is not supported on this platform."
                .into(),
        ),
    })
}

async fn execute_with_operation<O, A, S>(
    state: &AppState,
    request: TrashProjectResourceRequest,
    operation: O,
    after_trash: A,
    before_snapshot: S,
) -> AppResult<TrashProjectResourceResult>
where
    O: FnOnce(
            &Workspace,
            &TrashProjectResourceRequest,
            Arc<ResourceTrashMarker>,
        ) -> AppResult<ResourceTrashCommit>
        + Send
        + 'static,
    A: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()> + Send + 'static,
    S: FnOnce() -> AppResult<()>,
{
    validate_request(&request)?;
    let mut transition_epoch = state.project_transitions.lock().await;
    validate_epoch(*transition_epoch, &request)?;
    let database = state.database.clone();
    let recovery_database = database.clone();
    let operation_request = request.clone();
    let recovery_request = request.clone();
    let marker = Arc::new(ResourceTrashMarker::default());
    let operation_marker = Arc::clone(&marker);
    let operations = state.editor_operations.clone();
    let ticket = operations.issue_ticket()?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let observed_marker = Arc::clone(&operation_marker);
        let coordinated = operations.execute_project_transition(ticket, || {
            let result = execute_blocking(
                database,
                operation_request,
                operation_marker,
                operation,
                after_trash,
            );
            if result.is_ok() || observed_marker.mutation_started() {
                Ok(result)
            } else {
                Err(result.expect_err("failed Recycle Bin result exists"))
            }
        });
        coordinated.and_then(std::convert::identity)
    })
    .await;
    let mutation_started = marker.mutation_started();
    if mutation_started {
        *transition_epoch += 1;
    }
    let response_epoch = *transition_epoch;
    let (mut outcome, mut commit) = match task {
        Ok(Ok(commit)) if commit.resource_confirmed => (ResourceTrashOutcome::Committed, commit),
        Ok(Ok(commit)) => (ResourceTrashOutcome::EffectUnknown, commit),
        Ok(Err(_)) | Err(_) if mutation_started => (
            ResourceTrashOutcome::EffectUnknown,
            recover_effect_unknown(recovery_database, recovery_request, &marker)
                .unwrap_or_else(|_| fallback_commit(&request)),
        ),
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(AppError::InvalidRequest(error.to_string())),
    };
    let snapshot = if before_snapshot().is_ok() {
        build_bootstrap_snapshot(state, response_epoch).await.ok()
    } else {
        None
    };
    if snapshot.is_none() {
        outcome = ResourceTrashOutcome::EffectUnknown;
        append_warning(
            &mut commit,
            "The authoritative layout snapshot was unavailable after the Recycle Bin operation; Explorer is refreshing.",
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

fn execute_blocking<O, A>(
    database: Database,
    request: TrashProjectResourceRequest,
    marker: Arc<ResourceTrashMarker>,
    operation: O,
    after_trash: A,
) -> AppResult<ResourceTrashCommit>
where
    O: FnOnce(
        &Workspace,
        &TrashProjectResourceRequest,
        Arc<ResourceTrashMarker>,
    ) -> AppResult<ResourceTrashCommit>,
    A: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<()>,
{
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    layout::ensure_resource_closed(&transaction, &request.project_id, &request.source)?;
    validate_layout(&transaction, &request.current_layout)?;
    layout::ensure_request_resource_closed(&request)?;
    let commit = operation(&workspace, &request, marker)?;
    if commit.resource_confirmed {
        layout::migrate_selection(
            &transaction,
            &request.project_id,
            &commit.source,
            &commit.parent,
        )?;
    }
    after_trash(&transaction)?;
    transaction.commit()?;
    Ok(commit)
}

fn recover_effect_unknown(
    database: Database,
    request: TrashProjectResourceRequest,
    marker: &ResourceTrashMarker,
) -> AppResult<ResourceTrashCommit> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let workspace = resolve_workspace(&transaction, &request)?;
    layout::ensure_resource_closed(&transaction, &request.project_id, &request.source)?;
    validate_layout(&transaction, &request.current_layout)?;
    layout::ensure_request_resource_closed(&request)?;
    let commit = workspace.confirm_trashed_resource(&request.source, marker)?;
    if commit.resource_confirmed {
        layout::migrate_selection(
            &transaction,
            &request.project_id,
            &commit.source,
            &commit.parent,
        )?;
    }
    transaction.commit()?;
    Ok(commit)
}

fn resolve_workspace(
    transaction: &rusqlite::Transaction<'_>,
    request: &TrashProjectResourceRequest,
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
            "Recycle Bin Project is not active".into(),
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
                "Recycle Bin root does not belong to the active Project".into(),
            )
        })?;
    Workspace::open(&root.project_id, &root.id, &root.absolute_path)
}

fn validate_request(request: &TrashProjectResourceRequest) -> AppResult<()> {
    validate_request_id("Project ID", &request.project_id)?;
    validate_request_id("root ID", &request.source.root_id)?;
    crate::workspace::validate_contract_path(&request.source.relative_path, false)?;
    if request.current_layout.project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Recycle Bin layout does not belong to the requested Project".into(),
        ));
    }
    if request.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        || request.expected_transition_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "Recycle Bin intent metadata is outside the safe integer range".into(),
        ));
    }
    Ok(())
}

fn validate_epoch(epoch: u64, request: &TrashProjectResourceRequest) -> AppResult<()> {
    if epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the Recycle Bin operation could run".into(),
        ));
    }
    if epoch >= MAX_JAVASCRIPT_SAFE_INTEGER {
        return Err(AppError::InvalidRequest(
            "Project transition epoch exhausted before the Recycle Bin operation".into(),
        ));
    }
    Ok(())
}

fn validate_request_id(label: &str, value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > MAX_RESOURCE_TRASH_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "{label} must be 1-{MAX_RESOURCE_TRASH_ID_BYTES} UTF-8 bytes without control characters"
        )));
    }
    Ok(())
}

fn parent_resource(source: &ResourceRef) -> ResourceRef {
    ResourceRef {
        root_id: source.root_id.clone(),
        relative_path: source
            .relative_path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent)
            .into(),
    }
}

fn fallback_commit(request: &TrashProjectResourceRequest) -> ResourceTrashCommit {
    ResourceTrashCommit::unconfirmed(
        request.source.clone(),
        parent_resource(&request.source),
        "Recycle Bin operation may have changed the filesystem, but retained-authority recovery failed; Explorer is refreshing.",
    )
}

fn append_warning(commit: &mut ResourceTrashCommit, warning: &str) {
    commit.warning = Some(match commit.warning.take() {
        Some(existing) => format!("{existing} {warning}"),
        None => warning.into(),
    });
}

fn result_from_commit(
    request: TrashProjectResourceRequest,
    transition_epoch: u64,
    outcome: ResourceTrashOutcome,
    commit: ResourceTrashCommit,
    snapshot: Option<crate::domain::BootstrapSnapshot>,
) -> TrashProjectResourceResult {
    TrashProjectResourceResult {
        outcome,
        resource_confirmed: commit.resource_confirmed,
        project_id: request.project_id,
        project_generation: request.project_generation,
        transition_epoch,
        source: commit.source,
        parent: commit.parent,
        parent_listing: commit.parent_listing,
        snapshot,
        warning: commit.warning,
    }
}

mod layout;

#[cfg(test)]
mod tests;
