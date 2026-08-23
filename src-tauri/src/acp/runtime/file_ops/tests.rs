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

#[test]
fn apply_deleted_file_uses_workspace_trash() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("deleted.txt"), "delete\n").unwrap();
    let intent = fixture.intent(AgentFileOperation::Deleted, fixture.resource("deleted.txt"));

    apply_agent_file_intent(fixture.database(), &fixture.workspace, intent).unwrap();

    assert!(!fixture.root.join("deleted.txt").exists());
}
