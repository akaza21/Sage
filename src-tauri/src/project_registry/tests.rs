use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use uuid::Uuid;

use super::create::MAX_PROJECT_NAME_CHARS;
use super::*;
use crate::chat_lifecycle::store::ChatLifecycleStore;
use crate::editor_persistence::store::EditorBufferStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use crate::{editor, state::AppState};
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId, SageRootId};
use sage_db::database::Database;
use sage_domain::{chat::ProjectLayoutRequest, project::ProjectSwitchRequest};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

#[test]
fn process_root_authority_rejects_unknown_and_foreign_roots() {
    runtime().block_on(async {
        let folders = Folders::new();
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let foreign = database
            .create_project_from_root(&folders.root("foreign"))
            .unwrap();
        let active = database
            .create_project_from_root(&folders.root("active"))
            .unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(active.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();

        assert!(
            database
                .project_root_authority(&context, &SageRootId::parse("root-unknown").unwrap())
                .is_err()
        );
        assert!(
            database
                .project_root_authority(
                    &context,
                    &SageRootId::parse(foreign.roots[0].id.clone()).unwrap()
                )
                .is_err()
        );
    });
}

#[test]
fn process_root_authority_rejects_noncanonical_alias_and_missing_root() {
    runtime().block_on(async {
        let folders = Folders::new();
        let root = folders.root("root");
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = SageRootId::parse(project.roots[0].id.clone()).unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();

        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?2 WHERE id=?1",
                rusqlite::params![root_id.as_str(), root.join(".").to_string_lossy()],
            )
            .unwrap();
        assert!(database.project_root_authority(&context, &root_id).is_err());

        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?2 WHERE id=?1",
                rusqlite::params![root_id.as_str(), root.to_string_lossy()],
            )
            .unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(database.project_root_authority(&context, &root_id).is_err());
    });
}

#[test]
fn retained_process_root_detects_or_blocks_directory_substitution() {
    runtime().block_on(async {
        let folders = Folders::new();
        let root = folders.root("root");
        let moved = folders.base.join("moved");
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let authority = database.selected_project_root_authority(&context).unwrap();

        match fs::rename(&root, &moved) {
            Ok(()) => {
                fs::create_dir_all(&root).unwrap();
                assert!(authority.revalidate().is_err());
            }
            Err(_) => {
                // Windows retains the directory without FILE_SHARE_DELETE,
                // so ordinary replacement is blocked while authority lives.
                assert!(authority.revalidate().is_ok());
            }
        }
        drop(authority);
    });
}

#[cfg(unix)]
#[test]
fn process_root_authority_rejects_registered_symlink_alias() {
    use std::os::unix::fs::symlink;

    runtime().block_on(async {
        let folders = Folders::new();
        let root = folders.root("root");
        let alias = folders.base.join("alias");
        symlink(&root, &alias).unwrap();
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = SageRootId::parse(project.roots[0].id.clone()).unwrap();
        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?2 WHERE id=?1",
                rusqlite::params![root_id.as_str(), alias.to_string_lossy()],
            )
            .unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();

        assert!(database.project_root_authority(&context, &root_id).is_err());
    });
}

#[cfg(windows)]
#[test]
fn process_root_authority_rejects_registered_windows_directory_alias() {
    runtime().block_on(async {
        let folders = Folders::new();
        let root = folders.root("root");
        let alias = folders.base.join("alias");
        if std::os::windows::fs::symlink_dir(&root, &alias).is_err() {
            return;
        }
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = SageRootId::parse(project.roots[0].id.clone()).unwrap();
        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?2 WHERE id=?1",
                rusqlite::params![root_id.as_str(), alias.to_string_lossy()],
            )
            .unwrap();
        let state = AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();

        assert!(database.project_root_authority(&context, &root_id).is_err());
    });
}

struct Folders {
    base: PathBuf,
}

impl Folders {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-projects-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        Self { base }
    }

    fn root(&self, name: &str) -> PathBuf {
        let root = self.base.join(name);
        fs::create_dir_all(&root).unwrap();
        root
    }
}

impl Drop for Folders {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn layout(snapshot: &sage_domain::project::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn duplicate_canonical_root_is_rejected_and_nested_root_is_allowed() {
    let folders = Folders::new();
    let outer = folders.root("outer");
    let nested = outer.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&outer).unwrap();

    assert!(database.add_project_root(&project.id, &outer).is_err());
    let project = database.add_project_root(&project.id, &nested).unwrap();
    assert_eq!(project.roots.len(), 2);
    assert_ne!(project.roots[0].id, project.roots[1].id);
}

#[test]
fn root_limit_is_rejected_without_losing_existing_roots() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database
        .create_project_from_root(&folders.root("root-0"))
        .unwrap();
    for index in 1..MAX_ROOTS_PER_PROJECT {
        database
            .add_project_root(&project.id, &folders.root(&format!("root-{index}")))
            .unwrap();
    }
    assert!(
        database
            .add_project_root(&project.id, &folders.root("overflow"))
            .is_err()
    );
    assert_eq!(
        database.project_summary(&project.id).unwrap().roots.len(),
        MAX_ROOTS_PER_PROJECT as usize
    );
}

#[test]
fn project_limit_is_rejected_without_losing_the_registry() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    for index in 0..MAX_PROJECTS {
        database
            .create_project_from_root(&folders.root(&format!("project-{index}")))
            .unwrap();
    }
    assert!(
        database
            .create_project_from_root(&folders.root("project-overflow"))
            .is_err()
    );
    assert_eq!(database.project_count().unwrap(), MAX_PROJECTS);
}

#[cfg(target_os = "linux")]
fn filesystem_rejected_invalid_name(error: &std::io::Error) -> bool {
    const LINUX_EILSEQ: i32 = 84;
    error.kind() == std::io::ErrorKind::InvalidFilename
        || error.raw_os_error() == Some(LINUX_EILSEQ)
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_selected_root_is_rejected_before_sqlite_persistence() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let folders = Folders::new();
    let root = folders
        .base
        .join(OsString::from_vec(b"invalid-\xff".to_vec()));
    match fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if filesystem_rejected_invalid_name(&error) => {
            eprintln!(
                "skipping raw invalid-byte root fixture: filesystem rejected the test name: {error}"
            );
            return;
        }
        Err(error) => panic!("raw invalid-byte root setup failed unexpectedly: {error}"),
    }
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();

    assert!(matches!(
        database.create_project_from_root(&root),
        Err(AppError::InvalidRequest(_))
    ));
    assert_eq!(database.project_count().unwrap(), 0);
}

#[cfg(unix)]
#[test]
fn non_utf8_selected_root_is_rejected_at_portable_persistence_boundary() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    // macOS CI must not require the kernel/filesystem to create this fixture.
    // The production persistence boundary still rejects its raw path bytes.
    let root = PathBuf::from(OsString::from_vec(b"invalid-\xff".to_vec()));
    assert!(matches!(
        persisted_path_text(&root),
        Err(AppError::InvalidRequest(message)) if message.contains("not valid UTF-8")
    ));
}

#[test]
fn missing_root_remains_typed_and_does_not_hide_available_roots() {
    let folders = Folders::new();
    let first = folders.root("available");
    let missing = folders.root("missing");
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&first).unwrap();
    database.add_project_root(&project.id, &missing).unwrap();
    fs::remove_dir_all(&missing).unwrap();

    let restored = database.project_summary(&project.id).unwrap();
    assert_eq!(restored.roots[0].availability, "available");
    assert_eq!(restored.roots[1].availability, "missing");
}

#[test]
fn same_relative_path_in_two_roots_has_independent_durable_buffers() {
    let folders = Folders::new();
    let left = folders.root("left");
    let right = folders.root("right");
    fs::write(left.join("src.rs"), "left\n").unwrap();
    fs::write(right.join("src.rs"), "right\n").unwrap();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&left).unwrap();
    let project = database.add_project_root(&project.id, &right).unwrap();
    let records = database.root_records(&project.id).unwrap();

    for (index, record) in records.iter().enumerate() {
        let workspace = Workspace::open(&project.id, &record.id, &record.absolute_path).unwrap();
        let resource = workspace.resource("src.rs").unwrap();
        let opened = editor::open_document(&database, &workspace, &resource).unwrap();
        editor::snapshot_buffer(
            &database,
            &workspace,
            sage_domain::editor::BufferSnapshotRequest {
                resource,
                base: opened.base,
                line_ending: opened.line_ending,
                content: format!("dirty-{index}\n"),
                revision: 1,
            },
        )
        .unwrap();
    }

    let buffers = database.dirty_editor_buffers(&project.id, None).unwrap();
    assert_eq!(buffers.len(), 2);
    assert_ne!(buffers[0].resource.root_id, buffers[1].resource.root_id);
    assert!(
        buffers
            .iter()
            .all(|buffer| buffer.resource.relative_path == "src.rs")
    );
}

#[test]
fn switching_projects_restores_layout_and_dirty_buffer_metadata() {
    let folders = Folders::new();
    let root_a = folders.root("a");
    let root_b = folders.root("b");
    fs::write(root_a.join("dirty.txt"), "base\n").unwrap();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project_a = database.create_project_from_root(&root_a).unwrap();
    let record_a = database
        .root_records(&project_a.id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let workspace_a =
        Workspace::open(&project_a.id, &record_a.id, &record_a.absolute_path).unwrap();
    let resource = workspace_a.resource("dirty.txt").unwrap();
    let opened = editor::open_document(&database, &workspace_a, &resource).unwrap();
    editor::snapshot_buffer(
        &database,
        &workspace_a,
        sage_domain::editor::BufferSnapshotRequest {
            resource,
            base: opened.base,
            line_ending: opened.line_ending,
            content: "recover me\n".into(),
            revision: 2,
        },
    )
    .unwrap();

    let project_b = database.create_project_from_root(&root_b).unwrap();
    let current = database.bootstrap_snapshot().unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: project_a.id.clone(),
            current_layout: layout(&current),
        })
        .unwrap();
    let restored = database.bootstrap_snapshot().unwrap();
    assert_eq!(restored.project.unwrap().id, project_a.id);
    let buffers = database.dirty_editor_buffers(&project_a.id, None).unwrap();
    assert_eq!(buffers[0].content, "recover me\n");
    assert_eq!(database.project_list().unwrap().len(), 2);
    assert_ne!(project_a.id, project_b.id);
}

#[test]
fn missing_active_project_reference_falls_back_deterministically() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project_a = database
        .create_project_from_root(&folders.root("fallback-a"))
        .unwrap();
    let project_b = database
        .create_project_from_root(&folders.root("fallback-b"))
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE app_state SET active_project_id = 'missing' WHERE singleton = 1",
            [],
        )
        .unwrap();

    let restored = database.bootstrap_snapshot().unwrap();
    let restored_id = restored.project.unwrap().id;
    assert!(restored_id == project_a.id || restored_id == project_b.id);
    assert_eq!(
        database.active_project_id().unwrap().as_deref(),
        Some(restored_id.as_str())
    );
}

#[test]
fn create_add_and_switch_reject_stale_project_intents_without_mutation() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project_a = database
        .create_project_from_root(&folders.root("intent-a"))
        .unwrap();
    let snapshot_a = database.bootstrap_snapshot().unwrap();
    let project_b = database
        .create_project_from_root(&folders.root("intent-b"))
        .unwrap();
    let before_projects = database.project_count().unwrap();

    assert!(
        database
            .create_named_project_if_active(
                "Stale",
                &[folders.root("stale-create")],
                Some(&project_a.id),
            )
            .is_err()
    );
    assert_eq!(database.project_count().unwrap(), before_projects);
    assert!(
        database
            .add_project_root(&project_a.id, &folders.root("stale-add"))
            .is_err()
    );
    assert_eq!(
        database.project_summary(&project_a.id).unwrap().roots.len(),
        1
    );
    assert!(
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_a.id.clone(),
                current_layout: layout(&snapshot_a),
            })
            .is_err()
    );
    assert_eq!(
        database.active_project_id().unwrap().as_deref(),
        Some(project_b.id.as_str())
    );
}

#[test]
#[ignore = "manual release-mode project-transition performance evidence"]
fn benchmarks_twenty_project_switches_with_authoritative_projection() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project_a = database
        .create_project_from_root(&folders.root("switch-a"))
        .unwrap();
    let project_b = database
        .create_project_from_root(&folders.root("switch-b"))
        .unwrap();
    let mut current = database.bootstrap_snapshot().unwrap();
    let mut samples_micros = Vec::with_capacity(20);

    for index in 0..20 {
        let target = if index % 2 == 0 {
            &project_a.id
        } else {
            &project_b.id
        };
        let started = Instant::now();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: target.clone(),
                current_layout: layout(&current),
            })
            .unwrap();
        current = database.bootstrap_snapshot().unwrap();
        samples_micros.push(started.elapsed().as_micros() as u64);
        assert_eq!(current.project.as_ref().unwrap().id, *target);
    }

    samples_micros.sort_unstable();
    println!(
        "ADE0005_PROJECT_SWITCH_MICROS samples=20 p50={} p95={} values={samples_micros:?}",
        samples_micros[9], samples_micros[18]
    );
}

#[test]
fn ensure_local_project_creates_a_zero_root_chat_home() {
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let first = database.ensure_local_project().unwrap();
    assert_eq!(first.name, "Local");
    assert!(first.roots.is_empty());
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.project.as_ref().unwrap().id, first.id);
    assert!(snapshot.tabs.iter().any(|tab| tab.kind == "chat"));
    let again = database.ensure_local_project().unwrap();
    assert_eq!(again.id, first.id);
}

fn snapshot_layout(snapshot: &sage_domain::project::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn project_list_includes_bounded_chats_for_every_project() {
    let base = std::env::temp_dir().join(format!("sage-rail-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let cross_chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    for _ in 0..8 {
        database
            .create_chat(&snapshot_layout(&database.bootstrap_snapshot().unwrap()))
            .unwrap();
    }
    let latest: String = database.connection().query_row(
        "SELECT id FROM workbench_tabs WHERE project_id=?1 AND kind='chat' ORDER BY position DESC LIMIT 1",
        [&sage.id], |row| row.get(0),
    ).unwrap();
    database
        .connection()
        .execute(
            "UPDATE chats SET updated_at=?3 WHERE project_id=?1 AND id=?2",
            rusqlite::params![sage.id, latest, sage_core::time::now_millis() + 10_000],
        )
        .unwrap();
    let projects = database.project_list().unwrap();
    let cross_item = projects.iter().find(|item| item.id == cross.id).unwrap();
    let sage_item = projects.iter().find(|item| item.id == sage.id).unwrap();
    assert!(
        cross_item
            .rail_chats
            .iter()
            .any(|chat| chat.id == cross_chat)
    );
    assert!(!cross_item.rail_chats_truncated);
    // Nine chats fit the eight newest plus the eight first by position.
    assert_eq!(sage_item.rail_chats.len(), 9);
    assert_eq!(sage_item.rail_chats[0].id, latest);
    assert!(!sage_item.rail_chats_truncated);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn inactive_project_lists_its_first_chats_by_update_and_by_manual_order() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database
        .create_project_from_root(&folders.root("many"))
        .unwrap();
    for _ in 0..19 {
        database
            .create_chat(&snapshot_layout(&database.bootstrap_snapshot().unwrap()))
            .unwrap();
    }
    // The first chat by position is the least recently updated.
    let first: String = database
        .connection()
        .query_row(
            "SELECT id FROM workbench_tabs WHERE project_id=?1 AND kind='chat' ORDER BY position LIMIT 1",
            [&project.id],
            |row| row.get(0),
        )
        .unwrap();
    database
        .connection()
        .execute("UPDATE chats SET updated_at=1 WHERE id=?1", [&first])
        .unwrap();
    database
        .create_project_from_root(&folders.root("active"))
        .unwrap();
    let projects = database.project_list().unwrap();
    let item = projects.iter().find(|item| item.id == project.id).unwrap();
    assert!(item.rail_chats.len() <= 16);
    assert!(item.rail_chats.iter().any(|chat| chat.id == first));
    let mut positions: Vec<i64> = item.rail_chats.iter().map(|chat| chat.position).collect();
    positions.sort_unstable();
    let mut all: Vec<i64> = database
        .connection()
        .prepare("SELECT position FROM workbench_tabs WHERE project_id=?1 AND kind='chat' ORDER BY position LIMIT 8")
        .unwrap()
        .query_map([&project.id], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    all.sort_unstable();
    assert_eq!(&positions[..8], &all[..]);
    assert!(item.rail_chats_truncated);
}

#[test]
fn inactive_project_keeps_an_older_pin_ahead_of_eight_newer_chats() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database
        .create_project_from_root(&folders.root("pinned"))
        .unwrap();
    let pinned = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    for _ in 0..8 {
        database
            .create_chat(&snapshot_layout(&database.bootstrap_snapshot().unwrap()))
            .unwrap();
    }
    database
        .connection()
        .execute("UPDATE workbench_tabs SET pinned=1 WHERE id=?1", [&pinned])
        .unwrap();
    database
        .connection()
        .execute("UPDATE chats SET updated_at=1 WHERE id=?1", [&pinned])
        .unwrap();
    database
        .create_project_from_root(&folders.root("active"))
        .unwrap();
    let projects = database.project_list().unwrap();
    let item = projects.iter().find(|item| item.id == project.id).unwrap();
    assert_eq!(item.rail_chats.len(), 9);
    assert_eq!(item.rail_chats[0].id, pinned);
    assert!(item.rail_chats[0].pinned);
    assert!(!item.rail_chats_truncated);
}

#[test]
fn named_project_keeps_its_name_and_source_folders_in_order() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let active = database
        .create_project_from_root(&folders.root("active"))
        .unwrap();
    let project = database
        .create_named_project_if_active(
            "  Client work  ",
            &[folders.root("web"), folders.root("api")],
            Some(&active.id),
        )
        .unwrap();
    assert_eq!(project.name, "Client work");
    let names: Vec<_> = project
        .roots
        .iter()
        .map(|root| root.name.as_str())
        .collect();
    assert_eq!(names, ["web", "api"]);
    assert_eq!(
        database.bootstrap_snapshot().unwrap().project.unwrap().id,
        project.id
    );
}

#[test]
fn named_project_may_start_without_source_folders() {
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database
        .create_named_project_if_active("Notes", &[], None)
        .unwrap();
    assert_eq!(project.name, "Notes");
    assert!(project.roots.is_empty());
}

#[test]
fn named_project_rejects_bad_names_and_duplicate_folders_without_mutation() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let web = folders.root("dup-web");
    for (name, roots) in [
        ("   ", vec![]),
        ("tab\tname", vec![]),
        (&*"x".repeat(MAX_PROJECT_NAME_CHARS + 1), vec![]),
        ("Dup", vec![web.clone(), web.join(".")]),
    ] {
        assert!(
            database
                .create_named_project_if_active(name, &roots, None)
                .is_err(),
            "{name:?} should be rejected"
        );
    }
    assert_eq!(database.project_count().unwrap(), 0);
    assert!(
        database
            .create_named_project_if_active(&"x".repeat(MAX_PROJECT_NAME_CHARS), &[], None)
            .is_ok()
    );
}

#[test]
fn rail_chat_finish_time_ignores_runtime_bookkeeping() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database
        .create_project_from_root(&folders.root("finished"))
        .unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    {
        let connection = database.connection();
        connection.execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
             capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at)
             VALUES('finished-session',?1,?2,0,'codex-app-server',?3,'{}','ready','hot',0,0,0,1,1)",
            rusqlite::params![project.id, chat, "a".repeat(64)],
        ).unwrap();
        connection.execute(
            "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
             VALUES(?1,?2,'finished-session',1,'running',1)",
            rusqlite::params![project.id, chat],
        ).unwrap();
        connection
            .execute(
                "INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,
                 item_bytes,fact_count,fact_bytes,payload_bytes,revision)
                 VALUES(?1,?2,'finished-session',2,0,0,0,0,0,0)",
                rusqlite::params![project.id, chat],
            )
            .unwrap();
        for (ordinal, terminal_at) in [(0, 2_000), (1, 5_000)] {
            connection.execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                 state,delivery_state,revision,created_at,updated_at,terminal_at)
                 VALUES(?3,?1,?2,'finished-session',1,?4,?3,'completed','correlated',0,1,?5,?5)",
                rusqlite::params![project.id, chat, format!("turn-{ordinal}"), ordinal, terminal_at],
            ).unwrap();
        }
        // Parking the session later moves the chat's update time only.
        connection
            .execute(
                "UPDATE chats SET updated_at=9000 WHERE project_id=?1 AND id=?2",
                rusqlite::params![project.id, chat],
            )
            .unwrap();
    }
    let restored = database.bootstrap_snapshot().unwrap();
    let tab = restored.tabs.iter().find(|tab| tab.id == chat).unwrap();
    assert_eq!((tab.updated_at, tab.finished_at), (Some(9000), Some(5_000)));
    database
        .create_project_from_root(&folders.root("other"))
        .unwrap();
    let projects = database.project_list().unwrap();
    let rail = projects.iter().find(|item| item.id == project.id).unwrap();
    let rail_chat = rail.rail_chats.iter().find(|row| row.id == chat).unwrap();
    assert_eq!(
        (rail_chat.updated_at, rail_chat.finished_at),
        (Some(9000), Some(5_000))
    );
}

#[test]
fn project_list_keeps_its_order_when_another_project_opens() {
    let folders = Folders::new();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let names = ["first", "second", "third"];
    for (created_at, name) in (1_i64..).zip(names) {
        let project = database
            .create_project_from_root(&folders.root(name))
            .unwrap();
        database
            .connection()
            .execute(
                "UPDATE projects SET created_at=?2 WHERE id=?1",
                rusqlite::params![project.id, created_at],
            )
            .unwrap();
    }
    let listed_names = |database: &Database| -> Vec<String> {
        database
            .project_list()
            .unwrap()
            .into_iter()
            .map(|item| item.name)
            .collect()
    };
    assert_eq!(listed_names(&database), ["third", "second", "first"]);

    let first = database
        .project_list()
        .unwrap()
        .into_iter()
        .find(|item| item.name == "first")
        .unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: first.id,
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
        })
        .unwrap();

    assert_eq!(listed_names(&database), ["third", "second", "first"]);
}
