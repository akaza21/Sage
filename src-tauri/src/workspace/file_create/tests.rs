use std::fs;

use uuid::Uuid;

use super::*;

fn fixture() -> (std::path::PathBuf, Workspace) {
    let root = std::env::temp_dir().join(format!("sage-file-create-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).unwrap();
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    (root, workspace)
}

fn parent(path: &str) -> ResourceRef {
    ResourceRef {
        root_id: "root-a".into(),
        relative_path: path.into(),
    }
}

#[test]
fn creates_one_empty_file_without_overwriting() {
    let (root, workspace) = fixture();
    let result = workspace
        .create_empty_file(&parent("src"), "new.rs")
        .unwrap();
    assert_eq!(result.resource.relative_path, "src/new.rs");
    assert!(result.resource_confirmed);
    assert!(result.warning.is_none());
    assert_eq!(fs::read(root.join("src/new.rs")).unwrap(), b"");
    fs::write(root.join("src/existing.rs"), "preserve\n").unwrap();
    assert!(
        workspace
            .create_empty_file(&parent("src"), "existing.rs")
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(root.join("src/existing.rs")).unwrap(),
        "preserve\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sync_and_post_create_listing_failures_return_committed_reconciliation() {
    let (root, workspace) = fixture();
    let result = workspace
        .create_empty_file_with(
            &parent("src"),
            "warning.rs",
            |_| Ok(()),
            |_| Err(std::io::Error::other("injected sync failure")),
            |_, _| Err(AppError::InvalidRequest("injected listing failure".into())),
        )
        .unwrap();
    assert_eq!(result.resource.relative_path, "src/warning.rs");
    assert!(result.resource_confirmed);
    assert!(result.warning.as_deref().unwrap().contains("durability"));
    assert!(
        result
            .warning
            .as_deref()
            .unwrap()
            .contains("Explorer refresh")
    );
    assert!(result.parent_listing.entries.iter().any(|entry| {
        entry.resource == result.resource && entry.kind == "file" && entry.byte_len == Some(0)
    }));
    assert!(root.join("src/warning.rs").is_file());

    let collision = workspace
        .create_empty_file(&parent("src"), "warning.rs")
        .unwrap_err();
    assert!(collision.to_string().contains("exists"));
    assert!(root.join("src/warning.rs").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn renamed_unix_parent_never_invents_the_old_resource_path() {
    let (root, workspace) = fixture();
    let parent_path = root.join("src");
    let moved_path = root.join("moved");
    let result = workspace
        .create_empty_file_with(
            &parent("src"),
            "moved.rs",
            |_| {
                fs::rename(&parent_path, &moved_path)?;
                Ok(())
            },
            |file| file.sync_all(),
            |_, _| Err(AppError::InvalidRequest("injected listing failure".into())),
        )
        .unwrap();
    assert!(!result.resource_confirmed);
    assert!(
        !result
            .parent_listing
            .entries
            .iter()
            .any(|entry| entry.resource == result.resource)
    );
    assert!(moved_path.join("moved.rs").is_file());
    assert!(result.warning.as_deref().unwrap().contains("unknown"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn enforces_portable_name_and_utf8_path_bounds() {
    let (root, _workspace) = fixture();
    assert!(validate_new_file_basename(&"a".repeat(255)).is_ok());
    assert!(validate_new_file_basename(&"a".repeat(256)).is_err());
    assert!(validate_new_file_basename(&format!("{}a", "\u{e9}".repeat(127))).is_ok());
    assert!(validate_new_file_basename(&"\u{e9}".repeat(128)).is_err());
    for invalid in [
        "..", "a/b", "a\\b", "CON", "nul.txt", "COM1.js", "lpt9", "COM¹", "com².rs", "CoM³",
        "LPT¹", "lpt².md", "LpT³", "bad.", "bad ", "a\0b", "a\u{7f}b", "a*b",
    ] {
        assert!(
            validate_new_file_basename(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    assert!(validate_new_file_path(&"a".repeat(16_382), "x").is_ok());
    assert!(validate_new_file_path(&"a".repeat(16_383), "x").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn existing_directory_writer_blocks_creation() {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ADD_FILE, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let (root, workspace) = fixture();
    let writer = OpenOptions::new()
        .access_mode(FILE_ADD_FILE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(root.join("src"))
        .unwrap();
    assert!(
        workspace
            .create_empty_file(&parent("src"), "blocked.rs")
            .is_err()
    );
    assert!(!root.join("src/blocked.rs").exists());
    drop(writer);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn held_parent_authority_blocks_replacement_before_create() {
    let (root, workspace) = fixture();
    let parent_path = root.join("src");
    let moved_path = root.join("moved");
    let result = workspace
        .create_empty_file_with(
            &parent("src"),
            "anchored.rs",
            |_| {
                assert!(fs::rename(&parent_path, &moved_path).is_err());
                Ok(())
            },
            |file| file.sync_all(),
            |workspace, parent| {
                assert!(fs::rename(&parent_path, &moved_path).is_err());
                workspace.list_directory(parent)
            },
        )
        .unwrap();
    assert_eq!(result.resource.relative_path, "src/anchored.rs");
    assert!(root.join("src/anchored.rs").is_file());
    assert!(!moved_path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn apple_openat_binding_keeps_the_variadic_posix_abi() {
    super::platform::assert_apple_openat_variadic_binding();
}

#[test]
fn rejects_parent_aliases_before_creation() {
    let (root, workspace) = fixture();
    for alias in ["../src", "src/..", "src\\nested", "src//nested"] {
        assert!(
            workspace
                .create_empty_file(&parent(alias), "blocked.rs")
                .is_err(),
            "accepted parent alias {alias:?}"
        );
    }
    assert!(!root.join("src/blocked.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_workspace_root_identity_drift_without_creating() {
    let (root, workspace) = fixture();
    let original = root.with_extension("original");
    fs::rename(&root, &original).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let error = workspace
        .create_empty_file(&parent("src"), "escaped.rs")
        .unwrap_err();
    assert!(error.to_string().contains("identity changed"));
    assert!(!root.join("src/escaped.rs").exists());
    assert!(!original.join("src/escaped.rs").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(original).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_symlink_parent_traversal() {
    use std::os::unix::fs::symlink;

    let (root, workspace) = fixture();
    let outside = root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    assert!(
        workspace
            .create_empty_file(&parent("link"), "escaped.rs")
            .is_err()
    );
    assert!(!outside.join("escaped.rs").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn rejects_reparse_parent_traversal_when_symlink_creation_is_available() {
    use std::os::windows::fs::symlink_dir;

    let (root, workspace) = fixture();
    let outside = root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    if symlink_dir(&outside, root.join("link")).is_ok() {
        assert!(
            workspace
                .create_empty_file(&parent("link"), "escaped.rs")
                .is_err()
        );
        assert!(!outside.join("escaped.rs").exists());
    }
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
