// P1 defines the private process contract before P2 connects the runtime.
// Keep the dormant private surface warning-clean without making it public.
#![allow(dead_code, unused_imports)]

mod capacity;
mod identity;
mod platform;
mod supervisor;
#[cfg(not(windows))]
mod unavailable;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::monotonic_tick;

pub(crate) use capacity::{TreeCapacityLease, TreeCapacityPool};
pub(crate) use identity::{ProcessIdentityReceipt, SpawnTimingReceipt};
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
pub(crate) fn launch_fixture_for_test(runtime_epoch: u64) -> RunningTree {
    use sha2::{Digest, Sha256};
    use std::{ffi::OsString, io::Read};
    use uuid::Uuid;

    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("acp-adversarial-fixture.exe")
        .canonicalize()
        .unwrap();
    let mut file = std::fs::File::open(&executable).unwrap();
    let byte_len = file.metadata().unwrap().len();
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).unwrap();
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
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch,
        artifact: ArtifactExpectation::new(
            Uuid::nil(),
            executable,
            digest.finalize().into(),
            byte_len,
        )
        .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![
            OsString::from("--case"),
            OsString::from("stale-project-generation-completion"),
            OsString::from("--seed"),
            OsString::from("30000"),
        ])
        .unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root,
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let pool = TreeCapacityPool::isolated_ade_0006a();
    PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()).unwrap()
}

#[cfg(test)]
mod tests;
