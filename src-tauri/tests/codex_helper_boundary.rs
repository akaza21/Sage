#![cfg(all(windows, feature = "codex-provisioning-helper"))]

use std::fs;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::time::Instant;

use sage_lib::codex_helper_boundary::{
    BoundaryError, BoundaryStatus, current_process_elevated, direct_non_elevated_probe,
};
use uuid::Uuid;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-helper-boundary-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn direct_probe_honours_caller_elevation_without_residue() {
    let root = TempRoot::new();
    let helper = root.0.join("sage-codex-0146-elevated-helper.exe");
    fs::copy(
        env!("CARGO_BIN_EXE_sage-codex-0146-elevated-helper"),
        &helper,
    )
    .unwrap();
    let before = fs::read_dir(&root.0).unwrap().count();

    let result = direct_non_elevated_probe(
        &helper,
        b"sealed-core-request-placeholder-v1",
        Duration::from_secs(10),
    );

    if current_process_elevated().unwrap() {
        // Elevated callers are refused before the helper starts.
        assert_eq!(result, Err(BoundaryError::Authority));
    } else {
        let receipt = result.unwrap();
        assert_eq!(receipt.status, BoundaryStatus::ElevationRequired);
        assert!(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&receipt.pipe_name)
                .is_err()
        );
    }
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), before);
    assert_eq!(
        fs::read(&helper).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_sage-codex-0146-elevated-helper")).unwrap()
    );
}

#[test]
fn unconnected_helper_exits_at_its_deadline_without_residue() {
    let root = TempRoot::new();
    let helper = root.0.join("sage-codex-0146-elevated-helper.exe");
    fs::copy(
        env!("CARGO_BIN_EXE_sage-codex-0146-elevated-helper"),
        &helper,
    )
    .unwrap();
    assert!(fs::metadata(&helper).unwrap().len() <= 32 * 1024 * 1024);
    let pipe = format!(r"\\.\pipe\sage-codex-0146-{}", Uuid::new_v4().simple());
    let mut child = Command::new(&helper)
        .arg("--sage-codex-helper-boundary-v1")
        .arg(&pipe)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "helper exceeded its deadline");
        std::thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(status.code(), Some(16));
    assert!(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe)
            .is_err()
    );
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
}
