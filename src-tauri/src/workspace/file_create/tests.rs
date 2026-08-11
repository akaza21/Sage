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
    let resource = workspace
        .create_empty_file(&parent("src"), "new.rs")
        .unwrap();
    assert_eq!(resource.relative_path, "src/new.rs");
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
fn enforces_portable_name_and_utf8_path_bounds() {
    let (root, _workspace) = fixture();
    assert!(validate_new_file_basename(&"a".repeat(255)).is_ok());
    assert!(validate_new_file_basename(&"a".repeat(256)).is_err());
    assert!(validate_new_file_basename(&format!("{}a", "\u{e9}".repeat(127))).is_ok());
    assert!(validate_new_file_basename(&"\u{e9}".repeat(128)).is_err());
    for invalid in [
        "..", "a/b", "a\\b", "CON", "nul.txt", "COM1.js", "lpt9", "bad.", "bad ", "a\0b",
        "a\u{7f}b", "a*b",
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
