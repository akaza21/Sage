use super::TreeCapacityLease;
use super::platform::LaunchFailure;
use super::supervisor::{
    LaunchError, LaunchFailureTracker, LaunchSpec, TreeExit, ZeroProcessToken,
};
use crate::acp::{
    process::ProcessIdentityReceipt,
    redaction::SafeDiagnostic,
    transport::{Frame, FrameError, FrameWriteOutcome, OutboundFrame},
};

/// Placeholder authority for platforms that have not proved an enforced tree.
/// It cannot be constructed through the product launch path.
pub(crate) struct RunningTree;
#[must_use = "dropping unresolved launch recovery keeps capacity fail-closed"]
pub(crate) struct LaunchRecovery {
    lease: Option<TreeCapacityLease>,
    tracker: LaunchFailureTracker,
}
pub(crate) struct StdinEndpoint;
pub(crate) struct StdoutEndpoint;
#[derive(Clone)]
pub(crate) struct TreeControl;

impl RunningTree {
    pub(crate) fn into_parts(self) -> (StdinEndpoint, StdoutEndpoint, TreeControl) {
        unreachable!("unsupported platforms cannot construct a running ACP tree")
    }
}

impl LaunchRecovery {
    fn pre_spawn(lease: TreeCapacityLease, runtime_epoch: u64) -> Self {
        Self {
            lease: Some(lease),
            tracker: LaunchFailureTracker::pre_spawn(runtime_epoch),
        }
    }

    pub(crate) fn retry_zero(mut self) -> Result<ZeroProcessToken, Box<Self>> {
        let released = match self.lease.as_mut() {
            Some(lease) => lease.release_prelaunch_after_zero().is_ok(),
            None => false,
        };
        if released {
            self.lease.take();
        }
        let token = released
            .then(|| self.tracker.finish_zero(Ok(())).ok())
            .flatten();
        match token {
            Some(token) => Ok(token),
            None => Err(Box::new(self)),
        }
    }
}

impl StdinEndpoint {
    pub(crate) fn write_frame(&mut self, _frame: &OutboundFrame) -> FrameWriteOutcome {
        FrameWriteOutcome::ZeroByteFailure
    }

    pub(crate) fn close(&mut self) {}
}

impl StdoutEndpoint {
    pub(crate) fn read(&mut self, _target: &mut [u8]) -> Result<usize, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        Err(FrameError::Io)
    }
}

impl TreeControl {
    pub(crate) fn process_identity_receipt(&self) -> Result<ProcessIdentityReceipt, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn request_termination(&self, _reason: TreeExit) -> Result<(), LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn wait(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn wait_before(
        &self,
        _deadline: std::time::Instant,
    ) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn terminate(&self, _reason: TreeExit) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn wait_zero(&self) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn terminate_zero(
        &self,
        _reason: TreeExit,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    pub(crate) fn stderr_summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }
}

pub(crate) fn launch(
    spec: &LaunchSpec,
    lease: TreeCapacityLease,
) -> Result<RunningTree, LaunchFailure> {
    // Ordering is security-sensitive: an unsupported platform fails before
    // inspecting either native path or touching process/session state.
    let _project_scope = spec.project_root.scope();
    let recovery = LaunchRecovery::pre_spawn(lease, spec.runtime_epoch);
    match recovery.retry_zero() {
        Ok(token) => Err(LaunchFailure::zero_process(
            LaunchError::PlatformUnavailable,
            token,
        )),
        Err(recovery) => Err(LaunchFailure::convergence_unknown(
            LaunchError::ContainmentFailure,
            *recovery,
        )),
    }
}
