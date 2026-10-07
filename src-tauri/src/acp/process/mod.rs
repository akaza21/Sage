// P1 defines the private process contract before P2 connects the runtime.
// Keep the dormant private surface warning-clean without making it public.
#![allow(dead_code, unused_imports)]

mod capacity;
mod identity;
mod manifest;
mod platform;
mod supervisor;
#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::TerminalJobBoundary;
#[cfg(windows)]
pub(crate) use windows::monotonic_tick;
#[cfg(windows)]
pub(crate) use windows::{
    PreviewJobBoundary, PreviewLaunchFailure, PreviewLaunchRecovery, PreviewWindowsProcess,
};

#[cfg(all(test, not(windows)))]
pub(crate) use unix::launch_fixture_for_test;

pub(crate) use capacity::{TreeCapacityLease, TreeCapacityPool};
pub(crate) use identity::{ProcessIdentityReceipt, SpawnTimingReceipt};
pub(crate) use manifest::{ManifestExpectation, ManifestGuard};
pub(crate) use platform::{
    LaunchFailure, LaunchRecovery, PlatformSupervisor, RunningTree, StdinEndpoint, StdoutEndpoint,
    TreeControl,
};
pub(crate) use supervisor::ZeroProcessToken;
pub(crate) use supervisor::{
    ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, EntryPointKind, LaunchError,
    LaunchSpec, ProcessDeadlines, RequiredContainment, TreeLimits,
};
pub(crate) use supervisor::{SupervisorLifecycle, TeardownAction, TreeExit, TreeState};

#[cfg(all(test, windows))]
struct FixtureLiveProcess {
    executable: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

#[cfg(all(test, windows))]
const FIXTURE_LIVE_PROCESS_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(all(test, windows))]
enum FixtureCandidateOutcome<T> {
    Usable(T),
    RejectedWithZeroProcesses,
    CleanupUnproven,
}

#[cfg(all(test, windows))]
impl FixtureLiveProcess {
    fn canonicalized(self) -> Option<Self> {
        let executable = self.executable.canonicalize().ok()?;
        Some(Self {
            executable,
            args: self.args,
            environment: self.environment,
        })
    }
}

#[cfg(all(test, windows))]
fn fixture_live_process_candidates() -> Vec<FixtureLiveProcess> {
    use std::ffi::OsString;

    let system_root =
        std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from("C:\\Windows"));
    let system32 = std::path::Path::new(&system_root).join("System32");
    let minimal_environment = vec![(OsString::from("SystemRoot"), system_root.clone())];
    let unique_signal = format!("SageAcpProcessFixture{}", uuid::Uuid::new_v4().as_simple());
    vec![
        FixtureLiveProcess {
            executable: system32.join("waitfor.exe"),
            args: vec![
                OsString::from("/t"),
                OsString::from("30"),
                OsString::from(unique_signal),
            ],
            environment: minimal_environment.clone(),
        },
        FixtureLiveProcess {
            executable: system32.join("ping.exe"),
            args: vec![
                OsString::from("-n"),
                OsString::from("31"),
                OsString::from("-w"),
                OsString::from("1000"),
                OsString::from("127.0.0.1"),
            ],
            environment: minimal_environment,
        },
    ]
}

#[cfg(all(test, windows))]
pub(crate) fn launch_fixture_for_test(runtime_epoch: u64) -> RunningTree {
    launch_first_usable_fixture_live_process_for_test(
        runtime_epoch,
        fixture_live_process_candidates(),
    )
    .expect(
        "a windows system executable stays alive for the live process fixture after proven cleanup",
    )
}

#[cfg(all(test, windows))]
fn launch_first_usable_fixture_live_process_for_test(
    runtime_epoch: u64,
    candidates: Vec<FixtureLiveProcess>,
) -> Option<RunningTree> {
    select_first_usable_fixture_candidate(
        candidates
            .into_iter()
            .filter_map(FixtureLiveProcess::canonicalized)
            .collect(),
        |candidate| try_launch_fixture_live_process_for_test(runtime_epoch, candidate),
    )
}

#[cfg(all(test, windows))]
fn select_first_usable_fixture_candidate<T, L>(
    candidates: Vec<FixtureLiveProcess>,
    mut launch: L,
) -> Option<T>
where
    L: FnMut(FixtureLiveProcess) -> FixtureCandidateOutcome<T>,
{
    for candidate in candidates {
        match launch(candidate) {
            FixtureCandidateOutcome::Usable(usable) => return Some(usable),
            FixtureCandidateOutcome::RejectedWithZeroProcesses => continue,
            FixtureCandidateOutcome::CleanupUnproven => return None,
        }
    }
    None
}

#[cfg(all(test, windows))]
fn try_launch_fixture_live_process_for_test(
    runtime_epoch: u64,
    candidate: FixtureLiveProcess,
) -> FixtureCandidateOutcome<RunningTree> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use uuid::Uuid;

    let FixtureLiveProcess {
        executable,
        args,
        environment,
    } = candidate;
    let Ok(mut file) = std::fs::File::open(&executable) else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let Ok(metadata) = file.metadata() else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let byte_len = metadata.len();
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let Ok(read) = file.read(&mut chunk) else {
            return FixtureCandidateOutcome::RejectedWithZeroProcesses;
        };
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    let project_id = crate::acp::contract::SageProjectId::parse("project-process-fixture").unwrap();
    let project_root = crate::project_registry::ProjectRootAuthority::for_test(
        project_id,
        crate::acp::contract::ProjectGeneration::ZERO,
        crate::acp::contract::SageRootId::parse("root-process-fixture").unwrap(),
        &std::env::current_dir().unwrap(),
    )
    .unwrap();
    let Ok(artifact) =
        ArtifactExpectation::new(Uuid::nil(), executable, digest.finalize().into(), byte_len)
    else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let Ok(fixed_args) = BoundedOsArgs::new(args) else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let Ok(environment) = BoundedEnvironment::new(environment) else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch,
        artifact,
        fixed_args,
        environment,
        project_root,
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let Ok(lease) = pool.try_acquire(spec.limits.descendants) else {
        return FixtureCandidateOutcome::RejectedWithZeroProcesses;
    };
    let tree = match PlatformSupervisor::launch(&spec, lease) {
        Ok(tree) => tree,
        Err(failure) => return fixture_launch_failure_outcome(failure),
    };
    let control = tree.control_for_test();
    std::thread::sleep(FIXTURE_LIVE_PROCESS_SETTLE);
    match control.native_exit_code_if_exited() {
        Ok(None) => FixtureCandidateOutcome::Usable(tree),
        _ => match control.terminate_zero(TreeExit::Cancelled) {
            Ok(_) => FixtureCandidateOutcome::RejectedWithZeroProcesses,
            Err(_) => FixtureCandidateOutcome::CleanupUnproven,
        },
    }
}

#[cfg(all(test, windows))]
fn fixture_launch_failure_outcome<T>(failure: LaunchFailure) -> FixtureCandidateOutcome<T> {
    match failure.into_zero_process() {
        Ok(_) => FixtureCandidateOutcome::RejectedWithZeroProcesses,
        Err(failure) => match failure.into_recovery() {
            Ok((_, recovery)) => match recovery.retry_zero() {
                Ok(_) => FixtureCandidateOutcome::RejectedWithZeroProcesses,
                Err(_) => FixtureCandidateOutcome::CleanupUnproven,
            },
            Err(_) => FixtureCandidateOutcome::CleanupUnproven,
        },
    }
}

#[cfg(all(test, windows))]
pub(crate) use windows::retained_preview_recovery_with_retry_fault_for_test;

#[cfg(test)]
mod tests;
