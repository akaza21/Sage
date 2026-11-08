use tauri::{AppHandle, Manager, State};

use crate::{
    domain::{
        BootstrapSnapshot, BufferSnapshotAck, BufferSnapshotRequest, DirectoryListing,
        EditorDocument, EditorSaveResult, FileDiff, ProjectRepositoryStatus, ResourceRef, TextFile,
        WorkbenchTab,
    },
    editor,
    editor_coordination::EditorOperationKind,
    repository,
    state::AppState,
    workbench_restore::{
        MAX_RESOURCE_TABS, MAX_RESTORED_TAB_BYTES, MAX_RESTORED_TABS, tab_projection_bytes,
        validate_tab_projection,
    },
    workspace::Workspace,
};

#[tauri::command]
pub async fn get_bootstrap_snapshot(
    state: State<'_, AppState>,
) -> Result<BootstrapSnapshot, String> {
    let transition_epoch = *state.project_transitions.lock().await;
    let database = state.database.clone();
    let harnesses = state.harnesses.clone();
    let startup_recovery_notice = state.startup_recovery_notice.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        build_initial_bootstrap_snapshot(
            &database,
            &harnesses,
            startup_recovery_notice.as_deref(),
            transition_epoch,
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(snapshot)
}

pub(crate) async fn build_bootstrap_snapshot(
    state: &AppState,
    transition_epoch: u64,
) -> crate::error::AppResult<BootstrapSnapshot> {
    build_bootstrap_snapshot_with_repository_status(state, transition_epoch).await
}

fn build_initial_bootstrap_snapshot(
    database: &crate::persistence::Database,
    harnesses: &crate::harnesses::HarnessCatalog,
    startup_recovery_notice: Option<&str>,
    transition_epoch: u64,
) -> crate::error::AppResult<BootstrapSnapshot> {
    let mut snapshot = database.bootstrap_snapshot_with_catalog(harnesses)?;
    snapshot.transition_epoch = transition_epoch;
    let Some(project) = &snapshot.project else {
        append_startup_recovery_notice(&mut snapshot, startup_recovery_notice);
        return Ok(snapshot);
    };
    snapshot.recovered_buffers = editor::recovered_buffers(database, &project.id)?;
    merge_recovered_file_tabs(&mut snapshot);
    append_startup_recovery_notice(&mut snapshot, startup_recovery_notice);
    validate_tab_projection(&snapshot.tabs)?;
    Ok(snapshot)
}

async fn build_bootstrap_snapshot_with_repository_status(
    state: &AppState,
    transition_epoch: u64,
) -> crate::error::AppResult<BootstrapSnapshot> {
    let mut snapshot = state
        .database
        .bootstrap_snapshot_with_catalog(&state.harnesses)?;
    snapshot.transition_epoch = transition_epoch;
    let Some(project) = &snapshot.project else {
        append_startup_recovery_notice(&mut snapshot, state.startup_recovery_notice.as_deref());
        return Ok(snapshot);
    };
    let roots = state.database.root_records(&project.id)?;
    let mut repository_roots = Vec::with_capacity(roots.len());
    for root in roots {
        let workspace = Workspace::open(&root.project_id, &root.id, &root.absolute_path)
            .and_then(|workspace| {
                editor::recover_pending_saves(&state.database, &workspace)?;
                Ok(workspace)
            })
            .map_err(|error| error.to_string());
        repository_roots.push((root.id, workspace));
    }
    snapshot.repositories = repository::project_status(repository_roots).await;
    snapshot.recovered_buffers = editor::recovered_buffers(&state.database, &project.id)?;
    merge_recovered_file_tabs(&mut snapshot);
    append_startup_recovery_notice(&mut snapshot, state.startup_recovery_notice.as_deref());
    validate_tab_projection(&snapshot.tabs)?;
    Ok(snapshot)
}

fn append_startup_recovery_notice(snapshot: &mut BootstrapSnapshot, notice: Option<&str>) {
    let Some(notice) = notice else { return };
    snapshot.recovery_notice = Some(match snapshot.recovery_notice.take() {
        Some(existing) => format!("{existing} {notice}"),
        None => notice.into(),
    });
}

fn merge_recovered_file_tabs(snapshot: &mut BootstrapSnapshot) {
    let mut next_position = snapshot
        .tabs
        .iter()
        .map(|tab| tab.position)
        .max()
        .unwrap_or(-1)
        .checked_add(1);
    let mut resource_count = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.kind == "file" || tab.kind == "diff")
        .count();
    let mut projection_bytes = snapshot.tabs.iter().map(tab_projection_bytes).sum::<i64>();
    let mut hidden = 0_usize;
    let mut used_ids = snapshot
        .tabs
        .iter()
        .map(|tab| tab.id.clone())
        .collect::<std::collections::HashSet<_>>();
    for recovered in &snapshot.recovered_buffers {
        if let Some(tab) = snapshot
            .tabs
            .iter_mut()
            .find(|tab| tab.resource.as_ref() == Some(&recovered.resource))
        {
            tab.dirty = true;
            continue;
        }
        if snapshot.tabs.len() == MAX_RESTORED_TABS
            || resource_count == MAX_RESOURCE_TABS
            || next_position.is_none()
        {
            hidden += 1;
            continue;
        }
        let title = recovered
            .resource
            .relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&recovered.resource.relative_path)
            .to_owned();
        let base_id = format!(
            "recovered-file:{}:{}",
            recovered.resource.root_id, recovered.resource.relative_path
        );
        let mut id = base_id.clone();
        let mut suffix = 1_u32;
        while used_ids.contains(&id) {
            id = format!("{base_id}:{suffix}");
            suffix += 1;
        }
        let tab = WorkbenchTab {
            id,
            kind: "file".into(),
            title,
            position: next_position.unwrap_or_default(),
            pinned: false,
            dirty: true,
            status: Some("recovering".into()),
            updated_at: None,
            finished_at: None,
            resource: Some(recovered.resource.clone()),
            repository: None,
            diff_ref: None,
        };
        let tab_bytes = tab_projection_bytes(&tab);
        if projection_bytes.saturating_add(tab_bytes) > MAX_RESTORED_TAB_BYTES {
            hidden += 1;
            continue;
        }
        used_ids.insert(tab.id.clone());
        projection_bytes += tab_bytes;
        resource_count += 1;
        snapshot.tabs.push(tab);
        next_position = next_position.and_then(|position| position.checked_add(1));
    }
    snapshot.recovery_notice = (hidden > 0).then(|| {
        format!(
            "{hidden} dirty buffer{} remain safely stored; close a File/Diff tab and restart to open recovery tabs",
            if hidden == 1 { "" } else { "s" }
        )
    });
    if hidden == 0 {
        snapshot.recovery_notice = None;
    }
}

#[tauri::command]
pub async fn get_repository_status(
    state: State<'_, AppState>,
) -> Result<ProjectRepositoryStatus, String> {
    let project_id = state
        .database
        .active_project_id()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no project is active".to_owned())?;
    let roots = state
        .database
        .root_records(&project_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|root| {
            let workspace = Workspace::open(&root.project_id, &root.id, &root.absolute_path)
                .map_err(|error| error.to_string());
            (root.id, workspace)
        })
        .collect();
    Ok(repository::project_status(roots).await)
}

#[tauri::command]
pub async fn get_file_diff(
    state: State<'_, AppState>,
    resource: ResourceRef,
    scope: Option<crate::repository_diff::GitDiffScope>,
) -> Result<FileDiff, String> {
    let workspace = state
        .database
        .resolve_active_resource(&resource)
        .map_err(|error| error.to_string())?;
    match scope {
        Some(scope) => {
            crate::repository_diff::scoped_file_diff(&workspace, &resource, Some(scope)).await
        }
        None => repository::file_diff(&workspace, &resource).await,
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_directory(
    app: AppHandle,
    resource: ResourceRef,
) -> Result<DirectoryListing, String> {
    let database = app.state::<AppState>().database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database
            .resolve_active_resource(&resource)
            .and_then(|workspace| workspace.list_directory(&resource))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_text_file(
    state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<TextFile, String> {
    state
        .database
        .resolve_active_resource(&resource)
        .and_then(|workspace| workspace.read_text_file(&resource))
        .map_err(|error| error.to_string())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpenError {
    kind: &'static str,
    message: String,
}
impl From<crate::error::AppError> for EditorOpenError {
    fn from(error: crate::error::AppError) -> Self {
        Self {
            kind: if matches!(error, crate::error::AppError::MixedLineEndings) {
                "mixedLineEndings"
            } else {
                "unavailable"
            },
            message: error.to_string(),
        }
    }
}

#[tauri::command]
pub async fn open_editor_document(
    state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<EditorDocument, EditorOpenError> {
    run_editor_operation(
        state,
        resource,
        EditorOperationKind::Read,
        editor::open_document,
    )
    .await
    .map_err(EditorOpenError::from)
}

#[tauri::command]
pub async fn snapshot_editor_buffer(
    state: State<'_, AppState>,
    request: BufferSnapshotRequest,
) -> Result<BufferSnapshotAck, String> {
    let resource = request.resource.clone();
    run_editor_operation(
        state,
        resource,
        EditorOperationKind::Snapshot,
        move |database, workspace, _| editor::snapshot_buffer(database, workspace, request),
    )
    .await
    .map_err(|error| error.to_string())
}

/// "Keep mine" for an Editor buffer whose File changed or was deleted on disk.
#[tauri::command]
pub async fn rebase_editor_buffer(
    state: State<'_, AppState>,
    request: crate::domain::BufferRebaseRequest,
) -> Result<EditorSaveResult, String> {
    let resource = request.resource.clone();
    run_editor_operation(
        state,
        resource,
        EditorOperationKind::Terminal,
        move |database, workspace, _| editor::rebase_buffer(database, workspace, request),
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_editor_document(
    state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<EditorSaveResult, String> {
    run_editor_operation(
        state,
        resource,
        EditorOperationKind::Terminal,
        editor::save_document,
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn discard_editor_buffer(
    state: State<'_, AppState>,
    resource: ResourceRef,
) -> Result<EditorDocument, String> {
    run_editor_operation(
        state,
        resource,
        EditorOperationKind::Terminal,
        editor::discard_document,
    )
    .await
    .map_err(|error| error.to_string())
}

async fn run_editor_operation<T: Send + 'static>(
    state: State<'_, AppState>,
    resource: ResourceRef,
    kind: EditorOperationKind,
    operation: impl FnOnce(
        &crate::persistence::Database,
        &Workspace,
        &ResourceRef,
    ) -> crate::error::AppResult<T>
    + Send
    + 'static,
) -> crate::error::AppResult<T> {
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations.issue_ticket()?;
    tauri::async_runtime::spawn_blocking(move || {
        operations.execute(ticket, kind, || {
            let workspace = database.resolve_active_resource(&resource)?;
            operation(&database, &workspace, &resource)
        })
    })
    .await
    .map_err(|error| crate::error::AppError::InvalidRequest(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use super::*;
    use crate::{domain::RecoveredEditorBuffer, persistence::Database};

    fn snapshot() -> (std::path::PathBuf, BootstrapSnapshot, String) {
        let root = std::env::temp_dir().join(format!("sage-recovery-tabs-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        (root, snapshot, project.roots[0].id.clone())
    }

    fn recovered(root_id: &str, path: &str, revision: u64) -> RecoveredEditorBuffer {
        RecoveredEditorBuffer {
            resource: ResourceRef {
                root_id: root_id.into(),
                relative_path: path.into(),
            },
            revision,
        }
    }

    #[test]
    fn initial_bootstrap_defers_repository_status() {
        let root = std::env::temp_dir().join(format!("sage-initial-bootstrap-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        database.create_project_from_root(&root).unwrap();
        let state = AppState::new(database);

        let snapshot = build_initial_bootstrap_snapshot(
            &state.database,
            &state.harnesses,
            state.startup_recovery_notice.as_deref(),
            0,
        )
        .unwrap();

        assert!(snapshot.repositories.roots.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_uses_unique_ids_when_a_persisted_tab_collides() {
        let (root, mut snapshot, root_id) = snapshot();
        let recovery = recovered(&root_id, "dirty.txt", 1);
        snapshot.tabs[0].id = format!("recovered-file:{root_id}:dirty.txt");
        snapshot.recovered_buffers = vec![recovery];
        merge_recovered_file_tabs(&mut snapshot);
        assert_eq!(snapshot.tabs.len(), 2);
        assert_ne!(snapshot.tabs[0].id, snapshot.tabs[1].id);
        assert!(snapshot.tabs[1].id.ends_with(":1"));
        validate_tab_projection(&snapshot.tabs).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_preserves_bounded_projection_and_reports_hidden_dirty_buffers() {
        let (root, mut snapshot, root_id) = snapshot();
        for index in 0..MAX_RESOURCE_TABS {
            snapshot.tabs.push(WorkbenchTab {
                id: format!("file-{index}"),
                kind: "file".into(),
                title: format!("{index}.txt"),
                position: index as i64 + 1,
                pinned: false,
                dirty: false,
                status: None,
                updated_at: None,
                finished_at: None,
                resource: Some(ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: format!("{index}.txt"),
                }),
                repository: None,
                diff_ref: None,
            });
        }
        snapshot.recovered_buffers = vec![
            recovered(&root_id, "missing-a.txt", 1),
            recovered(&root_id, "missing-b.txt", 2),
        ];
        let durable = snapshot.recovered_buffers.clone();
        merge_recovered_file_tabs(&mut snapshot);
        assert_eq!(
            snapshot
                .tabs
                .iter()
                .filter(|tab| tab.kind == "file" || tab.kind == "diff")
                .count(),
            MAX_RESOURCE_TABS
        );
        assert_eq!(snapshot.recovered_buffers, durable);
        assert!(
            snapshot
                .recovery_notice
                .as_deref()
                .unwrap()
                .contains("2 dirty buffers")
        );
        validate_tab_projection(&snapshot.tabs).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_never_exceeds_the_total_tab_cap() {
        let (root, mut snapshot, root_id) = snapshot();
        for index in 1..MAX_RESTORED_TABS {
            snapshot.tabs.push(WorkbenchTab {
                id: format!("utility-{index}"),
                kind: "output".into(),
                title: format!("Output {index}"),
                position: index as i64,
                pinned: false,
                dirty: false,
                status: None,
                updated_at: None,
                finished_at: None,
                resource: None,
                repository: None,
                diff_ref: None,
            });
        }
        snapshot.recovered_buffers = vec![recovered(&root_id, "orphan.txt", 1)];
        merge_recovered_file_tabs(&mut snapshot);
        assert_eq!(snapshot.tabs.len(), MAX_RESTORED_TABS);
        assert!(snapshot.recovery_notice.is_some());
        validate_tab_projection(&snapshot.tabs).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_root_orphan_buffers_survive_bounded_recovery_and_restart() {
        let base = std::env::temp_dir().join(format!("sage-recovery-restart-{}", Uuid::new_v4()));
        let root = base.join("root");
        fs::create_dir_all(&root).unwrap();
        for index in 0..MAX_RESOURCE_TABS {
            fs::write(root.join(format!("{index}.txt")), "clean\n").unwrap();
        }
        let database_path = base.join("sage.db");
        let database = Database::open(&database_path).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = project.roots[0].id.clone();
        let initial = database.bootstrap_snapshot().unwrap();
        let mut tabs = initial.tabs;
        for index in 0..MAX_RESOURCE_TABS {
            tabs.push(WorkbenchTab {
                id: format!("persisted-file-{index}"),
                kind: "file".into(),
                title: format!("{index}.txt"),
                position: index as i64 + 1,
                pinned: false,
                dirty: false,
                status: None,
                updated_at: None,
                finished_at: None,
                resource: Some(ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: format!("{index}.txt"),
                }),
                repository: None,
                diff_ref: None,
            });
        }
        database
            .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                project_id: project.id.clone(),
                tabs,
                active_tab_id: initial.active_tab_id,
                selected_root_id: Some(root_id.clone()),
                selected_resource: None,
            })
            .unwrap();
        for (index, path) in ["orphan-a.txt", "orphan-b.txt"].into_iter().enumerate() {
            database
                .connection()
                .execute(
                    "INSERT INTO editor_buffers
                     (project_id, root_id, relative_path, base_fingerprint,
                      base_byte_len, base_modified_at, line_ending, buffer_content,
                      buffer_byte_len, buffer_fingerprint, revision,
                      pending_temp_path, updated_at)
                     VALUES (?1, ?2, ?3, 'base', 0, NULL, 'lf', 'dirty', 5,
                             'buffer', ?4, NULL, ?4)",
                    rusqlite::params![project.id, root_id, path, index as i64 + 1],
                )
                .unwrap();
        }
        drop(database);
        fs::remove_dir_all(&root).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        for _ in 0..2 {
            let state = AppState::new(Database::open(&database_path).unwrap());
            let restored = runtime
                .block_on(build_bootstrap_snapshot(&state, 0))
                .unwrap();
            assert_eq!(restored.recovered_buffers.len(), 2);
            assert_eq!(
                restored
                    .tabs
                    .iter()
                    .filter(|tab| tab.kind == "file" || tab.kind == "diff")
                    .count(),
                MAX_RESOURCE_TABS
            );
            assert!(restored.recovery_notice.is_some());
            let rows: i64 = state
                .database
                .connection()
                .query_row("SELECT COUNT(*) FROM editor_buffers", [], |row| row.get(0))
                .unwrap();
            assert_eq!(rows, 2);
        }
        fs::remove_dir_all(base).unwrap();
    }
}
