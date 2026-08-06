use std::{
    fs,
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use crate::acp::{
    contract::RuntimeEpoch,
    process::{RunningTree, TreeCapacityPool, TreeControl, TreeExit},
    transport::{FrameWriteOutcome, OutboundFrame},
};

use super::{
    external_fixture_adapters::{fixture_artifact, launch_spec},
    external_fixture_support::TempRoot,
    generated_cases::FROZEN_CASES,
    oracle::assert_literal,
    test_support::{FixtureDeadline, launch_with_deadline},
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn windows_descendant_exact_literal_observer() {
    let (_pool, tree, deadline) = launch_case("descendants-exact-32", 1);
    let (mut stdin, _stdout, control) = tree.into_parts();
    wait_for_active_processes(&control, 33, deadline);
    let cancel = OutboundFrame::new(br#"{"jsonrpc":"2.0","method":"cancel"}"#.to_vec()).unwrap();
    assert_eq!(stdin.write_frame(&cancel), FrameWriteOutcome::Complete);
    stdin.close();
    let (_, _zero) = control.wait_zero_within(deadline.tree()).unwrap();
    let survivors = control.active_processes().unwrap();

    assert_case_literal(
        "descendants-exact-32",
        json!({
            "launchAccepted": true,
            "survivors": survivors,
        }),
    );
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn windows_job_exact_literal_observer() {
    let (pool, tree, deadline) = launch_case("windows-job-processes-33", 2);
    let (_stdin, _stdout, control) = tree.into_parts();
    wait_for_active_processes(&control, 33, deadline);
    let active = control.active_processes().unwrap();
    let descendant_ledger = pool.usage().1;
    let (_, _zero) = control
        .terminate_zero(TreeExit::Cancelled)
        .expect("exact Job tree must converge");

    assert_case_literal(
        "windows-job-processes-33",
        json!({
            "accepted": true,
            "jobActiveProcesses": active,
            "descendantLedger": descendant_ledger,
        }),
    );
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn windows_descendant_overflow_literal_observer() {
    let (_pool, tree, deadline) = launch_case("descendants-over-32", 3);
    let (_stdin, _stdout, control) = tree.into_parts();
    let (exit, _zero) = control.wait_zero_within(deadline.tree()).unwrap();
    let survivors = control.active_processes().unwrap();
    let failed = matches!(exit, TreeExit::Exited { code: Some(code) } if code != 0);

    assert_case_literal(
        "descendants-over-32",
        json!({
            "terminal": if failed { "failed" } else { "completed" },
            "launchAccepted": !failed,
            "survivors": survivors,
        }),
    );
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn windows_job_overflow_literal_observer() {
    let (pool, tree, deadline) = launch_case("windows-job-processes-34", 4);
    let (_stdin, _stdout, control) = tree.into_parts();
    wait_for_active_processes(&control, 33, deadline);
    let job_maximum = control.active_processes().unwrap();
    let descendant_maximum = pool.usage().1;
    let (exit, _zero) = control.wait_zero_within(deadline.tree()).unwrap();
    let accepted = matches!(exit, TreeExit::Exited { code: Some(0) });
    let tree_must_exit = control.active_processes().unwrap() == 0;

    assert_case_literal(
        "windows-job-processes-34",
        json!({
            "accepted": accepted,
            "treeMustExit": tree_must_exit,
            "jobActiveProcessMaximum": job_maximum,
            "descendantLedgerMaximum": descendant_maximum,
        }),
    );
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn windows_verified_artifact_replacement_literal_observer() {
    let deadline = FixtureDeadline::new();
    let temp = TempRoot::new("sage-artifact-literal");
    fs::create_dir_all(temp.path()).unwrap();
    let copy = temp.path().join("fixture.exe");
    let replacement = temp.path().join("replacement.exe");
    let (source, digest, bytes) = fixture_artifact();
    fs::copy(source, &copy).unwrap();
    let copy = copy.canonicalize().unwrap();
    fs::write(&replacement, b"replacement").unwrap();
    let mut spec = launch_spec(
        "frame-lf-split-every-byte",
        RuntimeEpoch::new(5).unwrap(),
        copy.clone(),
        digest,
        bytes,
    );
    let replacement_attempted = Arc::new(AtomicBool::new(false));
    let replacement_executed = Arc::new(AtomicBool::new(false));
    let attempted = Arc::clone(&replacement_attempted);
    let executed = Arc::clone(&replacement_executed);
    let hook_path = copy.clone();
    let hook_replacement = replacement.clone();
    spec.test_after_artifact_verified = Some(Box::new(move || {
        attempted.store(true, Ordering::SeqCst);
        let replaced = replace_file(&hook_path, &hook_replacement);
        if !replaced {
            assert_replacement_unmoved(&hook_path, &hook_replacement, bytes);
        }
        executed.store(replaced, Ordering::SeqCst);
    }));

    let pool = TreeCapacityPool::isolated_ade_0006a();
    let lease = pool.try_acquire(spec.limits.descendants).unwrap();
    let tree = launch_with_deadline(spec, lease, deadline).unwrap_or_else(|failure| {
        panic!(
            "replacement launch failed: {:?}; replacement executed: {}",
            failure.error(),
            replacement_executed.load(Ordering::SeqCst)
        )
    });
    assert!(replacement_attempted.load(Ordering::SeqCst));
    let (mut stdin, mut stdout, control) = tree.into_parts();
    let initialize =
        OutboundFrame::new(br#"{"jsonrpc":"2.0","method":"initialize"}"#.to_vec()).unwrap();
    assert_eq!(stdin.write_frame(&initialize), FrameWriteOutcome::Complete);
    stdin.close();
    let output = stdout.read_frame().unwrap().unwrap();
    let output: serde_json::Value = serde_json::from_slice(output.as_bytes()).unwrap();
    let spawned_original = output["method"] == "fixture/initializeSuccess";
    assert!(matches!(
        control.wait_root_exit(),
        Ok(TreeExit::Exited { code: Some(0) })
    ));
    let held_after_exit = !replace_file(&copy, &replacement);
    if held_after_exit {
        assert_replacement_unmoved(&copy, &replacement, bytes);
    }
    let (_, _zero) = control.wait_zero_within(deadline.tree()).unwrap();
    drop(control);
    drop(stdout);
    let released_after_tree = replace_file(&copy, &replacement);

    assert_case_literal(
        "verified-artifact-replacement-race",
        json!({
            "spawnedBytes": if spawned_original { "original-verified" } else { "replacement" },
            "replacementExecuted": replacement_executed.load(Ordering::SeqCst),
            "handleHeldThroughTreeExit": held_after_exit && released_after_tree,
        }),
    );
    temp.remove();
}

fn launch_case(
    case_id: &'static str,
    epoch: u64,
) -> (Arc<TreeCapacityPool>, RunningTree, FixtureDeadline) {
    let deadline = FixtureDeadline::new();
    let (path, digest, bytes) = fixture_artifact();
    let spec = launch_spec(
        case_id,
        RuntimeEpoch::new(epoch).unwrap(),
        path,
        digest,
        bytes,
    );
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let lease = pool.try_acquire(spec.limits.descendants).unwrap();
    let tree = launch_with_deadline(spec, lease, deadline).unwrap();
    (pool, tree, deadline)
}

fn assert_case_literal(case_id: &str, observed: Value) {
    let case = FROZEN_CASES.iter().find(|case| case.id == case_id).unwrap();
    assert_literal(case, observed);
}

fn replace_file(destination: &Path, replacement: &Path) -> bool {
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replacement = replacement
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both strings are NUL-terminated and remain live for the call.
    unsafe {
        MoveFileExW(
            replacement.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) != 0
    }
}

fn assert_replacement_unmoved(destination: &Path, replacement: &Path, expected_bytes: usize) {
    assert_eq!(
        fs::metadata(destination).unwrap().len(),
        expected_bytes as u64
    );
    assert_eq!(fs::metadata(replacement).unwrap().len(), 11);
}

fn wait_for_active_processes(control: &TreeControl, expected: u32, deadline: FixtureDeadline) {
    let end = deadline.operation_end_before_cleanup();
    loop {
        let active = control.active_processes().unwrap();
        if active == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < end,
            "fixture reached {active} active processes, expected {expected}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
