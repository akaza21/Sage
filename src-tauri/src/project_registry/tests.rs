use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use uuid::Uuid;

use super::*;
use crate::{
    domain::{ProjectLayoutRequest, ProjectSwitchRequest},
    editor,
    persistence::Database,
};

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

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
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
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&outer).unwrap();

    assert!(database.add_project_root(&project.id, &outer).is_err());
    let project = database.add_project_root(&project.id, &nested).unwrap();
    assert_eq!(project.roots.len(), 2);
    assert_ne!(project.roots[0].id, project.roots[1].id);
}

#[test]
fn root_limit_is_rejected_without_losing_existing_roots() {
    let folders = Folders::new();
    let database = Database::open(Path::new(":memory:")).unwrap();
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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

#[test]
fn missing_root_remains_typed_and_does_not_hide_available_roots() {
    let folders = Folders::new();
    let first = folders.root("available");
    let missing = folders.root("missing");
    let database = Database::open(Path::new(":memory:")).unwrap();
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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
            crate::domain::BufferSnapshotRequest {
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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
        crate::domain::BufferSnapshotRequest {
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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
            .create_project_from_root_if_active(&folders.root("stale-create"), Some(&project_a.id),)
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
    let database = Database::open(Path::new(":memory:")).unwrap();
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
