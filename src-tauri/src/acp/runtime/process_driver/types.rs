use std::time::{SystemTime, UNIX_EPOCH};

use crate::acp::{
    process::{StdinEndpoint, TreeControl, TreeExit, ZeroProcessToken},
    runtime::{EgressTransport, NonfatalRefusal, SessionTreeLease},
    scheduler::RuntimeOwner,
    transport::{FrameWriteOutcome, OutboundFrame},
};
use sage_process::error::LaunchError;

use super::MailboxError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchSource {
    Activation,
    Ingress,
    Prompt,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirstDeliveryUnknown {
    pub(crate) source: DispatchSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeardownCause {
    ReaderEof,
    ReaderIo,
    ProcessExit,
    ProtocolFailure,
    PumpFailure,
    MandatoryDenyFailure,
    DeliveryUnknown,
    RefusedVersion {
        observed: crate::acp::contract::RequiredText<64>,
    },
    HostTermination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeardownDetail {
    Exit(TreeExit),
    Summary(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingExit {
    pub(crate) exit: Option<TreeExit>,
    pub(crate) deadline_millis: i64,
    pub(crate) termination_requested: bool,
}

impl From<TeardownCause> for crate::acp::lifecycle::TurnTerminalReason {
    fn from(value: TeardownCause) -> Self {
        match value {
            TeardownCause::ProtocolFailure => Self::ProtocolError,
            TeardownCause::ProcessExit | TeardownCause::ReaderEof | TeardownCause::ReaderIo => {
                Self::HarnessExited
            }
            TeardownCause::HostTermination => Self::CancelledByUser,
            TeardownCause::RefusedVersion { .. } => Self::RefusedVersion,
            TeardownCause::PumpFailure
            | TeardownCause::MandatoryDenyFailure
            | TeardownCause::DeliveryUnknown => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DriverError {
    IdentityMismatch,
    Mailbox(MailboxError),
    RejectedIntent,
    Refused(NonfatalRefusal),
    TeardownRequired(TeardownCause),
    Tree,
    NotReusable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DriverProgress {
    pub(crate) accepted_frames: usize,
    pub(crate) deferred_flushes: usize,
    pub(crate) written_frames: usize,
    pub(crate) cancelled_permission_frames: usize,
    pub(crate) zero_byte_failures: usize,
    pub(crate) rejected_controls: usize,
    /// The adapter holds the cancel until the harness names the Turn.
    pub(crate) cancel_deferred: bool,
}

pub(crate) trait DriverClock: Send + Sync {
    fn now_millis(&self) -> i64;
}

pub(crate) struct SystemDriverClock;

impl DriverClock for SystemDriverClock {
    fn now_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| i64::try_from(value.as_millis()).ok())
            .unwrap_or(i64::MAX)
    }
}

pub(crate) trait StdinTransport: EgressTransport {
    fn close(&mut self);
}

impl EgressTransport for StdinEndpoint {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.write_frame(frame)
    }
}

impl StdinTransport for StdinEndpoint {
    fn close(&mut self) {
        StdinEndpoint::close(self);
    }
}

pub(crate) trait ProcessTreeControl {
    type ZeroProof;

    fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError>;
    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError>;
    fn log_wait_handle_observation(&self, _: &'static str) {}
    fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_root_exit()
    }
    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError>;
}

pub(crate) trait CoordinatorZeroToken {
    fn try_into_coordinator_proof(
        self,
        owner: RuntimeOwner,
    ) -> Result<crate::acp::runtime::ZeroProcessProof, (Self, DriverError)>
    where
        Self: Sized;
}

impl CoordinatorZeroToken for ZeroProcessToken {
    fn try_into_coordinator_proof(
        self,
        owner: RuntimeOwner,
    ) -> Result<crate::acp::runtime::ZeroProcessProof, (Self, DriverError)> {
        match crate::acp::runtime::ZeroProcessProof::from_supervisor(owner, &self) {
            Ok(proof) => Ok(proof),
            Err(_) => Err((self, DriverError::IdentityMismatch)),
        }
    }
}

#[cfg(test)]
impl CoordinatorZeroToken for () {
    fn try_into_coordinator_proof(
        self,
        owner: RuntimeOwner,
    ) -> Result<crate::acp::runtime::ZeroProcessProof, (Self, DriverError)> {
        Ok(crate::acp::runtime::ZeroProcessProof::confirmed(owner))
    }
}

impl ProcessTreeControl for TreeControl {
    type ZeroProof = ZeroProcessToken;

    fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        TreeControl::request_termination(self, reason)
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        TreeControl::wait_root_exit(self)
    }

    fn log_wait_handle_observation(&self, checkpoint: &'static str) {
        TreeControl::log_wait_handle_observation(self, checkpoint);
    }

    fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        TreeControl::observe_root_exit(self)
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        TreeControl::wait_zero(self)
    }
}

pub(crate) trait TreeLeaseOwner {
    fn runtime_owner(&self) -> &RuntimeOwner;
}

impl TreeLeaseOwner for SessionTreeLease {
    fn runtime_owner(&self) -> &RuntimeOwner {
        self.owner()
    }
}

pub(super) fn tree_exit(cause: TeardownCause) -> TreeExit {
    match cause {
        TeardownCause::ReaderEof | TeardownCause::ProcessExit => TreeExit::Exited { code: None },
        TeardownCause::HostTermination => TreeExit::Cancelled,
        TeardownCause::ProtocolFailure
        | TeardownCause::PumpFailure
        | TeardownCause::MandatoryDenyFailure
        | TeardownCause::DeliveryUnknown
        | TeardownCause::RefusedVersion { .. }
        | TeardownCause::ReaderIo => TreeExit::ProtocolFailure,
    }
}
