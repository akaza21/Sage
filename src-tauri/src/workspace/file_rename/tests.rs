use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::workspace::Workspace;

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-file-rename-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/old.rs"), "preserve\n").unwrap();
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
fn renames_one_regular_file_without_changing_content() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    let result = fixture.workspace.rename_file(&source, "new.rs").unwrap();

    assert_eq!(result.source, source);
    assert_eq!(result.target.relative_path, "src/new.rs");
    assert!(result.resource_confirmed);
    assert!(!fixture.root.join("src/old.rs").exists());
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/new.rs")).unwrap(),
        "preserve\n"
    );
    assert!(result.parent_listing.unwrap().entries.iter().any(|entry| {
        entry.resource == result.target && entry.kind == "file" && entry.byte_len == Some(9)
    }));
}

#[test]
fn collision_fails_without_overwriting_either_file() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("src/new.rs"), "target\n").unwrap();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    assert!(fixture.workspace.rename_file(&source, "new.rs").is_err());
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/old.rs")).unwrap(),
        "preserve\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/new.rs")).unwrap(),
        "target\n"
    );
}

#[cfg(windows)]
#[test]
fn case_only_rename_commits_the_requested_exact_spelling() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    let result = fixture.workspace.rename_file(&source, "OLD.rs").unwrap();

    assert_eq!(result.target.relative_path, "src/OLD.rs");
    assert!(result.resource_confirmed);
    assert!(
        !result
            .parent_listing
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.resource.relative_path == "src/old.rs")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/OLD.rs")).unwrap(),
        "preserve\n"
    );
}

#[test]
fn directories_and_non_portable_targets_fail_before_mutation() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("src/folder")).unwrap();
    let directory = fixture.workspace.resource("src/folder").unwrap();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    assert!(
        fixture
            .workspace
            .rename_file(&directory, "renamed")
            .is_err()
    );
    assert!(fixture.workspace.rename_file(&source, "../escape").is_err());
    assert!(fixture.root.join("src/folder").is_dir());
    assert!(fixture.root.join("src/old.rs").is_file());
}

#[cfg(any(unix, windows))]
#[test]
fn link_source_fails_before_mutation() {
    let fixture = Fixture::new();
    let link = fixture.root.join("src/link.rs");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.root.join("src/old.rs"), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(fixture.root.join("src/old.rs"), &link).unwrap();
    let source = crate::domain::ResourceRef {
        root_id: "root".into(),
        relative_path: "src/link.rs".into(),
    };

    assert!(
        fixture
            .workspace
            .rename_file(&source, "renamed.rs")
            .is_err()
    );
    assert!(link.exists());
    assert!(fixture.root.join("src/old.rs").is_file());
}
