use std::{
    io::Read,
    process::{Child, ChildStderr},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use crate::acp::{process::TreeCapacityLease, redaction::StderrRing};
use sage_process::error::LaunchError;

use super::{
    super::{
        platform::LaunchFailure,
        supervisor::{LaunchFailureTracker, TreeExit, ZeroProcessToken},
    },
    RunningTree,
    containment::TreeControl,
};

impl TreeControl {
    pub(crate) fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_before(Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_before(Instant::now() + self.0.terminate_deadline)
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

    pub(crate) fn terminate_zero_before(
        &self,
        reason: TreeExit,
        deadline: Instant,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        self.request_termination(reason)?;
        self.wait_zero_before(deadline)
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

pub(super) fn drain_stderr(
    mut stderr: ChildStderr,
    ring: Arc<Mutex<StderrRing>>,
    done: mpsc::SyncSender<()>,
) {
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

pub(super) fn fail_after_spawn(
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
