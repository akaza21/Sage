use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

use crate::acp::{process::TreeCapacityLease, redaction::StderrRing};

use super::{
    super::{
        platform::LaunchFailure,
        supervisor::{LaunchError, LaunchSpec, RequiredContainment},
    },
    RunningTree,
    containment::{TreeAuthority, TreeControl},
    reap::{drain_stderr, fail_after_spawn, fail_pre_spawn},
};

pub(super) struct ArtifactIdentity {
    pub(super) path: PathBuf,
    pub(super) sha256: [u8; 32],
    pub(super) byte_len: u64,
}

pub(crate) fn launch(
    spec: &LaunchSpec,
    mut lease: TreeCapacityLease,
) -> Result<RunningTree, LaunchFailure> {
    if spec.required_guarantee != RequiredContainment::TrustedCooperativeTree
        || !lease.matches(spec.limits.descendants)
    {
        return fail_pre_spawn(LaunchError::ContainmentFailure, lease, spec.runtime_epoch);
    }
    let identity = match verify_artifact(spec) {
        Ok(identity) => identity,
        Err(error) => return fail_pre_spawn(error, lease, spec.runtime_epoch),
    };
    if spec.project_root.revalidate().is_err() {
        return fail_pre_spawn(LaunchError::IdentityMismatch, lease, spec.runtime_epoch);
    }
    let mut command = Command::new(&identity.path);
    command
        .args(spec.fixed_args.as_slice())
        .current_dir(spec.project_root.as_path())
        .env_clear()
        .envs(spec.environment.as_slice().iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return fail_pre_spawn(LaunchError::Io, lease, spec.runtime_epoch),
    };
    let Some(stdin) = child.stdin.take() else {
        return fail_after_spawn(child, LaunchError::Io, lease, spec.runtime_epoch);
    };
    let Some(stdout) = child.stdout.take() else {
        return fail_after_spawn(child, LaunchError::Io, lease, spec.runtime_epoch);
    };
    let Some(stderr) = child.stderr.take() else {
        return fail_after_spawn(child, LaunchError::Io, lease, spec.runtime_epoch);
    };
    lease.activate();
    let (done, completion) = mpsc::sync_channel(1);
    let ring = Arc::new(Mutex::new(StderrRing::new(
        usize::try_from(sage_core::limits::LIMITS.process.stderr_ring_bytes).unwrap_or(usize::MAX),
        usize::try_from(sage_core::limits::LIMITS.process.stderr_line_bytes).unwrap_or(usize::MAX),
    )));
    drain_stderr(stderr, Arc::clone(&ring), done);
    let spawned_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    let control = TreeControl::new(TreeAuthority {
        child: Mutex::new(child),
        requested_exit: Mutex::new(None),
        lease: Mutex::new(Some(lease)),
        stderr: ring,
        stderr_done: Mutex::new(completion),
        converged: Mutex::new(false),
        runtime_epoch: spec.runtime_epoch,
        terminate_deadline: Duration::from_millis(spec.deadlines.terminate_ms),
        artifact: identity,
        spawned_millis,
    });
    Ok(RunningTree::new(stdin, stdout, control))
}

fn verify_artifact(spec: &LaunchSpec) -> Result<ArtifactIdentity, LaunchError> {
    let path = std::fs::canonicalize(&spec.artifact.canonical_absolute_path)
        .map_err(|_| LaunchError::IdentityMismatch)?;
    if path != spec.artifact.canonical_absolute_path {
        return Err(LaunchError::IdentityMismatch);
    }
    let mut file = File::open(&path).map_err(|_| LaunchError::IdentityMismatch)?;
    let bytes = file
        .metadata()
        .map_err(|_| LaunchError::IdentityMismatch)?
        .len();
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| LaunchError::IdentityMismatch)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    if bytes != spec.artifact.byte_len || hasher.finalize().as_slice() != spec.artifact.sha256 {
        return Err(LaunchError::IdentityMismatch);
    }
    Ok(ArtifactIdentity {
        path,
        sha256: spec.artifact.sha256,
        byte_len: bytes,
    })
}
