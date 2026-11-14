use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::*;
use crate::{
    acp::{
        contract::{ProjectGeneration, SageProjectId, SageRootId},
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchError, LaunchSpec,
            ProcessDeadlines, RequiredContainment, TreeCapacityPool, TreeExit, TreeLimits,
        },
    },
    project_registry::ProjectRootAuthority,
};

fn shell_spec(script: &str) -> LaunchSpec {
    let path = std::fs::canonicalize("/bin/sh").unwrap();
    let mut file = File::open(&path).unwrap();
    let bytes = file.metadata().unwrap().len();
    let mut hash = Sha256::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = file.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 7,
        artifact: ArtifactExpectation::new(Uuid::nil(), path, hash.finalize().into(), bytes)
            .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![OsString::from("-c"), OsString::from(script)]).unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root: ProjectRootAuthority::for_test(
            SageProjectId::parse("project-unix-process").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-unix-process").unwrap(),
            &std::env::current_dir().unwrap(),
        )
        .unwrap(),
        required_guarantee: RequiredContainment::TrustedCooperativeTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
    }
}

#[test]
fn cooperative_supervisor_streams_one_frame_and_releases_its_lease_at_root_exit() {
    let spec = shell_spec("printf '{\"id\":1}\\n'");
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree = launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()).unwrap();
    let (_, mut stdout, control) = tree.into_parts();
    assert_eq!(
        stdout.read_frame().unwrap().unwrap().as_bytes(),
        br#"{"id":1}"#
    );
    assert!(stdout.read_frame().unwrap().is_none());
    assert!(matches!(
        control.wait_before(Instant::now() + Duration::from_secs(1)),
        Ok(TreeExit::Exited { code: Some(0) })
    ));
    assert_eq!(pool.usage(), (0, 0));
}

#[test]
fn cooperative_supervisor_terminates_the_admitted_root_before_releasing_capacity() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    // Keep the sleeper in the admitted root PID: a shell child can inherit
    // stderr and hold its drain open after the shell itself has been killed.
    let spec = shell_spec("exec sleep 5");
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree = launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()).unwrap();
    let (_, _, control) = tree.into_parts();
    assert_eq!(pool.usage(), (1, spec.limits.descendants));
    assert!(matches!(
        control.terminate_zero(TreeExit::Cancelled),
        Ok((TreeExit::Cancelled, _))
    ));
    assert_eq!(pool.usage(), (0, 0));
}

#[test]
fn cooperative_supervisor_rejects_artifact_drift_before_spawn_and_releases_capacity() {
    let mut spec = shell_spec("exit 0");
    spec.artifact.sha256[0] ^= 0xff;
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let failure = match launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()) {
        Ok(_) => panic!("artifact drift must fail before spawn"),
        Err(failure) => failure,
    };

    assert_eq!(failure.error(), LaunchError::IdentityMismatch);
    assert!(failure.into_zero_process().is_ok());
    assert_eq!(pool.usage(), (0, 0));
}
