use std::fs::File;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::{ResumeThread, TerminateProcess};

use super::artifact::VerifiedArtifact;
#[cfg(feature = "acp-fixture")]
use super::authority::CharacterizationObservationState;
use super::authority::{RunningTree, TreeAuthority};
use super::io::{StderrDrain, StdinEndpoint, StdoutEndpoint};
use super::job::JobBoundary;
use super::native::NativeHandle;
use super::{LaunchError, LaunchSpec, TreeCapacityLease, wait_until};
use crate::acp::process::SpawnTimingReceipt;
use crate::acp::process::platform::LaunchFailure;
use crate::acp::process::supervisor::{LaunchFailureTracker, ZeroProcessToken};

#[must_use = "dropping unresolved launch recovery keeps capacity fail-closed"]
pub(crate) struct LaunchRecovery {
    state: RecoveryState,
}

enum RecoveryState {
    PreSpawn {
        lease: TreeCapacityLease,
        tracker: LaunchFailureTracker,
    },
    Started(Box<StartedRecovery>),
}

struct StartedRecovery {
    process: Option<NativeHandle>,
    thread: Option<NativeHandle>,
    job: JobBoundary,
    artifact: Option<VerifiedArtifact>,
    stderr: Option<StderrDrain>,
    lease: Option<TreeCapacityLease>,
    active_process_limit: u32,
    deadline: Duration,
    absolute_cleanup_deadline: Option<Instant>,
    assigned_to_job: bool,
    tracker: LaunchFailureTracker,
    spawn_timing: Option<SpawnTimingReceipt>,
}

impl LaunchRecovery {
    fn pre_spawn(lease: TreeCapacityLease, runtime_epoch: u64) -> Self {
        Self {
            state: RecoveryState::PreSpawn {
                lease,
                tracker: LaunchFailureTracker::pre_spawn(runtime_epoch),
            },
        }
    }

    pub(super) fn child_created(
        process: Option<NativeHandle>,
        thread: Option<NativeHandle>,
        job: JobBoundary,
        artifact: VerifiedArtifact,
        lease: TreeCapacityLease,
        spawn_timing: Option<SpawnTimingReceipt>,
        spec: &LaunchSpec,
    ) -> Self {
        Self {
            state: RecoveryState::Started(Box::new(StartedRecovery {
                process,
                thread,
                job,
                artifact: Some(artifact),
                stderr: None,
                lease: Some(lease),
                active_process_limit: spec.limits.active_processes,
                deadline: Duration::from_millis(spec.deadlines.spawn_ms),
                absolute_cleanup_deadline: spec.absolute_cleanup_deadline,
                assigned_to_job: false,
                tracker: LaunchFailureTracker::child_created(spec.runtime_epoch),
                spawn_timing,
            })),
        }
    }

    pub(crate) fn retry_zero(mut self) -> Result<ZeroProcessToken, Box<Self>> {
        let token = match &mut self.state {
            RecoveryState::PreSpawn { lease, tracker } => lease
                .release_prelaunch_after_zero()
                .and_then(|()| tracker.finish_zero(Ok(())))
                .ok(),
            RecoveryState::Started(started) => started
                .try_zero()
                .and_then(|()| started.tracker.finish_zero(Ok(())))
                .ok(),
        };
        match token {
            Some(token) => Ok(token),
            None => Err(Box::new(self)),
        }
    }

    pub(super) fn assign_job(&mut self) -> Result<(), LaunchError> {
        let RecoveryState::Started(started) = &mut self.state else {
            return Err(LaunchError::ContainmentFailure);
        };
        started.job.assign(
            started
                .process
                .as_ref()
                .ok_or(LaunchError::ContainmentFailure)?
                .raw(),
        )?;
        started.assigned_to_job = true;
        Ok(())
    }

    pub(super) fn verify_artifact(&self, spec: &LaunchSpec) -> Result<(), LaunchError> {
        let RecoveryState::Started(started) = &self.state else {
            return Err(LaunchError::ContainmentFailure);
        };
        started
            .artifact
            .as_ref()
            .ok_or(LaunchError::ContainmentFailure)?
            .matches_running_image(
                started
                    .process
                    .as_ref()
                    .ok_or(LaunchError::ContainmentFailure)?
                    .raw(),
                spec,
            )
    }

    pub(super) fn start_stderr(&mut self, stderr: File) -> Result<(), LaunchError> {
        let RecoveryState::Started(started) = &mut self.state else {
            return Err(LaunchError::ContainmentFailure);
        };
        started.stderr = Some(StderrDrain::spawn(stderr)?);
        Ok(())
    }

    pub(super) fn resume(&self) -> Result<(), LaunchError> {
        let RecoveryState::Started(started) = &self.state else {
            return Err(LaunchError::ContainmentFailure);
        };
        let thread = started
            .thread
            .as_ref()
            .ok_or(LaunchError::ContainmentFailure)?;
        // SAFETY: the retained primary thread was created suspended.
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(())
    }

    pub(super) fn use_termination_deadline(&mut self, spec: &LaunchSpec) {
        if let RecoveryState::Started(started) = &mut self.state {
            started.deadline = Duration::from_millis(spec.deadlines.terminate_ms);
        }
    }

    pub(super) fn into_running(
        self,
        parent_stdin: File,
        parent_stdout: File,
    ) -> Result<RunningTree, LaunchFailure> {
        let started = match self.state {
            RecoveryState::Started(started) => *started,
            state @ RecoveryState::PreSpawn { .. } => {
                return Err(LaunchFailure::convergence_unknown(
                    LaunchError::ContainmentFailure,
                    Self { state },
                ));
            }
        };
        let absolute_cleanup_deadline = started.absolute_cleanup_deadline;
        let (
            process,
            thread,
            job,
            artifact,
            stderr,
            lease,
            active_process_limit,
            deadline,
            tracker,
            spawn_timing,
        ) = match started {
            StartedRecovery {
                process: Some(process),
                thread: Some(thread),
                job,
                artifact: Some(artifact),
                stderr: Some(stderr),
                lease: Some(lease),
                active_process_limit,
                deadline,
                assigned_to_job: true,
                tracker,
                spawn_timing: Some(spawn_timing),
                ..
            } => (
                process,
                thread,
                job,
                artifact,
                stderr,
                lease,
                active_process_limit,
                deadline,
                tracker,
                spawn_timing,
            ),
            started => {
                return Err(LaunchFailure::convergence_unknown(
                    LaunchError::ContainmentFailure,
                    Self {
                        state: RecoveryState::Started(Box::new(started)),
                    },
                ));
            }
        };
        let authority = TreeAuthority {
            process,
            thread,
            job,
            artifact,
            stderr,
            lease: Mutex::new(Some(lease)),
            active_process_limit,
            terminate_deadline: deadline,
            absolute_cleanup_deadline,
            requested_exit: Mutex::new(None),
            convergence: Mutex::new(false),
            #[cfg(feature = "acp-fixture")]
            characterization_observation: Mutex::new(CharacterizationObservationState::default()),
            runtime_epoch: tracker.runtime_epoch(),
            spawn_timing,
        };
        Ok(RunningTree::new(
            StdinEndpoint::new(parent_stdin),
            StdoutEndpoint::new(parent_stdout),
            authority,
        ))
    }
}

impl StartedRecovery {
    fn try_zero(&mut self) -> Result<(), LaunchError> {
        let process = self
            .process
            .as_ref()
            .ok_or(LaunchError::ContainmentFailure)?;
        if self.assigned_to_job {
            let _ = self.job.terminate();
        }
        // SAFETY: the retained root handle is valid. This fallback also covers
        // failures that occur before successful Job assignment.
        unsafe { TerminateProcess(process.raw(), 1) };
        let deadline = {
            let native_deadline = Instant::now() + self.deadline;
            self.absolute_cleanup_deadline
                .map_or(native_deadline, |absolute| native_deadline.min(absolute))
        };
        wait_until(process.raw(), deadline)?;
        if self.assigned_to_job {
            self.job
                .wait_for_zero(self.active_process_limit, deadline)?;
        }
        if let Some(stderr) = self.stderr.as_ref() {
            stderr.wait_until(deadline)?;
        }
        self.lease
            .as_mut()
            .ok_or(LaunchError::ContainmentFailure)?
            .release_active_after_zero()?;
        self.lease.take();
        Ok(())
    }
}

pub(super) fn fail_pre_spawn(
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

pub(super) fn classify_started_failure(
    error: LaunchError,
    recovery: LaunchRecovery,
) -> LaunchFailure {
    match recovery.retry_zero() {
        Ok(token) => LaunchFailure::zero_process(error, token),
        Err(recovery) => LaunchFailure::convergence_unknown(error, *recovery),
    }
}
