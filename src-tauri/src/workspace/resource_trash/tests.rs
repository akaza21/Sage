use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::{domain::TrashResourceKind, workspace::Workspace};

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-resource-trash-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src/empty")).unwrap();
        fs::create_dir_all(root.join("src/nonempty")).unwrap();
        fs::create_dir_all(root.join("src/dotgit/.git")).unwrap();
        fs::write(root.join("src/file.rs"), "preserve\n").unwrap();
        fs::write(root.join("src/nonempty/child.rs"), "preserve\n").unwrap();
        fs::write(root.join("src/dotgit/.git/config"), "preserve\n").unwrap();
        let workspace = Workspace::open("project", "root", &root).unwrap();
        Self { root, workspace }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn preflight_admits_one_regular_file_and_one_empty_folder() {
    let fixture = Fixture::new();

    let file = fixture.workspace.resource("src/file.rs").unwrap();
    let folder = fixture.workspace.resource("src/empty").unwrap();
    let file_parent = fixture
        .workspace
        .preflight_resource_trash(&file, TrashResourceKind::File)
        .unwrap();
    let folder_parent = fixture
        .workspace
        .preflight_resource_trash(&folder, TrashResourceKind::EmptyFolder)
        .unwrap();

    assert_eq!(file_parent.relative_path, "src");
    assert_eq!(folder_parent.relative_path, "src");
}

#[test]
fn preflight_rejects_root_kind_mismatch_and_nonempty_folder_without_mutation() {
    let fixture = Fixture::new();
    let root = crate::domain::ResourceRef {
        root_id: "root".into(),
        relative_path: String::new(),
    };
    let file = fixture.workspace.resource("src/file.rs").unwrap();
    let nonempty = fixture.workspace.resource("src/nonempty").unwrap();
    let hidden_nonempty = fixture.workspace.resource("src/dotgit").unwrap();

    assert!(
        fixture
            .workspace
            .preflight_resource_trash(&root, TrashResourceKind::EmptyFolder)
            .is_err()
    );
    assert!(
        fixture
            .workspace
            .preflight_resource_trash(&file, TrashResourceKind::EmptyFolder)
            .is_err()
    );
    assert!(
        fixture
            .workspace
            .preflight_resource_trash(&nonempty, TrashResourceKind::EmptyFolder)
            .is_err()
    );
    assert!(
        fixture
            .workspace
            .preflight_resource_trash(&hidden_nonempty, TrashResourceKind::EmptyFolder)
            .is_err()
    );
    assert!(fixture.root.join("src/file.rs").is_file());
    assert!(fixture.root.join("src/nonempty/child.rs").is_file());
    assert!(fixture.root.join("src/dotgit/.git/config").is_file());
}
