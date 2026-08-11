use std::{fs, path::Path, sync::Arc};

use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::{
    domain::{
        FileRenameOutcome, ProjectLayoutRequest, RenameProjectFileRequest, ResourceRef,
        WorkbenchTab,
    },
    persistence::{Database, now_millis},
    state::AppState,
};

struct Fixture {
    root: std::path::PathBuf,
    state: Arc<AppState>,
    project_id: String,
    root_id: String,
    layout: ProjectLayoutRequest,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-rename-command-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/old.rs"), "preserve\n").unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let layout = ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: Some(project.roots[0].id.clone()),
            selected_resource: Some(ResourceRef {
                root_id: project.roots[0].id.clone(),
                relative_path: "src/old.rs".into(),
            }),
        };
        database.persist_project_layout(&layout).unwrap();
        Self {
            root,
            state: Arc::new(AppState::new(database)),
            project_id: project.id,
            root_id: project.roots[0].id.clone(),
            layout,
        }
    }

    fn request(&self, basename: &str) -> RenameProjectFileRequest {
        RenameProjectFileRequest {
            project_id: self.project_id.clone(),
            project_generation: 7,
            expected_transition_epoch: 0,
            source: ResourceRef {
                root_id: self.root_id.clone(),
                relative_path: "src/old.rs".into(),
            },
            basename: basename.into(),
            current_layout: self.layout.clone(),
        }
    }

    fn resource_tab(&self, id: &str, kind: &str, relative_path: &str) -> WorkbenchTab {
        WorkbenchTab {
            id: id.into(),
            kind: kind.into(),
            title: relative_path.into(),
            position: self.layout.tabs.len() as i64,
            pinned: false,
            dirty: false,
            status: None,
            resource: Some(ResourceRef {
                root_id: self.root_id.clone(),
                relative_path: relative_path.into(),
            }),
        }
    }

    fn insert_buffer(&self, relative_path: &str) {
        self.state
            .database
            .write_connection()
            .unwrap()
            .execute(
                "INSERT INTO editor_buffers(
                   project_id,root_id,relative_path,base_fingerprint,base_byte_len,
                   base_modified_at,line_ending,buffer_content,buffer_byte_len,
                   buffer_fingerprint,revision,pending_temp_path,updated_at
                 ) VALUES(?1,?2,?3,'base',0,NULL,'none','',0,'buffer',1,NULL,?4)",
                params![self.project_id, self.root_id, relative_path, now_millis()],
            )
            .unwrap();
    }
}

#[test]
fn durable_source_or_target_tabs_cannot_be_omitted_from_the_request() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for (kind, relative_path) in [
            ("file", "src/old.rs"),
            ("diff", "src/old.rs"),
            ("file", "src/new.rs"),
            ("diff", "src/new.rs"),
        ] {
            let fixture = Fixture::new();
            let mut durable_layout = fixture.layout.clone();
            durable_layout.tabs.push(fixture.resource_tab(
                &format!("durable:{kind}:{relative_path}"),
                kind,
                relative_path,
            ));
            fixture
                .state
                .database
                .persist_project_layout(&durable_layout)
                .unwrap();

            let error = execute(&fixture.state, fixture.request("new.rs"))
                .await
                .unwrap_err();

            assert!(error.to_string().contains("close File and Diff tabs"));
            assert_eq!(
                fs::read_to_string(fixture.root.join("src/old.rs")).unwrap(),
                "preserve\n"
            );
            assert!(!fixture.root.join("src/new.rs").exists());
        }
    });
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn commits_exact_file_and_migrates_only_the_selected_resource() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();

        let result = execute(&fixture.state, fixture.request("new.rs"))
            .await
            .unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::Committed);
        assert!(result.resource_confirmed);
        assert_eq!(result.project_generation, 7);
        assert_eq!(result.transition_epoch, 0);
        assert_eq!(result.source.relative_path, "src/old.rs");
        assert_eq!(result.target.relative_path, "src/new.rs");
        assert_eq!(
            result.snapshot.selected_resource.as_ref(),
            Some(&result.target)
        );
        assert!(
            result
                .parent_listing
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .any(|entry| {
                    entry.resource == result.target
                        && entry.kind == "file"
                        && entry.byte_len == Some(9)
                })
        );
        assert!(!fixture.root.join("src/old.rs").exists());
        assert_eq!(
            fs::read_to_string(fixture.root.join("src/new.rs")).unwrap(),
            "preserve\n"
        );
    });
}

#[test]
fn source_or_target_file_and_diff_tabs_fail_before_filesystem_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for (kind, relative_path) in [
            ("file", "src/old.rs"),
            ("diff", "src/old.rs"),
            ("file", "src/new.rs"),
            ("diff", "src/new.rs"),
        ] {
            let fixture = Fixture::new();
            let mut request = fixture.request("new.rs");
            request.current_layout.tabs.push(fixture.resource_tab(
                &format!("{kind}:{relative_path}"),
                kind,
                relative_path,
            ));
            let error = execute(&fixture.state, request).await.unwrap_err();
            assert!(error.to_string().contains("close File and Diff tabs"));
            assert_eq!(
                fs::read_to_string(fixture.root.join("src/old.rs")).unwrap(),
                "preserve\n"
            );
            assert!(!fixture.root.join("src/new.rs").exists());
        }
    });
}

#[test]
fn source_or_target_editor_buffers_fail_before_filesystem_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for relative_path in ["src/old.rs", "src/new.rs"] {
            let fixture = Fixture::new();
            fixture.insert_buffer(relative_path);

            let error = execute(&fixture.state, fixture.request("new.rs"))
                .await
                .unwrap_err();

            assert!(error.to_string().contains("editor buffer"));
            assert!(fixture.root.join("src/old.rs").is_file());
            assert!(!fixture.root.join("src/new.rs").exists());
        }
    });
}

#[test]
fn post_filesystem_transaction_failure_recovers_as_effect_unknown() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();

        let result = execute_with(&fixture.state, fixture.request("new.rs"), |_| {
            Err(AppError::InvalidRequest(
                "injected post-rename transaction failure".into(),
            ))
        })
        .await
        .unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::EffectUnknown);
        assert!(result.resource_confirmed);
        assert!(result.warning.as_deref().unwrap().contains("interrupted"));
        assert_eq!(result.snapshot.selected_resource, Some(result.target));
        assert!(!fixture.root.join("src/old.rs").exists());
        assert!(fixture.root.join("src/new.rs").is_file());
    });
}

#[test]
fn stale_epoch_and_foreign_root_fail_before_filesystem_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        *fixture.state.project_transitions.lock().await = 1;
        let stale = execute(&fixture.state, fixture.request("new.rs"))
            .await
            .unwrap_err();
        assert!(stale.to_string().contains("project changed"));
        assert!(fixture.root.join("src/old.rs").is_file());

        *fixture.state.project_transitions.lock().await = 0;
        let mut foreign = fixture.request("new.rs");
        foreign.source.root_id = "foreign-root".into();
        let error = execute(&fixture.state, foreign).await.unwrap_err();
        assert!(error.to_string().contains("does not belong"));
        assert!(fixture.root.join("src/old.rs").is_file());
        assert!(!fixture.root.join("src/new.rs").exists());
    });
}
