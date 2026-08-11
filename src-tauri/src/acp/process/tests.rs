use std::ffi::OsString;
use std::path::PathBuf;

use uuid::Uuid;

use crate::acp::limits::LIMITS;

use super::supervisor::LaunchFailureTracker;
use super::*;

#[cfg(not(windows))]
fn launch_spec(path: PathBuf) -> LaunchSpec {
    LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 1,
        artifact: ArtifactExpectation::new(Uuid::nil(), path, [7; 32], 10).unwrap(),
        fixed_args: BoundedOsArgs::new(Vec::new()).unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        working_directory: CanonicalNativeDirectory::new(
            std::env::current_dir().unwrap().canonicalize().unwrap(),
        )
        .unwrap(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        #[cfg(all(test, windows))]
        test_after_artifact_verified: None,
    }
}

#[test]
fn argument_and_environment_exact_plus_one_are_rejected() {
    let exact_arguments =
        vec![OsString::from("a"); usize::try_from(LIMITS.process.arguments).unwrap()];
    assert!(BoundedOsArgs::new(exact_arguments).is_ok());
    let over_arguments =
        vec![OsString::from("a"); usize::try_from(LIMITS.process.arguments + 1).unwrap()];
    assert_eq!(
        BoundedOsArgs::new(over_arguments).err(),
        Some(LaunchError::ArgumentLimit)
    );

    let exact_environment = (0..LIMITS.process.environment_entries)
        .map(|index| (OsString::from(format!("K{index}")), OsString::from("v")))
        .collect();
    assert!(BoundedEnvironment::new(exact_environment).is_ok());
    let over_environment = (0..=LIMITS.process.environment_entries)
        .map(|index| (OsString::from(format!("K{index}")), OsString::from("v")))
        .collect();
    assert_eq!(
        BoundedEnvironment::new(over_environment).err(),
        Some(LaunchError::EnvironmentLimit)
    );
}

#[test]
fn launch_inputs_reject_nul_invalid_environment_and_case_aliases() {
    assert_eq!(
        BoundedOsArgs::new(vec![OsString::from("a\0b")]).err(),
        Some(LaunchError::InvalidArgument)
    );
    assert_eq!(
        BoundedEnvironment::new(vec![(OsString::from("A=B"), OsString::from("v"))]).err(),
        Some(LaunchError::InvalidEnvironment)
    );
    assert_eq!(
        BoundedEnvironment::new(vec![
            (OsString::from("Path"), OsString::from("a")),
            (OsString::from("PATH"), OsString::from("b")),
        ])
        .err(),
        Some(LaunchError::InvalidEnvironment)
    );
}

#[cfg(windows)]
#[test]
fn canonical_native_directory_rejects_verbatim_parent_traversal() {
    assert_eq!(
        CanonicalNativeDirectory::new(PathBuf::from(r"\\?\C:\repo\..\escape")).err(),
        Some(LaunchError::InvalidAbsolutePath)
    );
}

#[cfg(not(windows))]
#[test]
fn unsupported_platform_fails_before_artifact_access() {
    let missing = std::env::current_dir()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join("definitely-missing-acp-fixture");
    let spec = launch_spec(missing.clone());
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let failure =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .err()
            .unwrap();
    assert_eq!(failure.error(), LaunchError::PlatformUnavailable);
    assert_eq!(
        failure.into_zero_process().unwrap().runtime_epoch(),
        spec.runtime_epoch
    );
    assert_eq!(pool.usage(), (0, 0));
    assert!(!missing.exists());
}

#[test]
fn launch_zero_proof_requires_observation_and_is_exactly_once() {
    let mut pre_spawn = LaunchFailureTracker::pre_spawn(41);
    assert!(!pre_spawn.child_was_created());
    assert_eq!(pre_spawn.take_zero_token().unwrap().runtime_epoch(), 41);
    assert!(pre_spawn.take_zero_token().is_none());

    let mut cleaned_child = LaunchFailureTracker::child_created(42);
    assert!(cleaned_child.child_was_created());
    assert!(cleaned_child.take_zero_token().is_none());
    cleaned_child.confirm_zero();
    assert_eq!(cleaned_child.take_zero_token().unwrap().runtime_epoch(), 42);
    assert!(cleaned_child.take_zero_token().is_none());
}

#[test]
fn containment_and_deadline_unknown_never_mint_zero_proof() {
    for error in [LaunchError::ContainmentFailure, LaunchError::Deadline] {
        let mut unknown = LaunchFailureTracker::child_created(91);
        assert_eq!(unknown.finish_zero(Err(error)).err(), Some(error));
        assert!(unknown.take_zero_token().is_none());
    }
}

#[test]
fn capacity_lease_is_mandatory_bounded_and_fail_closed_after_activation() {
    assert!(std::sync::Arc::ptr_eq(
        &TreeCapacityPool::global(),
        &TreeCapacityPool::global()
    ));
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let lease = pool.try_acquire(32).unwrap();
    assert_eq!(pool.usage(), (1, 32));
    assert_eq!(
        pool.try_acquire(1).err(),
        Some(LaunchError::CapacityUnavailable)
    );
    drop(lease);
    assert_eq!(pool.usage(), (0, 0));

    let mut active = pool.try_acquire(32).unwrap();
    active.activate();
    drop(active);
    assert_eq!(pool.usage(), (1, 32));
}

#[test]
fn zero_release_is_exactly_once_for_prelaunch_and_active_leases() {
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let mut prelaunch = pool.try_acquire(32).unwrap();
    assert_eq!(prelaunch.release_prelaunch_after_zero(), Ok(()));
    assert_eq!(
        prelaunch.release_prelaunch_after_zero(),
        Err(LaunchError::ContainmentFailure)
    );
    assert_eq!(pool.usage(), (0, 0));

    let mut active = pool.try_acquire(32).unwrap();
    active.activate();
    assert_eq!(active.release_active_after_zero(), Ok(()));
    assert_eq!(
        active.release_active_after_zero(),
        Err(LaunchError::ContainmentFailure)
    );
    assert_eq!(pool.usage(), (0, 0));
}

#[test]
fn cancellation_releases_only_after_zero_process_convergence() {
    let mut lifecycle = SupervisorLifecycle::starting(32);
    assert!(lifecycle.mark_running());
    assert!(lifecycle.request_termination(TreeExit::Cancelled));
    assert_eq!(
        lifecycle.observe_processes(false, 1),
        TreeState::Terminating
    );
    assert!(!lifecycle.release_reservation());
    assert_eq!(lifecycle.observe_processes(false, 0), TreeState::Exited);
    assert_eq!(lifecycle.exit(), Some(TreeExit::Cancelled));
    assert!(lifecycle.release_reservation());
    assert!(!lifecycle.release_reservation());
}

#[test]
fn orphan_or_descendant_overflow_fails_closed() {
    let mut lifecycle = SupervisorLifecycle::starting(1);
    lifecycle.mark_running();
    assert_eq!(
        lifecycle.observe_processes(true, 2),
        TreeState::ContainmentLost
    );
    assert!(!lifecycle.release_reservation());

    let mut orphan = SupervisorLifecycle::starting(32);
    orphan.mark_running();
    orphan.request_termination(TreeExit::ParentShutdown);
    orphan.observe_processes(false, 1);
    orphan.mark_containment_deadline();
    assert_eq!(orphan.state(), TreeState::ContainmentLost);
    assert!(!orphan.release_reservation());
}

#[test]
fn teardown_order_places_zero_convergence_before_release() {
    let lifecycle = SupervisorLifecycle::starting(32);
    assert_eq!(
        lifecycle.teardown_plan(),
        [
            TeardownAction::CloseStdin,
            TeardownAction::TerminateTree,
            TeardownAction::AwaitChildExit,
            TeardownAction::AwaitZeroProcesses,
            TeardownAction::CloseNativeHandles,
            TeardownAction::ReleaseReservation,
        ]
    );
}

#[cfg(windows)]
fn verified_fixture_spec(case: &str, seed: u32) -> LaunchSpec {
    let path = PathBuf::from(
        std::env::var_os("SAGE_ACP_FIXTURE_PATH")
            .expect("ignored Windows fixture tests require SAGE_ACP_FIXTURE_PATH"),
    );
    verified_fixture_spec_at(case, seed, path)
}

#[cfg(windows)]
fn verified_fixture_spec_at(case: &str, seed: u32, path: PathBuf) -> LaunchSpec {
    use std::io::Read;

    use sha2::{Digest, Sha256};

    let path = path.canonicalize().unwrap();
    let mut file = std::fs::File::open(&path).unwrap();
    let bytes = file.metadata().unwrap().len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 1,
        artifact: ArtifactExpectation::new(Uuid::nil(), path, hasher.finalize().into(), bytes)
            .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![
            OsString::from("--case"),
            OsString::from(case),
            OsString::from("--seed"),
            OsString::from(seed.to_string()),
        ])
        .unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        working_directory: CanonicalNativeDirectory::new(
            std::env::current_dir().unwrap().canonicalize().unwrap(),
        )
        .unwrap(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires a separately built, path-bound acp-adversarial-fixture"]
fn windows_verified_fixture_streams_and_exits_cleanly() {
    let spec = verified_fixture_spec("frame-lf-split-every-byte", 42);
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .unwrap();
    let (mut stdin, mut stdout, control) = tree.into_parts();
    let identity = control.process_identity_receipt().unwrap();
    assert_eq!(identity.runtime_epoch(), spec.runtime_epoch);
    assert_ne!(identity.process_id(), 0);
    assert_ne!(identity.creation_time_filetime_100ns(), 0);
    assert_eq!(identity.executable_sha256(), spec.artifact.sha256);
    assert_eq!(identity.executable_bytes(), spec.artifact.byte_len);
    assert_eq!(control.process_identity_receipt().unwrap(), identity);
    let spawn = control.spawn_timing_receipt();
    assert!(spawn.frequency_hz() > 0);
    assert!(spawn.process_ready_tick() >= spawn.spawn_started_tick());
    assert!(spawn.duration_micros() > 0);
    let initialize = crate::acp::transport::OutboundFrame::new(
        b"{\"jsonrpc\":\"2.0\",\"method\":\"initialize\"}".to_vec(),
    )
    .unwrap();
    assert_eq!(
        stdin.write_frame(&initialize),
        crate::acp::transport::FrameWriteOutcome::Complete
    );
    stdin.close();
    let mut output = Vec::new();
    let mut chunk = [0_u8; 64];
    loop {
        let read = stdout.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..read]);
        assert!(output.len() <= 1024);
    }
    assert!(!output.is_empty());
    assert!(matches!(
        control.wait(),
        Ok(TreeExit::Exited { code: Some(0) })
    ));
    assert_eq!(control.process_identity_receipt().unwrap(), identity);
    assert_eq!(pool.usage(), (0, 0));
}

#[cfg(windows)]
#[test]
#[ignore = "requires a separately built, path-bound acp-adversarial-fixture"]
fn windows_cancel_and_parent_exit_converge_descendants() {
    let spec = verified_fixture_spec("descendants-exact-32", 30_000);
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .unwrap();
    let (_stdin, _stdout, control) = tree.into_parts();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(control.active_processes().unwrap() > 1);
    assert_eq!(
        control.terminate(TreeExit::Cancelled),
        Ok(TreeExit::Cancelled)
    );
    assert_eq!(pool.usage(), (0, 0));
}

#[cfg(windows)]
#[test]
#[ignore = "requires a separately built, path-bound acp-adversarial-fixture"]
fn windows_retained_artifact_handle_denies_replacement() {
    let source = PathBuf::from(std::env::var_os("SAGE_ACP_FIXTURE_PATH").unwrap());
    let directory = std::env::temp_dir().join(format!("sage-artifact-race-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let copy = directory.join("fixture.exe");
    std::fs::copy(source, &copy).unwrap();
    let spec =
        verified_fixture_spec_at("stale-project-generation-completion", 30_000, copy.clone());
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .unwrap();
    let (mut stdin, _stdout, control) = tree.into_parts();
    let frame = crate::acp::transport::OutboundFrame::new(b"{}".to_vec()).unwrap();
    assert_eq!(
        stdin.write_frame(&frame),
        crate::acp::transport::FrameWriteOutcome::Complete
    );
    assert!(std::fs::rename(&copy, directory.join("replacement.exe")).is_err());
    assert_eq!(
        control.terminate(TreeExit::Cancelled),
        Ok(TreeExit::Cancelled)
    );
    drop(control);
    drop(stdin);
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "requires a separately built, path-bound acp-adversarial-fixture"]
fn windows_stderr_stdout_and_cancel_are_independent() {
    use std::time::{Duration, Instant};

    let spec = verified_fixture_spec("stderr-ring-bytes-262145", 0);
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .unwrap();
    let (_stdin, mut stdout, control) = tree.into_parts();
    let reader = std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        stdout.read(&mut byte)
    });
    std::thread::sleep(Duration::from_millis(10));
    let started = Instant::now();
    control.request_termination(TreeExit::Cancelled).unwrap();
    assert!(started.elapsed() <= Duration::from_millis(100));
    assert_eq!(control.wait(), Ok(TreeExit::Cancelled));
    assert_eq!(reader.join().unwrap().unwrap(), 0);
    assert!(control.stderr_summary().unwrap().observed_bytes > 0);
    assert_eq!(pool.usage(), (0, 0));
}

#[cfg(windows)]
#[test]
fn windows_job_convergence_never_reopens_process_ids() {
    assert!(!include_str!("windows.rs").contains("OpenProcess"));
    assert!(!include_str!("windows/authority.rs").contains("OpenProcess"));
    assert!(!include_str!("windows/job.rs").contains("ProcessIdList"));
}
