use std::{fs, path::Path, sync::Arc};

use uuid::Uuid;

use super::*;
use crate::{
    domain::{
        ProjectLayoutRequest, ResourceRef, ResourceTrashOutcome, TrashProjectResourceRequest,
        TrashResourceKind, WorkbenchTab,
    },
    persistence::Database,
    state::AppState,
    workspace::{
        Workspace,
        resource_trash::{ResourceTrashCommit, ResourceTrashIdentity, ResourceTrashMarker},
    },
};

fn trash_with_fixture(
    workspace: &Workspace,
    request: &TrashProjectResourceRequest,
    _identity: ResourceTrashIdentity,
    marker: Arc<ResourceTrashMarker>,
) -> AppResult<ResourceTrashCommit> {
    let parent = workspace.preflight_resource_trash(&request.source, request.kind)?;
    marker.record_mutation_started();
    let source_path = workspace.root().join(&request.source.relative_path);
    match request.kind {
        TrashResourceKind::File => fs::remove_file(source_path)?,
        TrashResourceKind::EmptyFolder => fs::remove_dir(source_path)?,
    }
    let parent_listing = workspace.list_directory(&parent)?;
    Ok(ResourceTrashCommit {
        source: request.source.clone(),
        parent,
        resource_confirmed: true,
        parent_listing: Some(parent_listing),
        warning: None,
    })
}

struct Fixture {
    root: std::path::PathBuf,
    state: Arc<AppState>,
    project_id: String,
    root_id: String,
    layout: ProjectLayoutRequest,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-trash-command-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src/empty")).unwrap();
        fs::write(root.join("src/closed.rs"), "preserve\n").unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let root_id = project.roots[0].id.clone();
        let layout = ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: Some(root_id.clone()),
            selected_resource: Some(ResourceRef {
                root_id: root_id.clone(),
                relative_path: "src/closed.rs".into(),
            }),
        };
        database.persist_project_layout(&layout).unwrap();
        Self {
            root,
            state: Arc::new(AppState::new(database)),
            project_id: project.id,
            root_id,
            layout,
        }
    }

    fn file_request(&self) -> TrashProjectResourceRequest {
        TrashProjectResourceRequest {
            project_id: self.project_id.clone(),
            project_generation: 7,
            expected_transition_epoch: 0,
            source: ResourceRef {
                root_id: self.root_id.clone(),
                relative_path: "src/closed.rs".into(),
            },
            kind: TrashResourceKind::File,
            current_layout: self.layout.clone(),
        }
    }

    fn descendant_tab(&self) -> WorkbenchTab {
        WorkbenchTab {
            id: "descendant".into(),
            kind: "diff".into(),
            title: "ghost.rs".into(),
            position: self.layout.tabs.len() as i64,
            pinned: false,
            dirty: false,
            status: None,
            updated_at: None,
            finished_at: None,
            resource: Some(ResourceRef {
                root_id: self.root_id.clone(),
                relative_path: "src/empty/ghost.rs".into(),
            }),
            repository: None,
            diff_ref: None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn persist_fixture_intent(fixture: &Fixture) {
    let request = fixture.file_request();
    let workspace = fixture
        .state
        .database
        .resolve_active_resource(&request.source)
        .unwrap();
    let identity = workspace
        .resource_trash_identity(&request.source, request.kind)
        .unwrap();
    intent::persist(
        &fixture.state.database,
        &intent::PendingTrashIntent::new(&request, identity),
    )
    .unwrap();
}

fn pending_intent_count(fixture: &Fixture) -> i64 {
    fixture
        .state
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM resource_trash_intents", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn restart_before_effect_clears_intent_once_without_changing_the_resource() {
    let fixture = Fixture::new();
    persist_fixture_intent(&fixture);

    let recovered = intent::recover_at_startup(&fixture.state.database)
        .unwrap()
        .unwrap();

    assert_eq!(recovered.transition_epoch, 1);
    assert!(recovered.notice.contains("EffectUnknown"));
    assert!(fixture.root.join("src/closed.rs").is_file());
    assert_eq!(pending_intent_count(&fixture), 0);
    assert!(
        intent::recover_at_startup(&fixture.state.database)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .state
            .database
            .bootstrap_snapshot()
            .unwrap()
            .selected_resource,
        Some(fixture.file_request().source)
    );
}

#[test]
fn restart_during_effect_preserves_an_identity_mismatched_replacement() {
    let fixture = Fixture::new();
    persist_fixture_intent(&fixture);
    fs::rename(
        fixture.root.join("src/closed.rs"),
        fixture.root.join("src/original.rs"),
    )
    .unwrap();
    fs::write(fixture.root.join("src/closed.rs"), "replacement\n").unwrap();

    let recovered = intent::recover_at_startup(&fixture.state.database)
        .unwrap()
        .unwrap();

    assert!(recovered.notice.contains("identity could not be confirmed"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/closed.rs")).unwrap(),
        "replacement\n"
    );
    assert_eq!(pending_intent_count(&fixture), 0);
    assert_eq!(
        fixture
            .state
            .database
            .bootstrap_snapshot()
            .unwrap()
            .selected_resource,
        Some(fixture.file_request().source)
    );
}

#[cfg(any(windows, unix))]
#[test]
fn restart_during_effect_preserves_a_dangling_reparse_replacement() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink as symlink_file;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file;

    let fixture = Fixture::new();
    persist_fixture_intent(&fixture);
    let source = fixture.root.join("src/closed.rs");
    fs::rename(&source, fixture.root.join("src/original.rs")).unwrap();
    symlink_file(fixture.root.join("src/missing-target.rs"), &source)
        .expect("dangling-link regression requires a creatable file symlink");

    let recovered = intent::recover_at_startup(&fixture.state.database)
        .unwrap()
        .unwrap();

    assert!(recovered.notice.contains("identity could not be confirmed"));
    assert!(
        fs::symlink_metadata(&source)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(pending_intent_count(&fixture), 0);
    assert_eq!(
        fixture
            .state
            .database
            .bootstrap_snapshot()
            .unwrap()
            .selected_resource,
        Some(fixture.file_request().source)
    );
    assert!(
        intent::recover_at_startup(&fixture.state.database)
            .unwrap()
            .is_none()
    );
}

#[test]
fn restart_after_effect_repairs_selection_and_never_applies_twice() {
    let fixture = Fixture::new();
    persist_fixture_intent(&fixture);
    fs::remove_file(fixture.root.join("src/closed.rs")).unwrap();

    let recovered = intent::recover_at_startup(&fixture.state.database)
        .unwrap()
        .unwrap();

    assert!(recovered.notice.contains("selection was repaired"));
    assert_eq!(pending_intent_count(&fixture), 0);
    assert_eq!(
        fixture
            .state
            .database
            .bootstrap_snapshot()
            .unwrap()
            .selected_resource,
        Some(ResourceRef {
            root_id: fixture.root_id.clone(),
            relative_path: "src".into(),
        })
    );
    assert!(
        intent::recover_at_startup(&fixture.state.database)
            .unwrap()
            .is_none()
    );
}

#[test]
fn committed_file_trash_increments_epoch_and_selects_parent() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let result = execute_with_operation(
            &fixture.state,
            fixture.file_request(),
            trash_with_fixture,
            |_| Ok(()),
            || Ok(()),
        )
        .await
        .unwrap();

        assert_eq!(result.outcome, ResourceTrashOutcome::Committed);
        assert!(result.resource_confirmed);
        assert_eq!(result.transition_epoch, 1);
        assert_eq!(result.parent.relative_path, "src");
        assert_eq!(
            result.snapshot.unwrap().selected_resource,
            Some(result.parent)
        );
        assert!(!fixture.root.join("src/closed.rs").exists());
    });
}

#[test]
fn adapter_completion_without_mutation_marker_clears_intent_and_fails_closed() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let error = execute_with_operation(
            &fixture.state,
            fixture.file_request(),
            |workspace, request, _identity, _marker| {
                let parent = workspace.preflight_resource_trash(&request.source, request.kind)?;
                Ok(ResourceTrashCommit {
                    source: request.source.clone(),
                    parent,
                    resource_confirmed: true,
                    parent_listing: None,
                    warning: None,
                })
            },
            |_| Ok(()),
            || Ok(()),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("mutation boundary"));
        assert!(fixture.root.join("src/closed.rs").is_file());
        assert_eq!(pending_intent_count(&fixture), 0);
        assert_eq!(*fixture.state.project_transitions.lock().await, 0);
    });
}

#[test]
fn open_descendant_diff_blocks_empty_folder_before_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let mut request = fixture.file_request();
        request.source.relative_path = "src/empty".into();
        request.kind = TrashResourceKind::EmptyFolder;
        request.current_layout.selected_resource = Some(request.source.clone());
        request.current_layout.tabs.push(fixture.descendant_tab());

        let error = execute_with_operation(
            &fixture.state,
            request,
            trash_with_fixture,
            |_| Ok(()),
            || Ok(()),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("close File and Diff tabs"));
        assert!(fixture.root.join("src/empty").is_dir());
        assert_eq!(*fixture.state.project_transitions.lock().await, 0);
    });
}

#[test]
fn post_mutation_transaction_failure_is_typed_effect_unknown_and_increments_once() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let result = execute_with_operation(
            &fixture.state,
            fixture.file_request(),
            trash_with_fixture,
            |_| {
                Err(AppError::InvalidRequest(
                    "injected post-trash transaction failure".into(),
                ))
            },
            || Ok(()),
        )
        .await
        .unwrap();

        assert_eq!(result.outcome, ResourceTrashOutcome::EffectUnknown);
        assert!(!result.resource_confirmed);
        assert_eq!(result.transition_epoch, 1);
        assert!(
            result
                .warning
                .as_deref()
                .unwrap()
                .contains("recovery failed")
        );
        assert!(!fixture.root.join("src/closed.rs").exists());
    });
}

#[test]
fn stale_epoch_fails_before_the_test_operation_mutates_the_file() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        *fixture.state.project_transitions.lock().await = 1;

        let error = execute_with_operation(
            &fixture.state,
            fixture.file_request(),
            trash_with_fixture,
            |_| Ok(()),
            || Ok(()),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("Project changed"));
        assert!(fixture.root.join("src/closed.rs").is_file());
    });
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_platform_returns_typed_unchanged_result() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let result = execute(&fixture.state, fixture.file_request())
            .await
            .unwrap();

        assert_eq!(result.outcome, ResourceTrashOutcome::Unsupported);
        assert!(!result.resource_confirmed);
        assert_eq!(result.transition_epoch, 0);
        assert!(result.parent_listing.is_none());
        assert!(result.snapshot.is_none());
        assert!(fixture.root.join("src/closed.rs").is_file());
    });
}

#[test]
fn committed_trash_stops_the_previous_epoch_preview_but_a_stale_one_does_not() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let preview = crate::app_preview_commands::ServingPreview::start(0);

        let mut stale = fixture.file_request();
        stale.expected_transition_epoch = 1;
        let rejected = execute_with_operation(
            &fixture.state,
            stale,
            trash_with_fixture,
            |_| Ok(()),
            || Ok(()),
        )
        .await;
        assert!(rejected.is_err());
        assert!(
            preview.is_serving(),
            "a stale trash request must not stop the current Preview"
        );

        let result = execute_with_operation(
            &fixture.state,
            fixture.file_request(),
            trash_with_fixture,
            |_| Ok(()),
            || Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, ResourceTrashOutcome::Committed);
        assert_eq!(result.transition_epoch, 1);
        assert!(!preview.is_serving());
    });
}
