mod bounds;
mod discovery;
#[cfg(windows)]
mod windows_recovery;

use std::{
    fs,
    net::TcpListener,
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewScriptName, PreviewTargetRef},
    health, lifecycle,
    projection::{PreviewFailure, PreviewPhase, PreviewProjection, PreviewProjectionOwner},
};
use crate::acp::contract::{
    ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
};
use crate::acp::scheduler::RuntimeOwner;

#[test]
fn stopping_an_unstarted_target_does_not_cancel_the_next_epoch_start() {
    let root =
        std::env::temp_dir().join(format!("sage-preview-stop-start-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(root.join("server.js"), "setInterval(() => {}, 1000);").unwrap();
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let next_target = PreviewTargetRef {
        project_generation: ProjectGeneration::new(target.project_generation.get() + 1).unwrap(),
        expected_transition_epoch: target.expected_transition_epoch + 1,
        ..target.clone()
    };
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    assert!(
        lifecycle::stop(&target).is_err(),
        "a target with no reserved child cannot record a future cancellation"
    );
    let next_request = PreviewDiscoveryRequest {
        project_generation: next_target.project_generation,
        expected_transition_epoch: next_target.expected_transition_epoch,
        ..request
    };
    lifecycle::replace_discovered(&next_request, vec![(next_target.clone(), root.clone())]);
    lifecycle::begin_start(&next_target, &|_| Ok(())).unwrap();
    let reservation = lifecycle::reserve_start(next_target.clone())
        .expect("a stale Stop must not reject the next epoch's Start");
    lifecycle::stop(&next_target).unwrap();
    drop(reservation);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_epoch_rediscovery_stops_a_starting_child_that_cannot_be_promoted() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let root = std::env::temp_dir().join(format!("sage-preview-start-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("server.js"),
        r#"const fs = require("fs");
const http = require("http");
const server = http.createServer((_, response) => response.end());
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  fs.writeFileSync("preview-port", String(port));
  console.log(`http://127.0.0.1:${port}`);
});
"#,
    )
    .unwrap();
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    let start = lifecycle::reserve_start(target.clone()).unwrap();
    lifecycle::mark_waiting_for_origin(&target, &|_| Ok(())).unwrap();
    let port_file = root.join("preview-port");
    let deadline = Instant::now() + sage_test_support::test_wait::BUDGET;
    let port = loop {
        if let Ok(value) = fs::read_to_string(&port_file)
            && let Ok(port) = value.parse()
        {
            break port;
        }
        assert!(
            Instant::now() < deadline,
            "preview child did not publish its listening port"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let rediscovered_target = PreviewTargetRef {
        root_id: crate::acp::contract::SageRootId::parse("new-root-preview").unwrap(),
        ..target.clone()
    };
    lifecycle::replace_discovered(&request, vec![(rediscovered_target, root.clone())]);
    assert!(matches!(
        lifecycle::finish_start(start, &|_| Ok(())),
        Err(super::error::PreviewError::Conflict {
            resource: "activePreview",
            ..
        })
    ));
    let deadline = Instant::now() + sage_test_support::test_wait::BUDGET;
    loop {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                break;
            }
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("preview child still owns port {port}: {error}"),
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn https_health_is_fail_closed() {
    assert!(!health::get(Some("https://127.0.0.1:443")));
}

#[test]
fn child_owning_failures_reap_before_terminal_projection() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let generous = sage_test_support::test_wait::BUDGET.as_millis() as u64;
    let cases = [
        (
            "console.log('http://example.com:3000'); setInterval(() => {}, 1000);",
            generous,
            PreviewPhase::HealthFailed,
            PreviewFailure::OriginNotLoopback,
        ),
        (
            "process.exit(2);",
            generous,
            PreviewPhase::Exited,
            PreviewFailure::ExitedEarly,
        ),
        (
            "setInterval(() => {}, 1000);",
            1_000,
            PreviewPhase::HealthFailed,
            PreviewFailure::StartupTimeout,
        ),
        (
            "console.log('http://127.0.0.1:9'); setInterval(() => {}, 1000);",
            generous,
            PreviewPhase::HealthFailed,
            PreviewFailure::HealthTimeout,
        ),
    ];
    for (script, startup_wait_ms, expected_phase, expected_failure) in cases {
        super::lifecycle_process::test_override_startup_wait(startup_wait_ms);
        let root =
            std::env::temp_dir().join(format!("sage-preview-failure-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"node server.js"}}"#,
        )
        .unwrap();
        fs::write(root.join("server.js"), script).unwrap();
        let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
        let request = request_for(&target);
        lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
        let observed = Mutex::new(Vec::new());
        let observer = |projection: &PreviewProjection| {
            observed
                .lock()
                .unwrap()
                .push((projection.phase, projection.failure));
            Ok(())
        };
        lifecycle::begin_start(&target, &observer).unwrap();
        let reservation = lifecycle::reserve_start(target.clone()).unwrap();
        lifecycle::mark_waiting_for_origin(&target, &observer).unwrap();
        let outcome = lifecycle::finish_start(reservation, &observer);
        super::lifecycle_process::test_override_startup_wait(0);
        assert!(outcome.is_err());
        assert_eq!(lifecycle::test_owned_child_count(), 0);
        assert_eq!(
            observed.lock().unwrap().last().copied(),
            Some((expected_phase, Some(expected_failure)))
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn https_origin_fails_as_unsupported_before_the_health_deadline() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let root = preview_package(
        "https-origin",
        "console.log('https://127.0.0.1:3443'); setInterval(() => {}, 1000);",
    );
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    let observed = Mutex::new(Vec::new());
    let observer = |projection: &PreviewProjection| {
        observed
            .lock()
            .unwrap()
            .push((projection.phase, projection.failure));
        Ok(())
    };
    lifecycle::begin_start(&target, &observer).unwrap();
    let reservation = lifecycle::reserve_start(target.clone()).unwrap();
    lifecycle::mark_waiting_for_origin(&target, &observer).unwrap();
    let started = Instant::now();
    let outcome = lifecycle::finish_start(reservation, &observer);
    let elapsed = started.elapsed();
    assert!(matches!(
        outcome,
        Err(super::error::PreviewError::Unavailable { .. })
    ));
    assert!(
        elapsed < Duration::from_millis(crate::acp::limits::PREVIEW_LIMITS.health_deadline_ms),
        "https origin waited {elapsed:?}, as long as the health deadline"
    );
    assert_eq!(lifecycle::test_owned_child_count(), 0);
    let observed = observed.into_inner().unwrap();
    assert!(
        !observed
            .iter()
            .any(|(phase, _)| *phase == PreviewPhase::HealthChecking),
        "an https origin must not enter health checking: {observed:?}"
    );
    assert_eq!(
        observed.last().copied(),
        Some((
            PreviewPhase::HealthFailed,
            Some(PreviewFailure::OriginUnsupported)
        ))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_and_duplicate_stop_preserve_foreign_owner_and_reap_before_stopped() {
    let root = std::env::temp_dir().join(format!("sage-preview-stop-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("server.js"),
        r#"const http = require("http");
const server = http.createServer((_, response) => response.end());
server.listen(0, "127.0.0.1", () => console.log(`http://127.0.0.1:${server.address().port}`));
"#,
    )
    .unwrap();
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    let phases = Mutex::new(Vec::new());
    let observer = |projection: &PreviewProjection| {
        phases.lock().unwrap().push(projection.phase);
        Ok(())
    };
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    lifecycle::begin_start(&target, &observer).unwrap();
    let reservation = lifecycle::reserve_start(target.clone()).unwrap();
    lifecycle::mark_waiting_for_origin(&target, &observer).unwrap();
    lifecycle::finish_start(reservation, &observer).unwrap();
    assert_eq!(lifecycle::test_owned_child_count(), 1);
    let foreign = PreviewTargetRef {
        session_id: SageAgentSessionId::parse("session-foreign").unwrap(),
        ..target.clone()
    };
    assert!(lifecycle::stop(&foreign).is_err());
    assert_eq!(lifecycle::test_owned_child_count(), 1);
    phases.lock().unwrap().clear();
    lifecycle::stop_observed(&target, &observer).unwrap();
    assert_eq!(
        *phases.lock().unwrap(),
        [PreviewPhase::Stopping, PreviewPhase::Stopped]
    );
    assert_eq!(lifecycle::test_owned_child_count(), 0);
    phases.lock().unwrap().clear();
    lifecycle::stop_observed(&target, &observer).unwrap();
    assert!(phases.lock().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_transition_reaps_starting_child_between_stopping_and_stopped() {
    let root =
        std::env::temp_dir().join(format!("sage-preview-transition-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(root.join("server.js"), "setInterval(() => {}, 1000);").unwrap();
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    let reservation = lifecycle::reserve_start(target.clone()).unwrap();
    lifecycle::mark_waiting_for_origin(&target, &|_| Ok(())).unwrap();
    let phases = Mutex::new(Vec::new());
    lifecycle::stop_before_epoch_observed(5, &|projection| {
        phases.lock().unwrap().push(projection.phase);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        *phases.lock().unwrap(),
        [PreviewPhase::Stopping, PreviewPhase::Stopped]
    );
    assert_eq!(lifecycle::test_owned_child_count(), 0);
    assert!(lifecycle::test_projection().is_none());
    drop(reservation);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn admitted_same_owner_conflict_is_projected_but_observer_rejection_commits_nothing() {
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), std::env::temp_dir())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    assert!(lifecycle::begin_start(&target, &|_| Ok(())).is_err());
    let conflicted = lifecycle::test_projection().unwrap();
    assert_eq!(conflicted.phase, PreviewPhase::Starting);
    assert_eq!(conflicted.failure, Some(PreviewFailure::Conflict));
    let before = conflicted.clone();
    assert!(
        lifecycle::mark_waiting_for_origin(&target, &|_| Err(
            super::error::PreviewError::internal("observer rejected stale owner")
        ))
        .is_err()
    );
    assert_eq!(lifecycle::test_projection(), Some(before));
}

#[test]
fn rejected_stale_owner_cannot_overwrite_or_launch_without_a_child() {
    let current = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let stale = PreviewTargetRef {
        chat_id: SageChatId::parse("chat-stale").unwrap(),
        ..current.clone()
    };
    let request = request_for(&current);
    lifecycle::replace_discovered(&request, vec![(current.clone(), std::env::temp_dir())]);
    let before = lifecycle::test_projection().unwrap();
    assert!(
        lifecycle::transition_for_owner(
            PreviewProjectionOwner::from_target(&stale),
            PreviewPhase::Discovering,
            None,
            None,
            None,
            &|_| Err(super::error::PreviewError::internal("stale observer")),
        )
        .is_err()
    );
    assert_eq!(lifecycle::test_projection(), Some(before));
    assert!(lifecycle::reserve_start(stale).is_err());
    assert_eq!(lifecycle::test_owned_child_count(), 0);
}

#[test]
fn runtime_retirement_rejects_foreign_owner_and_clears_only_the_exact_owner() {
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), std::env::temp_dir())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    let foreign = RuntimeOwner {
        project_id: target.project_id.clone(),
        chat_id: SageChatId::parse("chat-foreign").unwrap(),
        session_id: target.session_id.clone(),
        runtime_epoch: target.runtime_epoch,
    };
    lifecycle::retire_runtime_owner(&foreign).unwrap();
    assert_eq!(
        lifecycle::test_projection().unwrap().phase,
        PreviewPhase::Starting
    );
    lifecycle::retire_runtime_owner(&RuntimeOwner {
        project_id: target.project_id.clone(),
        chat_id: target.chat_id.clone(),
        session_id: target.session_id.clone(),
        runtime_epoch: target.runtime_epoch,
    })
    .unwrap();
    assert!(lifecycle::test_projection().is_none());
    assert_eq!(lifecycle::test_owned_child_count(), 0);
    lifecycle::retire_runtime_owner(&RuntimeOwner {
        project_id: target.project_id,
        chat_id: target.chat_id,
        session_id: target.session_id,
        runtime_epoch: target.runtime_epoch,
    })
    .unwrap();
}

#[test]
fn cleanup_slot_blocks_foreign_and_replacement_start_until_tree_zero() {
    let root = preview_package(
        "cleanup-slot",
        r#"const http = require("http");
const server = http.createServer((_, response) => response.end());
server.listen(0, "127.0.0.1", () => console.log(`http://127.0.0.1:${server.address().port}`));"#,
    );
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let replacement = PreviewTargetRef {
        root_id: crate::acp::contract::SageRootId::parse("replacement-root-preview").unwrap(),
        ..target.clone()
    };
    let foreign = PreviewTargetRef {
        session_id: SageAgentSessionId::parse("session-cleanup-foreign").unwrap(),
        ..replacement.clone()
    };
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    let reservation = lifecycle::reserve_start(target.clone()).unwrap();
    lifecycle::mark_waiting_for_origin(&target, &|_| Ok(())).unwrap();
    lifecycle::finish_start(reservation, &|_| Ok(())).unwrap();

    let cleanup = lifecycle::CleanupTestStep::capture(&target, Vec::new()).unwrap();
    assert!(lifecycle::test_cleanup_occupied(&target));
    assert_eq!(lifecycle::test_owned_child_count(), 1);

    // Neither an unrelated runtime retirement nor an epoch pass with no stale
    // owner may release a cleanup slot taken by this exact child.
    lifecycle::retire_runtime_owner(&RuntimeOwner {
        project_id: target.project_id.clone(),
        chat_id: SageChatId::parse("chat-cleanup-foreign").unwrap(),
        session_id: target.session_id.clone(),
        runtime_epoch: target.runtime_epoch,
    })
    .unwrap();
    lifecycle::stop_before_epoch(4);
    assert!(lifecycle::test_cleanup_occupied(&target));

    lifecycle::replace_discovered(&request, vec![(replacement.clone(), root.clone())]);
    assert!(lifecycle::begin_start(&replacement, &|_| Ok(())).is_err());
    assert!(lifecycle::begin_start(&foreign, &|_| Ok(())).is_err());
    assert!(lifecycle::reserve_start(replacement.clone()).is_err());
    assert!(lifecycle::reserve_start(foreign).is_err());
    assert_eq!(lifecycle::test_owned_child_count(), 1);

    cleanup.finish().unwrap();
    assert_eq!(lifecycle::test_owned_child_count(), 0);
    assert!(!lifecycle::test_cleanup_occupied(&target));

    lifecycle::replace_discovered(&request, vec![(replacement.clone(), root.clone())]);
    lifecycle::begin_start(&replacement, &|_| Ok(())).unwrap();
    let reservation = lifecycle::reserve_start(replacement.clone())
        .expect("tree-zero cleanup must release exactly its occupied slot");
    lifecycle::stop(&replacement).unwrap();
    drop(reservation);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_failure_retains_its_exact_child_and_capacity_until_reconciled() {
    let root = preview_package("cleanup-failure", "setInterval(() => {}, 1000);");
    let target = preview_target(&format!("project-preview-{}", uuid::Uuid::new_v4()), 4, 4);
    let replacement = PreviewTargetRef {
        root_id: crate::acp::contract::SageRootId::parse("failure-replacement-root").unwrap(),
        ..target.clone()
    };
    let request = request_for(&target);
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
    let reservation = lifecycle::reserve_start(target.clone()).unwrap();
    let (release_first, wait_first) = mpsc::sync_channel(0);
    let (release_second, wait_second) = mpsc::sync_channel(0);
    let cleanup = lifecycle::CleanupTestStep::capture(
        &target,
        vec![
            thread::spawn(move || {
                let _ = wait_first.recv();
            }),
            thread::spawn(move || {
                let _ = wait_second.recv();
            }),
        ],
    )
    .unwrap();
    drop(reservation);

    assert!(
        cleanup.finish().is_err(),
        "drainer reconciliation must fail closed"
    );
    assert!(lifecycle::test_cleanup_occupied(&target));
    assert_eq!(lifecycle::test_owned_child_count(), 1);
    lifecycle::replace_discovered(&request, vec![(replacement.clone(), root.clone())]);
    assert!(lifecycle::reserve_start(replacement.clone()).is_err());

    release_first.send(()).unwrap();
    release_second.send(()).unwrap();
    cleanup.finish().unwrap();
    assert!(!lifecycle::test_cleanup_occupied(&target));
    assert_eq!(lifecycle::test_owned_child_count(), 0);

    lifecycle::replace_discovered(&request, vec![(replacement.clone(), root.clone())]);
    lifecycle::begin_start(&replacement, &|_| Ok(())).unwrap();
    let replacement_reservation = lifecycle::reserve_start(replacement.clone())
        .expect("only successful reconciliation may free capacity");
    lifecycle::stop(&replacement).unwrap();
    drop(replacement_reservation);
    fs::remove_dir_all(root).unwrap();
}

pub(super) fn discovery_request(
    project: &str,
    generation: u64,
    transition: u64,
) -> PreviewDiscoveryRequest {
    PreviewDiscoveryRequest {
        project_id: SageProjectId::parse(project).unwrap(),
        project_generation: ProjectGeneration::new(generation).unwrap(),
        expected_transition_epoch: transition,
        chat_id: SageChatId::parse("chat-preview").unwrap(),
        session_id: SageAgentSessionId::parse("session-preview").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
    }
}

fn preview_target(project: &str, generation: u64, transition: u64) -> PreviewTargetRef {
    let request = discovery_request(project, generation, transition);
    PreviewTargetRef {
        project_id: request.project_id,
        project_generation: request.project_generation,
        expected_transition_epoch: request.expected_transition_epoch,
        chat_id: request.chat_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
        root_id: crate::acp::contract::SageRootId::parse("root-preview").unwrap(),
        script_name: PreviewScriptName::Dev,
    }
}

fn request_for(target: &PreviewTargetRef) -> PreviewDiscoveryRequest {
    PreviewDiscoveryRequest {
        project_id: target.project_id.clone(),
        project_generation: target.project_generation,
        expected_transition_epoch: target.expected_transition_epoch,
        chat_id: target.chat_id.clone(),
        session_id: target.session_id.clone(),
        runtime_epoch: target.runtime_epoch,
    }
}

fn preview_package(prefix: &str, script: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sage-preview-{prefix}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(root.join("server.js"), script).unwrap();
    root
}
