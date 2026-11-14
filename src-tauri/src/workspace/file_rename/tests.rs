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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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

#[cfg(windows)]
#[test]
fn windows_empty_folder_rename_is_explicitly_refused_without_mutation() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("src/empty")).unwrap();
    let source = fixture.workspace.resource("src/empty").unwrap();

    let result = fixture.workspace.rename_empty_folder(&source, "renamed");

    assert!(
        matches!(result, Err(sage_core::error::AppError::Unsupported(message)) if message.contains("identity-scoped directory-mutation guard"))
    );
    assert!(fixture.root.join("src/empty").is_dir());
    assert!(!fixture.root.join("src/renamed").exists());
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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn source_replacement_after_held_preflight_fails_before_mutation() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();
    let source_path = fixture.root.join("src/old.rs");

    let error = fixture
        .workspace
        .rename_file_with_hooks(
            &source,
            "new.rs",
            || {
                fs::remove_file(&source_path)?;
                fs::write(&source_path, "replacement\n")?;
                Ok(())
            },
            |parent| fixture.workspace.list_directory(parent),
        )
        .unwrap_err();

    assert!(error.to_string().contains("identity changed"));
    assert_eq!(fs::read_to_string(source_path).unwrap(), "replacement\n");
    assert!(!fixture.root.join("src/new.rs").exists());
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn post_commit_listing_failure_is_unconfirmed_instead_of_escaping() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    let result = fixture
        .workspace
        .rename_file_with_hooks(
            &source,
            "new.rs",
            || Ok(()),
            |_| {
                Err(sage_core::error::AppError::InvalidRequest(
                    "injected listing failure".into(),
                ))
            },
        )
        .unwrap();

    assert!(!result.resource_confirmed);
    assert!(result.parent_listing.is_none());
    assert!(result.warning.as_deref().unwrap().contains("refreshing"));
    assert!(!fixture.root.join("src/old.rs").exists());
    assert!(fixture.root.join("src/new.rs").is_file());
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn parent_swap_during_reconciliation_never_confirms_the_replacement_path() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();
    let retained = fixture.root.join("retained-src");
    let visible = fixture.root.join("src");

    let result = fixture
        .workspace
        .rename_file_with_hooks(
            &source,
            "new.rs",
            || Ok(()),
            |parent| {
                fs::rename(&visible, &retained)?;
                fs::create_dir(&visible)?;
                fs::write(visible.join("new.rs"), "replacement\n")?;
                fixture.workspace.list_directory(parent)
            },
        )
        .unwrap();

    assert!(!result.resource_confirmed);
    assert!(result.parent_listing.is_none());
    if retained.exists() {
        assert_eq!(
            fs::read_to_string(retained.join("new.rs")).unwrap(),
            "preserve\n"
        );
        fs::remove_dir_all(retained).unwrap();
    } else {
        assert_eq!(
            fs::read_to_string(visible.join("new.rs")).unwrap(),
            "preserve\n"
        );
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn root_swap_during_reconciliation_never_confirms_the_replacement_path() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();
    let retained = fixture.root.with_extension("retained-root");
    let visible = fixture.root.clone();

    let result = fixture
        .workspace
        .rename_file_with_hooks(
            &source,
            "new.rs",
            || Ok(()),
            |parent| {
                fs::rename(&visible, &retained)?;
                fs::create_dir_all(visible.join("src"))?;
                fs::write(visible.join("src/new.rs"), "replacement\n")?;
                fixture.workspace.list_directory(parent)
            },
        )
        .unwrap();

    assert!(!result.resource_confirmed);
    assert!(result.parent_listing.is_none());
    if retained.exists() {
        assert_eq!(
            fs::read_to_string(retained.join("src/new.rs")).unwrap(),
            "preserve\n"
        );
        fs::remove_dir_all(retained).unwrap();
    } else {
        assert_eq!(
            fs::read_to_string(visible.join("src/new.rs")).unwrap(),
            "preserve\n"
        );
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_platform_fails_without_mutating_the_source() {
    let fixture = Fixture::new();
    let source = fixture.workspace.resource("src/old.rs").unwrap();

    let error = fixture
        .workspace
        .rename_file(&source, "new.rs")
        .unwrap_err();

    assert!(matches!(error, sage_core::error::AppError::Unsupported(_)));
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/old.rs")).unwrap(),
        "preserve\n"
    );
    assert!(!fixture.root.join("src/new.rs").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_linked_parent_directory_fails_before_mutation() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(fixture.root.join("src"), fixture.root.join("linked")).unwrap();
    let source = crate::domain::ResourceRef {
        root_id: "root".into(),
        relative_path: "linked/old.rs".into(),
    };

    assert!(
        fixture
            .workspace
            .rename_file(&source, "renamed.rs")
            .is_err()
    );
    assert!(fixture.root.join("src/old.rs").is_file());
    assert!(!fixture.root.join("src/renamed.rs").exists());
}
