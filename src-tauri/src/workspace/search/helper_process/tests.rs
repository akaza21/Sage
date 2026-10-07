use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};

use super::*;
use crate::workspace::search::{helper_protocol::HelperRequest, tests::request};

#[test]
fn sleeping_child_fixture() {
    if std::env::var_os("SAGE_SEARCH_SLEEP_FIXTURE").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

#[test]
fn clean_environment_fixture() {
    if std::env::var_os("SAGE_SEARCH_ASSERT_CLEAN_ENV").is_some() {
        assert!(std::env::var_os("PATH").is_none());
        assert!(std::env::var_os("SAGE_DISPOSABLE_PROFILE_ROOT").is_none());
        assert!(std::env::var_os("SAGE_PROJECT_ROOT").is_none());
    }
}

fn test_lease() -> OwnedSemaphorePermit {
    Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()
}

fn sleeping_process() -> ProcessSpec {
    ProcessSpec {
        executable: std::env::current_exe().unwrap(),
        arguments: vec![
            "--exact".into(),
            "workspace::search::helper_process::tests::sleeping_child_fixture".into(),
            "--nocapture".into(),
        ],
        environment: vec![("SAGE_SEARCH_SLEEP_FIXTURE".into(), "1".into())],
        clear_environment: true,
    }
}

#[test]
fn deadline_kills_and_reaps_the_actual_helper_process() {
    crate::test_wait::guarded("search helper run deadline", async {
        let (_cancel, receiver) = oneshot::channel();
        let error = run_with_spec(
            HelperRequest::new(request("needle"), Vec::new()),
            receiver,
            sleeping_process(),
            Duration::from_millis(100),
            Some(REAP_DEADLINE),
            test_lease(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("deadline"));
    });
}

#[test]
fn cancellation_kills_and_reaps_the_actual_helper_process() {
    crate::test_wait::guarded("search helper cancellation", async {
        let (cancel, receiver) = oneshot::channel();
        let cancellation = tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            let _ = cancel.send(());
        });
        let error = run_with_spec(
            HelperRequest::new(request("needle"), Vec::new()),
            receiver,
            sleeping_process(),
            Duration::from_secs(5),
            Some(REAP_DEADLINE),
            test_lease(),
        )
        .await
        .unwrap_err();
        cancellation.await.unwrap();
        assert!(error.to_string().contains("cancelled"));
    });
}

#[test]
fn forced_reap_handoff_keeps_the_process_under_a_registered_owner() {
    crate::test_wait::guarded("search helper forced reap handoff", async {
        let (_cancel, receiver) = oneshot::channel();
        let error = run_with_spec(
            HelperRequest::new(request("needle"), Vec::new()),
            receiver,
            sleeping_process(),
            Duration::from_millis(100),
            None,
            test_lease(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("cleanup remains contained"));
    });
}

#[test]
fn stalled_reap_policy_returns_at_its_bound() {
    crate::test_wait::guarded("search helper stalled reap bound", async {
        let reaped = bounded_reap(
            std::future::pending::<std::io::Result<std::process::ExitStatus>>(),
            Duration::from_millis(25),
        )
        .await;
        assert!(!reaped);
    });
}

#[test]
fn exact_helper_process_receives_only_its_allowlisted_environment() {
    crate::test_wait::guarded("search helper environment allowlist", async {
        let spec = ProcessSpec {
            executable: std::env::current_exe().unwrap(),
            arguments: vec![
                "--exact".into(),
                "workspace::search::helper_process::tests::clean_environment_fixture".into(),
                "--nocapture".into(),
            ],
            environment: vec![("SAGE_SEARCH_ASSERT_CLEAN_ENV".into(), "1".into())],
            clear_environment: true,
        };
        let status = configured_command(&spec)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success());
    });
}
