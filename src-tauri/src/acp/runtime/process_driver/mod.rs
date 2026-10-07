mod activation;
mod activation_parts;
mod close;
mod driver;
mod idle_close;
mod live_runner;
mod live_worker;
mod mailbox;
mod prepared_write;
mod reader;
mod reusable_close;
mod reuse;
mod session_config;
mod types;
mod worker_command;
mod write_fence;

pub(crate) use close::{
    ActiveCloseBeginState, CloseError, CloseFailure, ClosingProcessWrite, WorkerZeroProof,
    ZeroCloseState,
};
pub(crate) use driver::{DriverAdmission, DriverStep, ProcessAdapterDriver};
pub(crate) use idle_close::{
    IdleCloseBeginState, IdleCloseError, IdleCloseFailure, IdleClosingState, IdleZeroState,
};
pub(crate) use live_runner::{
    ActivationStartFailure, LiveRunnerExit, LiveRunnerHandle, RunnerStartSubstep,
};
pub(crate) use live_worker::{
    FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState,
};
pub(crate) use mailbox::{
    CancelIntent, ControlMessage, DriverMailbox, MailboxError, PromptIntent, ReaderTerminal,
};
pub(crate) use prepared_write::{PreparedProcessWrite, PreparedWriteError, PreparedWriteFailure};
pub(crate) use reader::{BlockingStdout, BlockingStdoutReader, ProcessExitWatcher};
pub(crate) use reusable_close::{ReusableCloseBeginState, ReusableClosingState, ReusableZeroState};
pub(crate) use reuse::{
    BindTurnResult, CompleteTurnResult, CompletedDriverParts, CoordinatorZeroProcessHandoff,
    PrepareDriverTurnError, PrepareDriverTurnFailure, PreparedDriverTurn, RebindFailure,
    ZeroProcessHandoff,
};
pub(crate) use types::{
    CoordinatorZeroToken, DispatchSource, DriverClock, DriverError, DriverProgress,
    FirstDeliveryUnknown, PendingExit, ProcessTreeControl, StdinTransport, SystemDriverClock,
    TeardownCause, TeardownDetail, TreeLeaseOwner,
};
pub(crate) use worker_command::{ProcessLivePort, WorkerCommand, WorkerEnvelope, WorkerReply};
use write_fence::{WriteFence, WriteFenceError};

#[cfg(test)]
pub(crate) fn protocol_failure_requests_tree_exit() -> bool {
    matches!(
        types::tree_exit(TeardownCause::ProtocolFailure),
        crate::acp::process::TreeExit::ProtocolFailure
    )
}

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod first_turn_tests;
#[cfg(test)]
pub(crate) use first_turn_tests::{
    CancelDispatchObservation, observe_process_live_port_cancel_at_capacity,
};
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worker_tests;
pub(crate) use activation::{
    ActivationDriverError, ActivationProcessDriver, ActivationStartupStep,
    RetainedActivationDriverConstruction,
};
pub(crate) use activation_parts::{
    ActivationProjectionRecoveryHandoff, ActivationZeroProcessHandoff,
    CompletedActivationDriverParts, FirstTurnBindError, FirstTurnBindFailure,
    FirstTurnDriverResult, FirstTurnRecoveryParts, PrepareActivationTurnError,
    PrepareActivationTurnFailure, PreparedActivationTurn, RecoveryActivationDriverParts,
};
