use std::{fs, path::PathBuf};

use uuid::Uuid;

use super::*;

struct Fixture {
    root: PathBuf,
    database: Option<Database>,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-agent-file-ops-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let workspace = Workspace::open(&project.id, &project.roots[0].id, &root).unwrap();
        Self {
            root,
            database: Some(database),
            workspace,
        }
    }

    fn resource(&self, path: &str) -> ResourceRef {
        self.workspace.resource(path).unwrap()
    }

    fn database(&self) -> &Database {
        self.database.as_ref().unwrap()
    }

    fn intent(&self, operation: AgentFileOperation, resource: ResourceRef) -> AgentFileIntent {
        AgentFileIntent {
            operation,
            resource,
            source: None,
            basename: None,
            content: None,
            project_id: self.workspace.project_id().into(),
            project_generation: 1,
            expected_transition_epoch: 1,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.database.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn apply_created_file_uses_workspace_create_not_raw_fs() {
    let fixture = Fixture::new();
    let resource = ResourceRef {
        root_id: fixture.workspace.root_id().into(),
        relative_path: "created.txt".into(),
    };
    let mut intent = fixture.intent(AgentFileOperation::Created, resource);
    intent.basename = Some("created.txt".into());

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert_eq!(fs::read(fixture.root.join("created.txt")).unwrap(), b"");
}

#[test]
fn apply_created_file_accepts_matching_recomposed_target() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("notes")).unwrap();
    let resource = ResourceRef {
        root_id: fixture.workspace.root_id().into(),
        relative_path: "notes/agent.txt".into(),
    };
    let mut intent = fixture.intent(AgentFileOperation::Created, resource);
    intent.basename = Some("agent.txt".into());

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert_eq!(fs::read(fixture.root.join("notes/agent.txt")).unwrap(), b"");
}

#[test]
fn apply_created_file_rejects_mismatched_basename() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("notes")).unwrap();
    let resource = ResourceRef {
        root_id: fixture.workspace.root_id().into(),
        relative_path: "notes/agent.txt".into(),
    };
    let mut intent = fixture.intent(AgentFileOperation::Created, resource);
    intent.basename = Some("other.txt".into());

    let error = apply_agent_file_intent(fixture.database(), &fixture.workspace, intent)
        .expect_err("mismatched basename must be rejected");

    assert!(
        error
            .to_string()
            .contains("does not match its typed resource")
    );
    assert!(!fixture.root.join("notes/agent.txt").exists());
    assert!(!fixture.root.join("notes/other.txt").exists());
}

#[test]
fn apply_updated_file_uses_editor_snapshot_and_save() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("updated.txt"), "before\n").unwrap();
    let mut intent = fixture.intent(AgentFileOperation::Updated, fixture.resource("updated.txt"));
    intent.content = Some("after\n".into());

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert_eq!(
        fs::read_to_string(fixture.root.join("updated.txt")).unwrap(),
        "after\n"
    );
}

#[test]
fn apply_updated_file_allows_empty_content_to_truncate() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("updated.txt"), "before\n").unwrap();
    let mut intent = fixture.intent(AgentFileOperation::Updated, fixture.resource("updated.txt"));
    intent.content = Some(String::new());

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert_eq!(
        fs::read_to_string(fixture.root.join("updated.txt")).unwrap(),
        ""
    );
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn apply_renamed_file_uses_workspace_rename() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("before.txt"), "before\n").unwrap();
    let source = fixture.resource("before.txt");
    let mut intent = fixture.intent(
        AgentFileOperation::Renamed,
        ResourceRef {
            root_id: fixture.workspace.root_id().into(),
            relative_path: "after.txt".into(),
        },
    );
    intent.source = Some(source);
    intent.basename = Some("after.txt".into());

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert!(!fixture.root.join("before.txt").exists());
    assert_eq!(
        fs::read_to_string(fixture.root.join("after.txt")).unwrap(),
        "before\n"
    );
}

#[cfg(windows)]
#[test]
fn apply_deleted_file_uses_workspace_trash() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("deleted.txt"), "delete\n").unwrap();
    let intent = fixture.intent(AgentFileOperation::Deleted, fixture.resource("deleted.txt"));

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert!(!fixture.root.join("deleted.txt").exists());
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn apply_renamed_file_fails_closed_without_mutating_the_source() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("before.txt"), "before\n").unwrap();
    let source = fixture.resource("before.txt");
    let mut intent = fixture.intent(
        AgentFileOperation::Renamed,
        ResourceRef {
            root_id: fixture.workspace.root_id().into(),
            relative_path: "after.txt".into(),
        },
    );
    intent.source = Some(source);
    intent.basename = Some("after.txt".into());

    let error = apply_agent_file_intent(fixture.database(), &fixture.workspace, intent)
        .expect_err("non-Windows rename must fail closed");

    assert!(
        matches!(error, crate::error::AppError::Unsupported(message) if message == "safe closed-file rename currently requires Windows held-source authority")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("before.txt")).unwrap(),
        "before\n"
    );
    assert!(!fixture.root.join("after.txt").exists());
}

#[cfg(not(windows))]
#[test]
fn apply_deleted_file_fails_closed_without_mutating_the_source() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("deleted.txt"), "delete\n").unwrap();
    let intent = fixture.intent(AgentFileOperation::Deleted, fixture.resource("deleted.txt"));

    let error = apply_agent_file_intent(fixture.database(), &fixture.workspace, intent)
        .expect_err("non-Windows deletion must fail closed");

    assert!(
        matches!(error, crate::error::AppError::Unsupported(message) if message == "recoverable resource trash currently requires the Windows Recycle Bin adapter")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("deleted.txt")).unwrap(),
        "delete\n"
    );
}
