use super::*;
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("sage-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        // Attachments refuse linked ancestors, and macOS reaches its temporary
        // directory through the /var link.
        Self(p.canonicalize().unwrap())
    }
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p.to_str().unwrap().into()
    }
    fn read(&self, name: &str) -> Resources {
        read::read_paths(&[self.0.join(name).to_str().unwrap().into()]).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn snapshots_are_readable_immutable_resources_and_capability_gated() {
    let f = Fixture::new();
    let path = f.file("text.txt", b"original");
    let r = read::read_paths(std::slice::from_ref(&path)).unwrap();
    fs::write(path, b"changed").unwrap();
    assert!(
        r.wire("hello", false)
            .unwrap_err()
            .contains("embedded-resource")
    );
    let blocks = r.wire("hello", true).unwrap();
    assert_eq!(blocks[0]["text"], "hello");
    assert_eq!(blocks[1]["resource"]["text"], "original");
    assert!(
        blocks[1]["resource"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("sage-attachment:")
    );
    let binary = f.file("binary", &[255, 0, 128]);
    assert_eq!(
        read::read_paths(&[binary])
            .unwrap()
            .wire("hello", true)
            .unwrap()[1]["resource"]["blob"],
        "/wCA"
    );
}
#[test]
fn bounded_expansion_respects_ignores_for_direct_and_directory_picks() {
    let f = Fixture::new();
    f.file(".gitignore", b"*.log\nignored/\n");
    f.file(".ignore", b"*.tmp\n");
    f.file("keep.txt", b"keep");
    f.file("bad.log", b"no");
    f.file("bad.tmp", b"no");
    f.file("ignored/nested", b"no");
    f.file("node_modules/no", b"no");
    f.file(".git/no", b"no");
    let r = f.read("");
    let names: Vec<_> = r.0.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"keep.txt"));
    assert!(!names.contains(&"bad.log"));
    assert!(!names.contains(&"no"));
    assert!(read::read_paths(&[f.0.join("bad.log").to_str().unwrap().into()]).is_err());
    assert!(read::read_paths(&[f.0.join("ignored/nested").to_str().unwrap().into()]).is_err());
}
#[test]
fn worktree_exclude_rules_resolved_through_commondir_fail_closed() {
    let f = Fixture::new();
    let mut oversized = String::from("secret.txt\n");
    oversized.push_str(&"#".repeat(256 * 1024));
    f.file("main/.git/info/exclude", oversized.as_bytes());
    f.file("main/.git/worktrees/wt/commondir", b"../..\n");
    let git_dir = f.0.join("main/.git/worktrees/wt");
    f.file(
        "wt/.git",
        format!("gitdir: {}\n", git_dir.to_str().unwrap()).as_bytes(),
    );
    f.file("wt/secret.txt", b"credential");
    f.file("wt/keep.txt", b"keep");
    let secret = f.0.join("wt/secret.txt").to_str().unwrap().to_owned();
    let folder = f.0.join("wt").to_str().unwrap().to_owned();
    // The matcher cannot apply the oversized shared exclude file, so neither
    // a direct pick nor a folder pick may admit what it could exclude.
    let refused = "Attachment ignore rules could not be applied in full.";
    assert_eq!(
        read::read_paths(std::slice::from_ref(&secret)),
        Err(refused)
    );
    assert_eq!(
        read::read_paths(std::slice::from_ref(&folder)),
        Err(refused)
    );
    f.file("main/.git/info/exclude", b"secret.txt\n");
    assert!(read::read_paths(&[secret]).is_err());
    let names: Vec<_> = read::read_paths(&[folder])
        .unwrap()
        .0
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(names, ["keep.txt"]);
}
#[test]
fn credential_files_are_refused_even_without_ignore_rules() {
    let f = Fixture::new();
    f.file("home/.aws/credentials", b"aws_secret_access_key=x");
    f.file("home/.ssh/config", b"Host *");
    f.file("home/.env.local", b"TOKEN=x");
    f.file("home/deploy.pem", b"-----BEGIN PRIVATE KEY-----");
    f.file("home/notes.txt", b"notes");
    let refused = "Credential files and folders cannot be attached.";
    for picked in [
        "home/.aws/credentials",
        "home/.aws",
        "home/.ssh/config",
        "home/.env.local",
        "home/deploy.pem",
    ] {
        let path = f.0.join(picked).to_str().unwrap().to_owned();
        assert_eq!(read::read_paths(&[path]), Err(refused), "{picked}");
    }
    let names: Vec<_> = f.read("home").0.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["notes.txt"]);
}
#[test]
fn failure_and_boundaries_do_not_admit_partial_content() {
    let f = Fixture::new();
    let max = f.file("max", &vec![0; FILE_BYTES]);
    assert_eq!(f.read("max").0[0].bytes.len(), FILE_BYTES);
    let over = f.file("over", &vec![0; FILE_BYTES + 1]);
    assert!(read::read_paths(&[over]).is_err());
    assert!(read::read_paths(&[max.clone(), max]).is_err());
    let paths: Vec<_> = (0..5)
        .map(|i| f.file(&format!("f{i}"), &vec![0; TOTAL_BYTES / 4]))
        .collect();
    assert!(read::read_paths(&paths[..4]).is_ok());
    assert!(read::read_paths(&paths).is_err());
    for i in 0..17 {
        f.file(&format!("many/{i}"), b"x");
    }
    assert!(read::read_paths(&[f.0.join("many").to_str().unwrap().into()]).is_err());
    assert!(read::read_paths(&[f.0.join("missing").to_str().unwrap().into()]).is_err());
    assert!(read::read_paths(&["relative".into()]).is_err());
}
#[test]
fn staging_conflicts_owner_isolation_and_lifecycle_release() {
    let f = Fixture::new();
    f.file("one", b"one");
    f.file("two", b"two");
    let mut stages = FileStages::default();
    let old = stages.stage("p", "c", 0, None, f.read("one")).unwrap();
    let snapshot = stages.get("p", "c");
    let weak = Arc::downgrade(&snapshot.0);
    assert!(stages.stage("p", "c", 0, None, f.read("two")).is_err());
    assert!(stages.get("p", "foreign").0.is_empty());
    assert!(stages.remove("p", "c", "stale", &old.files[0].id).is_err());
    assert!(stages.remove("p", "c", &old.revision, "foreign").is_err());
    let next = stages
        .stage("p", "c", 0, Some(&old.revision), f.read("two"))
        .unwrap();
    stages.consume("p", "c", &snapshot); // Late acknowledgement cannot consume a newer selection.
    assert_eq!(stages.list("p", "c").unwrap().revision, next.revision);
    drop(snapshot);
    assert!(weak.upgrade().is_none());
    assert!(
        stages
            .remove("p", "c", &next.revision, &next.files[0].id)
            .unwrap()
            .is_none()
    );
    stages.stage("p", "c", 0, None, f.read("one")).unwrap();
    let sent = stages.get("p", "c");
    stages.consume("p", "c", &sent);
    assert!(stages.list("p", "c").is_none());
    drop(sent);
    stages.stage("p", "c", 0, None, f.read("one")).unwrap();
    stages.clear_chat("p", "c");
    assert!(stages.batches.is_empty());
    stages.stage("p", "c", 0, None, f.read("one")).unwrap();
    stages.clear();
    assert!(stages.batches.is_empty());
}
#[test]
fn owner_and_memory_caps_and_settled_memory() {
    let f = Fixture::new();
    f.file("one", b"x");
    let mut stages = FileStages::default();
    for i in 0..OWNER_COUNT {
        stages
            .stage("p", &i.to_string(), 0, None, f.read("one"))
            .unwrap();
    }
    assert!(stages.stage("p", "over", 0, None, f.read("one")).is_err());
    stages.clear();
    let paths: Vec<_> = (0..4)
        .map(|i| f.file(&format!("large{i}"), &vec![1; TOTAL_BYTES / 4]))
        .collect();
    let large = read::read_paths(&paths).unwrap();
    let peak = large.retained_bytes();
    let weak = Arc::downgrade(&large.0);
    stages.stage("p", "c", 0, None, large).unwrap();
    let wire = stages.get("p", "c").wire("memory probe", true).unwrap();
    drop(wire);
    stages.clear();
    assert!(weak.upgrade().is_none());
    for i in 0..(STORE_BYTES / peak) {
        stages
            .stage(
                "p",
                &i.to_string(),
                0,
                None,
                read::read_paths(&paths).unwrap(),
            )
            .unwrap();
    }
    assert!(
        stages
            .stage("p", "full", 0, None, read::read_paths(&paths).unwrap())
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn symlinks_and_nonregular_files_are_refused() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let path = f.file("one", b"x");
    symlink(path, f.0.join("link")).unwrap();
    assert!(read::read_paths(&[f.0.join("link").to_str().unwrap().into()]).is_err());
    symlink(&f.0, f.0.join("loop")).unwrap();
    assert!(f.read("").0.iter().all(|r| r.name != "loop"));
    // A device file: a socket under a macOS temporary directory would exceed
    // the platform's socket path limit.
    assert_eq!(
        read::read_paths(&["/dev/null".into()]),
        Err("Only regular files can be attached.")
    );
}

#[cfg(unix)]
#[test]
fn unix_backslashes_in_names_do_not_select_a_different_file() {
    let f = Fixture::new();
    f.file("a\\b", b"selected");
    f.file("a/b", b"different");
    assert_eq!(f.read("a\\b").0[0].bytes, b"selected");
}
