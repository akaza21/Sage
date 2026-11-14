use super::*;
use crate::git_process::{GitProcessPolicy, GitProcessTestHarness, run};
use std::{fs, sync::mpsc};

#[test]
fn cancelling_an_in_flight_git_read_reaps_it_before_another_read() {
    let root = sage_test_support::test_paths::temp_dir()
        .join(format!("sage-git-window-cancel-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let initialized = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&root)
        .arg("init")
        .output()
        .unwrap();
    assert!(initialized.status.success());
    // Admission stays real, but occurs before the controlled process starts.
    tauri::async_runtime::block_on(run(
        &root,
        &["status", "--porcelain"],
        1024,
        GitProcessPolicy::read(crate::git_budget::READ),
    ))
    .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let (spawned, started) = mpsc::channel();
    let (resume, resumed) = mpsc::sync_channel(0);
    let resumed = std::sync::Mutex::new(resumed);
    let harness = GitProcessTestHarness::new();
    let (mut policy, wait_gate) =
        GitProcessPolicy::with_deadline_wait_gate(crate::git_budget::READ);
    policy.mutation = false;
    policy.visible_effect = false;
    policy.containment_registry = harness
        .mutation_policy(crate::git_budget::READ, crate::git_budget::ADMISSION)
        .containment_registry;
    // hash-object waits for our captured stdin while its spawn hook is held.
    policy.stdin = Some(b"window input".to_vec());
    let policy = policy.with_spawned_hook(move |_| {
        spawned.send(()).unwrap();
        resumed
            .lock()
            .unwrap()
            .recv_timeout(sage_test_support::test_wait::HANG_GUARD)
            .unwrap();
    });
    let worker = {
        let root = root.canonicalize().unwrap();
        let cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(CANCELLED.scope(
                cancelled,
                run(&root, &["hash-object", "--stdin"], 1024, policy),
            ))
        })
    };
    started
        .recv_timeout(sage_test_support::test_wait::HANG_GUARD)
        .unwrap();
    cancelled.store(true, Ordering::Release);
    resume.send(()).unwrap();
    let result = worker.join().unwrap();
    drop(wait_gate);
    assert!(
        matches!(result, Err(sage_core::error::AppError::Git(message)) if message.contains("cancelled"))
    );
    assert_eq!(harness.observation().available_reaper_permits, 8);
    let later = tauri::async_runtime::block_on(crate::git_process::run_cancellable(
        &root.canonicalize().unwrap(),
        &["status", "--porcelain"],
        1024,
        None,
        Arc::new(AtomicBool::new(false)),
    ))
    .unwrap();
    assert!(later.status.success());
    fs::remove_dir_all(root).unwrap();
}
