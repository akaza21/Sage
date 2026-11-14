use std::ffi::OsString;
use std::path::PathBuf;

use uuid::Uuid;

use crate::{
    acp::contract::{ProjectGeneration, SageProjectId, SageRootId},
    project_registry::ProjectRootAuthority,
};
use sage_core::limits::LIMITS;

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
        project_root: test_project_root(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        #[cfg(all(test, windows))]
        test_after_artifact_verified: None,
    }
}

fn test_project_root() -> ProjectRootAuthority {
    ProjectRootAuthority::for_test(
        SageProjectId::parse("project-process-test").unwrap(),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-process-test").unwrap(),
        &std::env::current_dir().unwrap(),
    )
    .unwrap()
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

#[test]
fn batch_launch_contract_accepts_only_cmd_and_bat_launchers() {
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    for extension in ["cmd", "bat", "CMD", "BAT"] {
        let artifact = ArtifactExpectation::windows_batch_launcher(
            Uuid::nil(),
            root.join(format!("codex.{extension}")),
            [7; 32],
            10,
        )
        .expect("npm launcher extension");
        assert_eq!(
            artifact.entrypoint_kind,
            EntryPointKind::WindowsBatchLauncher
        );
    }
    assert_eq!(
        ArtifactExpectation::windows_batch_launcher(
            Uuid::nil(),
            root.join("codex.exe"),
            [7; 32],
            10
        )
        .err(),
        Some(LaunchError::InvalidAbsolutePath)
    );
}

#[cfg(not(windows))]
#[test]
fn cooperative_wsl_supervisor_rejects_windows_tree_requirement_before_artifact_access() {
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
    assert_eq!(failure.error(), LaunchError::ContainmentFailure);
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
    let trees = sage_core::limits::LIMITS.process.operational_live_trees as u32;
    let leases: Vec<_> = (0..trees).map(|_| pool.try_acquire(32).unwrap()).collect();
    assert_eq!(pool.usage(), (trees, 32 * trees));
    assert_eq!(
        pool.try_acquire(1).err(),
        Some(LaunchError::CapacityUnavailable)
    );
    drop(leases);
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
        project_root: test_project_root(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    }
}

#[cfg(windows)]
#[test]
fn windows_batch_launcher_rejects_a_non_npm_script_before_execution() {
    use sha2::{Digest, Sha256};

    let directory = std::env::temp_dir().join(format!("sage-batch-launcher-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let launcher = directory.join("codex.cmd");
    let contents = b"@echo off\r\necho batch-launcher-should-not-run\r\n";
    std::fs::write(&launcher, contents).unwrap();
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 71,
        artifact: ArtifactExpectation::windows_batch_launcher(
            Uuid::nil(),
            launcher.canonicalize().unwrap(),
            Sha256::digest(contents).into(),
            u64::try_from(contents.len()).unwrap(),
        )
        .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![OsString::from("app-server")]).unwrap(),
        environment: BoundedEnvironment::new(
            [
                "APPDATA",
                "HOME",
                "LOCALAPPDATA",
                "PATH",
                "SystemRoot",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "WINDIR",
            ]
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
            .collect(),
        )
        .unwrap(),
        project_root: test_project_root(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let failure =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .err()
            .expect("only the observed npm Codex launcher shape may derive node execution");
    assert_eq!(failure.error(), LaunchError::IdentityMismatch);
    assert!(failure.into_zero_process().is_ok());
    assert_eq!(pool.usage(), (0, 0));
    std::fs::remove_file(launcher).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_batch_launcher_rejects_npm_shim_drift_before_cmd_executes_it() {
    use sha2::{Digest, Sha256};

    let directory = std::env::temp_dir().join(format!("sage-npm-shim-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let launcher = directory.join("codex.cmd");
    let original = b"@echo off\r\necho original> \"%~dp0executed.txt\"\r\n";
    let replacement = b"@echo off\r\necho replacement> \"%~dp0executed.txt\"\r\n";
    std::fs::write(&launcher, original).unwrap();
    let mut spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 72,
        artifact: ArtifactExpectation::windows_batch_launcher(
            Uuid::nil(),
            launcher.canonicalize().unwrap(),
            Sha256::digest(original).into(),
            u64::try_from(original.len()).unwrap(),
        )
        .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![OsString::from("app-server")]).unwrap(),
        environment: BoundedEnvironment::new(
            [
                "APPDATA",
                "HOME",
                "LOCALAPPDATA",
                "PATH",
                "SystemRoot",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "WINDIR",
            ]
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
            .collect(),
        )
        .unwrap(),
        project_root: test_project_root(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let replacement_path = launcher.clone();
    spec.test_after_artifact_verified = Some(Box::new(move || {
        std::fs::write(&replacement_path, replacement).unwrap();
    }));
    let pool = TreeCapacityPool::isolated_ade_0006a();

    let failure =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .err()
            .expect("drifted batch launcher must not spawn");

    assert_eq!(failure.error(), LaunchError::IdentityMismatch);
    assert_eq!(failure.into_zero_process().unwrap().runtime_epoch(), 72);
    assert!(
        !directory.join("executed.txt").exists(),
        "cmd.exe must not execute a drifted npm launcher"
    );
    assert_eq!(pool.usage(), (0, 0));
    std::fs::remove_file(launcher).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_npm_codex_launcher_hands_node_ordinary_script_and_working_directory_paths() {
    use sha2::{Digest, Sha256};

    // Node 22.23 cannot load a main script named by a verbatim `\\?\` path,
    // and the launcher's canonical path is verbatim; the project root is too.
    let directory = std::env::temp_dir().join(format!("sage-npm-codex-{}", Uuid::new_v4()));
    let script_directory = directory
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin");
    std::fs::create_dir_all(&script_directory).unwrap();
    std::fs::write(
        script_directory.join("codex.js"),
        b"process.stdout.write([process.argv[1], process.argv[2], process.cwd()].join('|'));\n",
    )
    .unwrap();
    std::fs::write(
        script_directory.parent().unwrap().join("package.json"),
        br#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
    )
    .unwrap();
    let launcher = directory.join("codex.cmd");
    let contents = concat!(
        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n",
        ":start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n",
        "  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n",
        "  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
        "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  ",
        "\"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n"
    )
    .as_bytes();
    std::fs::write(&launcher, contents).unwrap();
    let canonical = launcher.canonicalize().unwrap();
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 73,
        artifact: ArtifactExpectation::windows_batch_launcher(
            Uuid::nil(),
            canonical.clone(),
            Sha256::digest(contents).into(),
            u64::try_from(contents.len()).unwrap(),
        )
        .unwrap()
        .with_manifest(Some(
            ManifestExpectation::record(
                script_directory
                    .parent()
                    .unwrap()
                    .join("package.json")
                    .canonicalize()
                    .unwrap(),
            )
            .unwrap(),
        )),
        fixed_args: BoundedOsArgs::new(vec![OsString::from("app-server")]).unwrap(),
        environment: BoundedEnvironment::new(
            ["PATH", "SystemRoot", "TEMP", "TMP", "WINDIR"]
                .into_iter()
                .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
                .collect(),
        )
        .unwrap(),
        project_root: test_project_root(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .expect("the npm Codex launcher starts node");
    let (mut stdin, mut stdout, control) = tree.into_parts();
    stdin.close();
    let mut output = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        let read = stdout.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..read]);
        assert!(output.len() <= 4096);
    }
    assert!(matches!(
        control.wait(),
        Ok(TreeExit::Exited { code: Some(0) })
    ));
    let script = sage_core::win32_path::simplified(
        &canonical
            .parent()
            .unwrap()
            .join(r"node_modules\@openai\codex\bin\codex.js"),
    );
    // Pi names its session folder after the working directory, and a
    // verbatim prefix there produced an illegal file name.
    let cwd = sage_core::win32_path::simplified(spec.project_root.as_path());
    assert!(!cwd.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("{}|app-server|{}", script.display(), cwd.display())
    );
    assert_eq!(pool.usage(), (0, 0));
    std::fs::remove_dir_all(directory).unwrap();
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

#[cfg(windows)]
#[test]
fn admitted_fixture_live_process_candidates_stay_alive_under_their_declared_environment() {
    let mut admitted_candidates = 0_u32;
    for candidate in fixture_live_process_candidates() {
        let Some(candidate) = candidate.canonicalized() else {
            continue;
        };
        let executable = candidate.executable.clone();
        match try_launch_fixture_live_process_for_test(9, candidate) {
            FixtureCandidateOutcome::Usable(tree) => {
                let (_, _, control) = tree.into_parts();
                std::thread::sleep(std::time::Duration::from_millis(1500));
                assert_eq!(
                    control.native_exit_code_if_exited().unwrap(),
                    None,
                    "{} was admitted but exited before the fixture could use it",
                    executable.display()
                );
                control.terminate_zero(TreeExit::Cancelled).unwrap();
                admitted_candidates += 1;
            }
            FixtureCandidateOutcome::RejectedWithZeroProcesses => {}
            FixtureCandidateOutcome::CleanupUnproven => panic!(
                "{} was rejected without proving its process tree reached zero",
                executable.display()
            ),
        }
    }
    assert!(admitted_candidates > 0);
}

#[cfg(windows)]
#[test]
fn fixture_live_process_selection_falls_back_past_unusable_candidates() {
    let system_root =
        std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from("C:\\Windows"));
    let system32 = PathBuf::from(&system_root).join("System32");
    let minimal_environment = vec![(OsString::from("SystemRoot"), system_root.clone())];
    let mut candidates = vec![
        FixtureLiveProcess {
            executable: system32.join("drivers").join("etc").join("hosts"),
            args: Vec::new(),
            environment: minimal_environment.clone(),
        },
        FixtureLiveProcess {
            executable: system32.join("cmd.exe"),
            args: vec![OsString::from("/c"), OsString::from("exit")],
            environment: minimal_environment.clone(),
        },
    ];
    candidates.extend(fixture_live_process_candidates());
    let tree = launch_first_usable_fixture_live_process_for_test(11, candidates)
        .expect("a usable live process candidate remains after the unusable ones");
    let (_, _, control) = tree.into_parts();
    assert_eq!(control.native_exit_code_if_exited().unwrap(), None);
    control.terminate_zero(TreeExit::Cancelled).unwrap();

    assert!(
        launch_first_usable_fixture_live_process_for_test(
            12,
            vec![FixtureLiveProcess {
                executable: system32.join("cmd.exe"),
                args: vec![OsString::from("/c"), OsString::from("exit")],
                environment: minimal_environment,
            }],
        )
        .is_none()
    );
}

#[cfg(windows)]
#[test]
fn fixture_candidate_selection_falls_back_only_after_proven_zero_processes() {
    let candidate = || FixtureLiveProcess {
        executable: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
        args: Vec::new(),
        environment: Vec::new(),
    };

    let mut attempts = 0_u32;
    let selected = select_first_usable_fixture_candidate(vec![candidate(), candidate()], |_| {
        attempts += 1;
        if attempts == 1 {
            FixtureCandidateOutcome::RejectedWithZeroProcesses
        } else {
            FixtureCandidateOutcome::Usable(7_u32)
        }
    });
    assert_eq!(selected, Some(7));
    assert_eq!(attempts, 2);

    let mut attempts = 0_u32;
    let selected =
        select_first_usable_fixture_candidate::<u32, _>(vec![candidate(), candidate()], |_| {
            attempts += 1;
            FixtureCandidateOutcome::CleanupUnproven
        });
    assert_eq!(selected, None);
    assert_eq!(attempts, 1);
}
