use super::*;
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("sage-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
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
    let old = stages.stage("p", "c", None, f.read("one")).unwrap();
    let snapshot = stages.get("p", "c");
    let weak = Arc::downgrade(&snapshot.0);
    assert!(stages.stage("p", "c", None, f.read("two")).is_err());
    assert!(stages.get("p", "foreign").0.is_empty());
    assert!(stages.remove("p", "c", "stale", &old.files[0].id).is_err());
    assert!(stages.remove("p", "c", &old.revision, "foreign").is_err());
    let next = stages
        .stage("p", "c", Some(&old.revision), f.read("two"))
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
    stages.stage("p", "c", None, f.read("one")).unwrap();
    let sent = stages.get("p", "c");
    stages.consume("p", "c", &sent);
    assert!(stages.list("p", "c").is_none());
    drop(sent);
    stages.stage("p", "c", None, f.read("one")).unwrap();
    stages.clear_chat("p", "c");
    assert!(stages.batches.is_empty());
    stages.stage("p", "c", None, f.read("one")).unwrap();
    stages.clear();
    assert!(stages.batches.is_empty());
}
#[test]
fn owner_and_memory_caps_and_settled_memory() {
    memory_sample("baseline");
    let f = Fixture::new();
    f.file("one", b"x");
    let mut stages = FileStages::default();
    for i in 0..OWNER_COUNT {
        stages
            .stage("p", &i.to_string(), None, f.read("one"))
            .unwrap();
    }
    assert!(stages.stage("p", "over", None, f.read("one")).is_err());
    stages.clear();
    let paths: Vec<_> = (0..4)
        .map(|i| f.file(&format!("large{i}"), &vec![1; TOTAL_BYTES / 4]))
        .collect();
    let large = read::read_paths(&paths).unwrap();
    let peak = large.retained_bytes();
    memory_sample("staged");
    let weak = Arc::downgrade(&large.0);
    stages.stage("p", "c", None, large).unwrap();
    let wire = stages.get("p", "c").wire("memory probe", true).unwrap();
    let wire_bytes = serde_json::to_vec(&wire).unwrap().len();
    drop(wire);
    stages.clear();
    assert!(weak.upgrade().is_none());
    memory_sample("settled");
    println!(
        "attachment_memory: native_payload_peak={peak} bytes; serialized_wire={wire_bytes} bytes; settled_payload=0 bytes; global_staging_cap={STORE_BYTES} bytes"
    );
    for i in 0..(STORE_BYTES / peak) {
        stages
            .stage("p", &i.to_string(), None, read::read_paths(&paths).unwrap())
            .unwrap();
    }
    assert!(
        stages
            .stage("p", "full", None, read::read_paths(&paths).unwrap())
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
    let socket = std::os::unix::net::UnixListener::bind(f.0.join("socket")).unwrap();
    assert!(read::read_paths(&[f.0.join("socket").to_str().unwrap().into()]).is_err());
    drop(socket);
}

fn memory_sample(label: &str) {
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        {
            println!("attachment_process_memory {label}: {line}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = label;
}
