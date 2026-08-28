use std::{
    fs,
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};

use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::{
    domain::{
        FileRenameOutcome, ProjectLayoutRequest, RenameProjectFileRequest, ResourceRef,
        WorkbenchTab,
    },
    editor_coordination::EditorOperationKind,
    persistence::{Database, now_millis},
    project_commands::persist_project_layout_inner,
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
            updated_at: None,
            resource: Some(ResourceRef {
                root_id: self.root_id.clone(),
                relative_path: relative_path.into(),
            }),
            repository: None,
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

#[cfg(windows)]
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
        assert_eq!(result.transition_epoch, 1);
        assert_eq!(result.source.relative_path, "src/old.rs");
        assert_eq!(result.target.relative_path, "src/new.rs");
        assert_eq!(
            result.warning.as_deref(),
            Some("File was renamed, but its durability could not be confirmed.")
        );
        assert_eq!(
            result.snapshot.as_ref().unwrap().selected_resource.as_ref(),
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

#[cfg(windows)]
#[test]
fn commits_an_empty_folder_and_rejects_a_non_empty_folder() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        fs::create_dir(fixture.root.join("src/empty")).unwrap();
        let mut request = fixture.request("renamed");
        request.source.relative_path = "src/empty".into();
        request.current_layout.selected_resource = Some(request.source.clone());

        let result = execute_folder(&fixture.state, request).await.unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::Committed);
        assert!(result.resource_confirmed);
        assert!(fixture.root.join("src/renamed").is_dir());
        assert!(!fixture.root.join("src/empty").exists());
        assert!(result.parent_listing.unwrap().entries.iter().any(|entry| {
            entry.resource == result.target && entry.kind == "directory" && entry.byte_len.is_none()
        }));

        let fixture = Fixture::new();
        fs::create_dir(fixture.root.join("src/non-empty")).unwrap();
        fs::write(fixture.root.join("src/non-empty/child.txt"), "child").unwrap();
        let mut request = fixture.request("renamed");
        request.source.relative_path = "src/non-empty".into();
        request.current_layout.selected_resource = Some(request.source.clone());

        assert!(execute_folder(&fixture.state, request).await.is_err());
        assert!(fixture.root.join("src/non-empty/child.txt").is_file());
        assert!(!fixture.root.join("src/renamed").exists());
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
fn folder_rename_rejects_durable_descendant_file_and_diff_tabs() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for kind in ["file", "diff"] {
            let fixture = Fixture::new();
            fs::create_dir(fixture.root.join("src/empty")).unwrap();
            let mut durable_layout = fixture.layout.clone();
            durable_layout.tabs.push(fixture.resource_tab(
                &format!("{kind}:folder-descendant"),
                kind,
                "src/empty/closed.rs",
            ));
            fixture
                .state
                .database
                .persist_project_layout(&durable_layout)
                .unwrap();
            let mut request = fixture.request("renamed");
            request.source.relative_path = "src/empty".into();
            request.current_layout.selected_resource = Some(request.source.clone());

            let error = execute_folder(&fixture.state, request).await.unwrap_err();

            assert!(error.to_string().contains("close File and Diff tabs"));
            assert!(fixture.root.join("src/empty").is_dir());
            assert!(!fixture.root.join("src/renamed").exists());
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

#[cfg(windows)]
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
        assert_eq!(result.transition_epoch, 1);
        assert_eq!(
            result.snapshot.unwrap().selected_resource,
            Some(result.target)
        );
        assert!(!fixture.root.join("src/old.rs").exists());
        assert!(fixture.root.join("src/new.rs").is_file());
    });
}

#[cfg(windows)]
#[test]
fn replacement_target_is_never_confirmed_or_selected_during_recovery() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let target = fixture.root.join("src/new.rs");

        let result = execute_with(&fixture.state, fixture.request("new.rs"), move |_| {
            fs::remove_file(&target)?;
            fs::write(&target, "replacement\n")?;
            Err(AppError::InvalidRequest(
                "injected post-rename transaction failure".into(),
            ))
        })
        .await
        .unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::EffectUnknown);
        assert!(!result.resource_confirmed);
        assert_eq!(
            result.snapshot.unwrap().selected_resource,
            Some(result.source)
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("src/new.rs")).unwrap(),
            "replacement\n"
        );
    });
}

#[cfg(windows)]
#[test]
fn recovery_failure_stays_typed_with_a_safe_snapshot_envelope() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();

        let result = execute_with_hooks(
            &fixture.state,
            fixture.request("new.rs"),
            |_| {
                Err(AppError::InvalidRequest(
                    "injected post-rename transaction failure".into(),
                ))
            },
            || Err(AppError::InvalidRequest("injected recovery failure".into())),
            || Ok(()),
        )
        .await
        .unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::EffectUnknown);
        assert!(!result.resource_confirmed);
        assert!(result.snapshot.is_some());
        assert!(
            result
                .warning
                .as_deref()
                .unwrap()
                .contains("recovery failed")
        );
        assert_eq!(result.transition_epoch, 1);
    });
}

#[cfg(windows)]
#[test]
fn snapshot_failure_after_commit_stays_typed_with_parent_evidence() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();

        let result = execute_with_hooks(
            &fixture.state,
            fixture.request("new.rs"),
            |_| Ok(()),
            || Ok(()),
            || Err(AppError::InvalidRequest("injected snapshot failure".into())),
        )
        .await
        .unwrap();

        assert_eq!(result.outcome, FileRenameOutcome::EffectUnknown);
        assert!(result.resource_confirmed);
        assert!(result.parent_listing.is_some());
        assert!(result.snapshot.is_none());
        assert!(result.warning.as_deref().unwrap().contains("snapshot"));
        assert_eq!(result.transition_epoch, 1);
    });
}

#[cfg(windows)]
#[test]
fn committed_rename_rejects_chat_and_window_layouts_queued_at_the_old_epoch() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let rename_state = Arc::clone(&fixture.state);
        let request = fixture.request("new.rs");
        let rename = tokio::spawn(async move {
            execute_with(&rename_state, request, move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            })
            .await
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let stale_window_state = Arc::clone(&fixture.state);
        let mut stale_window_layout = fixture.layout.clone();
        stale_window_layout.tabs.push(fixture.resource_tab(
            "stale-window-source",
            "file",
            "src/old.rs",
        ));
        let window = tokio::spawn(async move {
            persist_project_layout_inner(&stale_window_state, Some(stale_window_layout), 0).await
        });
        let stale_chat_state = Arc::clone(&fixture.state);
        let stale_chat_layout = fixture.layout.clone();
        let chat = tokio::spawn(async move {
            crate::chat_commands::execute_chat_transition(
                &stale_chat_state,
                0,
                "stale queued Chat layout",
                move |database| database.create_chat(&stale_chat_layout).map(drop),
            )
            .await
        });
        tokio::task::yield_now().await;
        release_tx.send(()).unwrap();

        let renamed = rename.await.unwrap().unwrap();
        let window = window.await.unwrap();
        let chat = chat.await.unwrap();
        assert_eq!(renamed.transition_epoch, 1);
        assert!(window.is_err());
        assert!(!chat.committed);
        assert_eq!(chat.transition_epoch, 1);
        let snapshot = fixture.state.database.bootstrap_snapshot().unwrap();
        assert_eq!(snapshot.selected_resource, Some(renamed.target));
        assert!(!snapshot.tabs.iter().any(|tab| {
            matches!(tab.kind.as_str(), "file" | "diff")
                && tab.resource.as_ref() == Some(&renamed.source)
        }));
    });
}

#[cfg(windows)]
#[test]
fn rename_and_editor_snapshot_are_serialized_in_ticket_order() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let old_snapshot_ticket = fixture.state.editor_operations.issue_ticket().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let rename_state = Arc::clone(&fixture.state);
        let request = fixture.request("new.rs");
        let rename = tokio::spawn(async move {
            execute_with(&rename_state, request, move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            })
            .await
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let operations = fixture.state.editor_operations.clone();
        let database = fixture.state.database.clone();
        let project_id = fixture.project_id.clone();
        let root_id = fixture.root_id.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            operations.execute(old_snapshot_ticket, EditorOperationKind::Snapshot, || {
                database.write_connection()?.execute(
                    "INSERT INTO editor_buffers(
                       project_id,root_id,relative_path,base_fingerprint,base_byte_len,
                       base_modified_at,line_ending,buffer_content,buffer_byte_len,
                       buffer_fingerprint,revision,pending_temp_path,updated_at
                     ) VALUES(?1,?2,'src/old.rs','base',0,NULL,'none','',0,'buffer',1,NULL,?3)",
                    params![project_id, root_id, now_millis()],
                )?;
                Ok(())
            })
        });
        release_tx.send(()).unwrap();

        assert!(rename.await.unwrap().unwrap().resource_confirmed);
        assert!(snapshot.await.unwrap().is_err());
        assert!(
            !fixture
                .state
                .database
                .bootstrap_snapshot()
                .unwrap()
                .recovered_buffers
                .iter()
                .any(|buffer| buffer.resource.relative_path == "src/old.rs")
        );
    });
}

#[test]
fn editor_snapshot_that_wins_admission_blocks_rename_before_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let snapshot_ticket = fixture.state.editor_operations.issue_ticket().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let operations = fixture.state.editor_operations.clone();
        let database = fixture.state.database.clone();
        let project_id = fixture.project_id.clone();
        let root_id = fixture.root_id.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            operations.execute(snapshot_ticket, EditorOperationKind::Snapshot, || {
                database.write_connection()?.execute(
                    "INSERT INTO editor_buffers(
                       project_id,root_id,relative_path,base_fingerprint,base_byte_len,
                       base_modified_at,line_ending,buffer_content,buffer_byte_len,
                       buffer_fingerprint,revision,pending_temp_path,updated_at
                     ) VALUES(?1,?2,'src/old.rs','base',0,NULL,'none','',0,'buffer',1,NULL,?3)",
                    params![project_id, root_id, now_millis()],
                )?;
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            })
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let rename_state = Arc::clone(&fixture.state);
        let request = fixture.request("new.rs");
        let rename = tokio::spawn(async move { execute(&rename_state, request).await });
        tokio::task::yield_now().await;
        release_tx.send(()).unwrap();

        snapshot.await.unwrap().unwrap();
        let error = rename.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("editor buffer"));
        assert!(fixture.root.join("src/old.rs").is_file());
        assert!(!fixture.root.join("src/new.rs").exists());
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
