//! Cooperative WSL process ownership for the admitted local Codex CLI.
//!
//! Windows retains the production Job-object tree collector.  This bounded
//! beta path deliberately owns only the direct app-server process, and is
//! represented as `TrustedCooperativeTree` in the launch contract.

use std::{
    fs::File,
    io::{Read, Write},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

use crate::acp::{
    process::{ProcessIdentityReceipt, TreeCapacityLease},
    redaction::{SafeDiagnostic, SafeStderrDiagnostic, StderrRing},
    transport::{
        Frame, FrameError, FrameWriteOutcome, IncrementalFrameDecoder, OutboundFrame,
        write_json_line_blocking,
    },
};

use super::{
    platform::LaunchFailure,
    supervisor::{
        LaunchError, LaunchFailureTracker, LaunchSpec, RequiredContainment, TreeExit,
        ZeroProcessToken,
    },
};

pub(crate) struct StdinEndpoint(Option<ChildStdin>);

impl StdinEndpoint {
    pub(crate) fn write_frame(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .as_mut()
            .map_or(FrameWriteOutcome::ZeroByteFailure, |stdin| {
                write_json_line_blocking(stdin, frame)
            })
    }

    pub(crate) fn close(&mut self) {
        self.0.take();
    }
}

pub(crate) struct StdoutEndpoint {
    stdout: ChildStdout,
    decoder: IncrementalFrameDecoder,
}

impl StdoutEndpoint {
    pub(crate) fn read(&mut self, target: &mut [u8]) -> Result<usize, LaunchError> {
        if target.len() as u64 > crate::acp::limits::LIMITS.wire.frame_bytes {
            return Err(LaunchError::StdioLimit);
        }
        self.stdout.read(target).map_err(|_| LaunchError::Io)
    }

    pub(crate) fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            if let Some(frame) = self.decoder.drain_pending()? {
                return Ok(Some(frame));
            }
            let read = self.stdout.read(&mut chunk).map_err(|_| FrameError::Io)?;
            if read == 0 {
                return self.decoder.finish();
            }
            if let Some(frame) = self.decoder.push_chunk(&chunk[..read])? {
                return Ok(Some(frame));
            }
        }
    }
}

pub(crate) struct RunningTree {
    stdin: StdinEndpoint,
    stdout: StdoutEndpoint,
    control: TreeControl,
}

impl RunningTree {
    pub(crate) fn into_parts(self) -> (StdinEndpoint, StdoutEndpoint, TreeControl) {
        (self.stdin, self.stdout, self.control)
    }
}

#[derive(Clone)]
pub(crate) struct TreeControl(Arc<TreeAuthority>);

struct TreeAuthority {
    child: Mutex<Child>,
    requested_exit: Mutex<Option<TreeExit>>,
    lease: Mutex<Option<TreeCapacityLease>>,
    stderr: Arc<Mutex<StderrRing>>,
    stderr_done: Mutex<mpsc::Receiver<()>>,
    converged: Mutex<bool>,
    runtime_epoch: u64,
    terminate_deadline: Duration,
    artifact: ArtifactIdentity,
    spawned_millis: u64,
}

struct ArtifactIdentity {
    path: std::path::PathBuf,
    sha256: [u8; 32],
    byte_len: u64,
}

impl TreeControl {
    pub(crate) fn log_wait_handle_observation(&self, checkpoint: &'static str) {
        tracing::info!(
            checkpoint,
            pid = self.pid(),
            "WSL Codex root-process watcher attached"
        );
    }

    pub(crate) fn process_identity_receipt(&self) -> Result<ProcessIdentityReceipt, LaunchError> {
        Ok(ProcessIdentityReceipt::new(
            self.0.runtime_epoch,
            self.pid(),
            self.0.spawned_millis.saturating_mul(10_000),
            self.0.artifact.sha256,
            self.0.artifact.byte_len,
            self.0.artifact.path.clone(),
        ))
    }

    pub(crate) fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        self.0
            .requested_exit
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .get_or_insert(reason);
        let mut child = self
            .0
            .child
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        if child.try_wait().map_err(|_| LaunchError::Io)?.is_none() {
            child.kill().map_err(|_| LaunchError::Io)?;
        }
        Ok(())
    }

    pub(crate) fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_before(Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_before(Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn wait(&self) -> Result<TreeExit, LaunchError> {
        self.wait_root_exit()
    }

    pub(crate) fn wait_before(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        let status = loop {
            let mut child = self
                .0
                .child
                .lock()
                .map_err(|_| LaunchError::ContainmentFailure)?;
            if let Some(status) = child.try_wait().map_err(|_| LaunchError::Io)? {
                break status;
            }
            drop(child);
            if Instant::now() >= deadline {
                return Err(LaunchError::Deadline);
            }
            thread::sleep(Duration::from_millis(10));
        };
        self.converge(deadline)?;
        Ok(self.requested_exit().unwrap_or(TreeExit::Exited {
            code: status.code(),
        }))
    }

    pub(crate) fn terminate(&self, reason: TreeExit) -> Result<TreeExit, LaunchError> {
        self.request_termination(reason)?;
        self.wait_root_exit()
    }

    pub(crate) fn wait_zero(&self) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let exit = self.wait_root_exit()?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    pub(crate) fn wait_zero_before(
        &self,
        deadline: Instant,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let exit = self.wait_before(deadline.min(Instant::now() + self.0.terminate_deadline))?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    pub(crate) fn terminate_zero(
        &self,
        reason: TreeExit,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        self.terminate_zero_before(reason, Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn terminate_zero_before(
        &self,
        reason: TreeExit,
        deadline: Instant,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        self.request_termination(reason)?;
        self.wait_zero_before(deadline)
    }

    pub(crate) fn stderr_summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        self.0
            .stderr
            .lock()
            .map(|ring| ring.safe_summary())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(crate) fn stderr_diagnostic(&self) -> Result<SafeStderrDiagnostic, LaunchError> {
        self.0
            .stderr
            .lock()
            .map(|ring| ring.safe_diagnostic())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(crate) fn native_exit_code_if_exited(&self) -> Result<Option<i32>, LaunchError> {
        self.0
            .child
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .try_wait()
            .map(|status| status.and_then(|value| value.code()))
            .map_err(|_| LaunchError::Io)
    }

    fn pid(&self) -> u32 {
        self.0
            .child
            .lock()
            .map(|child| child.id())
            .unwrap_or_default()
    }

    fn requested_exit(&self) -> Option<TreeExit> {
        self.0.requested_exit.lock().ok().and_then(|exit| *exit)
    }

    fn converge(&self, deadline: Instant) -> Result<(), LaunchError> {
        let mut converged = self
            .0
            .converged
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        if *converged {
            return Ok(());
        }
        let receiver = self
            .0
            .stderr_done
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => LaunchError::Deadline,
                mpsc::RecvTimeoutError::Disconnected => LaunchError::ContainmentFailure,
            })?;
        self.0
            .lease
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .take()
            .ok_or(LaunchError::ContainmentFailure)?
            .release_after_zero()?;
        *converged = true;
        Ok(())
    }
}

impl Drop for TreeAuthority {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
        }
    }
}

#[must_use = "dropping unresolved launch recovery keeps capacity fail-closed"]
pub(crate) struct LaunchRecovery {
    lease: Option<TreeCapacityLease>,
    tracker: LaunchFailureTracker,
}

impl LaunchRecovery {
    fn pre_spawn(lease: TreeCapacityLease, runtime_epoch: u64) -> Self {
        Self {
            lease: Some(lease),
            tracker: LaunchFailureTracker::pre_spawn(runtime_epoch),
        }
    }
    pub(crate) fn retry_zero(mut self) -> Result<ZeroProcessToken, Box<Self>> {
        let released = self
            .lease
            .as_mut()
            .is_some_and(|lease| lease.release_prelaunch_after_zero().is_ok());
        if released {
            self.lease.take();
        }
        released
            .then(|| self.tracker.finish_zero(Ok(())).ok())
            .flatten()
            .ok_or_else(|| Box::new(self))
    }
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
        usize::try_from(crate::acp::limits::LIMITS.process.stderr_ring_bytes).unwrap_or(usize::MAX),
        usize::try_from(crate::acp::limits::LIMITS.process.stderr_line_bytes).unwrap_or(usize::MAX),
    )));
    drain_stderr(stderr, Arc::clone(&ring), done);
    let spawned_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    Ok(RunningTree {
        stdin: StdinEndpoint(Some(stdin)),
        stdout: StdoutEndpoint {
            stdout,
            decoder: IncrementalFrameDecoder::new(),
        },
        control: TreeControl(Arc::new(TreeAuthority {
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
        })),
    })
}

fn drain_stderr(mut stderr: ChildStderr, ring: Arc<Mutex<StderrRing>>, done: mpsc::SyncSender<()>) {
    thread::spawn(move || {
        let mut chunk = [0_u8; 8 * 1024];
        while let Ok(read) = stderr.read(&mut chunk) {
            if read == 0 {
                break;
            }
            if let Ok(mut ring) = ring.lock() {
                ring.ingest(&chunk[..read]);
            } else {
                break;
            }
        }
        let _ = done.send(());
    });
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

fn fail_pre_spawn(
    error: LaunchError,
    lease: TreeCapacityLease,
    runtime_epoch: u64,
) -> Result<RunningTree, LaunchFailure> {
    let recovery = LaunchRecovery::pre_spawn(lease, runtime_epoch);
    match recovery.retry_zero() {
        Ok(token) => Err(LaunchFailure::zero_process(error, token)),
        Err(recovery) => Err(LaunchFailure::convergence_unknown(
            LaunchError::ContainmentFailure,
            *recovery,
        )),
    }
}

fn fail_after_spawn(
    mut child: Child,
    error: LaunchError,
    lease: TreeCapacityLease,
    runtime_epoch: u64,
) -> Result<RunningTree, LaunchFailure> {
    if child.kill().is_err() || child.wait().is_err() {
        return Err(LaunchFailure::convergence_unknown(
            error,
            LaunchRecovery::pre_spawn(lease, runtime_epoch),
        ));
    }
    fail_pre_spawn(error, lease, runtime_epoch)
}

#[cfg(test)]
pub(crate) fn launch_fixture_for_test(runtime_epoch: u64) -> RunningTree {
    use std::{ffi::OsString, fs::File, io::Read};

    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    let executable = std::fs::canonicalize("/bin/sh").unwrap();
    let mut file = File::open(&executable).unwrap();
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
    let project_root = crate::project_registry::ProjectRootAuthority::for_test(
        crate::acp::contract::SageProjectId::parse("project-process-fixture").unwrap(),
        crate::acp::contract::ProjectGeneration::ZERO,
        crate::acp::contract::SageRootId::parse("root-process-fixture").unwrap(),
        &std::env::current_dir().unwrap(),
    )
    .unwrap();
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch,
        artifact: crate::acp::process::ArtifactExpectation::new(
            Uuid::nil(),
            executable,
            digest.finalize().into(),
            byte_len,
        )
        .unwrap(),
        fixed_args: crate::acp::process::BoundedOsArgs::new(vec![
            OsString::from("-c"),
            OsString::from("exec sleep 30"),
        ])
        .unwrap(),
        environment: crate::acp::process::BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root,
        required_guarantee: RequiredContainment::TrustedCooperativeTree,
        limits: crate::acp::process::TreeLimits::ade_0006a(),
        deadlines: crate::acp::process::ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
    };
    let pool = crate::acp::process::TreeCapacityPool::isolated_ade_0006a();
    launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()).unwrap()
}

#[cfg(test)]
#[path = "unix/tests.rs"]
mod tests;
