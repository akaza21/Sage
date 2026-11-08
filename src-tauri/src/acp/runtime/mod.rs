mod activation_pump;
mod adapter_pump;
mod adapter_store_journal;
mod bounded_thread;
mod coordinator;
#[allow(
    dead_code,
    reason = "agent file operations are staged and not dispatched yet"
)]
pub(crate) mod file_ops;
pub(crate) mod host_browse;
mod journal;
mod journal_support;
pub(crate) mod later_permission;
#[cfg(all(test, feature = "acp-fixture"))]
mod live_capacity;
mod live_engine;
#[cfg(test)]
mod mailbox;
#[cfg(test)]
mod normalizer;
pub(crate) mod notice_routes;
mod notifier;
pub(crate) mod permissions;
pub(crate) mod read_text_file;

pub(crate) use activation_pump::{
    ActivatedSession, ActivationFlush, ActivationIntent, ActivationJournal, ActivationMode,
    ActivationPump, ActivationPumpError, ActivationTransport, CoordinatorActivationHandoff,
    CoordinatorActivationJournal, CoordinatorShutdownHandoff, IdleActivationParts,
    RecoveryActivationParts, TeardownActivationParts,
};
#[cfg(test)]
pub(crate) use activation_pump::{ActivationJournalError, ActivationReceipt};
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use adapter_pump::normalized_payload_bytes;
pub(crate) use adapter_pump::{
    ActiveTurnIntent, AdapterBatchJournal, AdapterPump, AdapterPumpError, CompletedPumpParts,
    EgressTransport, FlushDisposition, NonfatalRefusal, PumpJournalError, SessionPumpIntent,
};
#[cfg(test)]
pub(crate) use adapter_pump::{DurableAdapterEvent, JournalReceipt, PumpIntent, TurnScope};
pub(crate) use adapter_store_journal::SqliteAdapterBatchJournal;
pub(crate) use bounded_thread::{BoundedThread, ThreadWaitError};
mod process_driver;
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use process_driver::{
    CancelDispatchObservation, observe_process_live_port_cancel_at_capacity,
    protocol_failure_requests_tree_exit,
};
mod projection_publisher;
pub(crate) mod prompt_diagnostics;
mod runtime_host;
mod session_coordinator;
pub(crate) mod terminal_detail;

pub(crate) use coordinator::{
    ActiveDispatch, CoordinatorError, PrepareDispatch, PrepareFailure, PrepareRecovery,
    ProjectIntentAuthority, SendCoordinator,
};
#[cfg(test)]
pub(crate) use journal::JournalRunner;
pub(crate) use journal::{JournalError, JournalHandle, JournalLane};
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use live_capacity::{
    CapacityScope, LiveCapacityError, LiveCapacityLease, LiveCapacityPool,
};
pub(crate) use live_engine::{LiveRuntimeEngine, LiveRuntimePort, LiveWrite};
#[cfg(test)]
pub(crate) use mailbox::{MailboxError, ProjectionMailbox, PublishDisposition};
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use normalizer::{NormalizerInput, normalize};
pub(crate) use notifier::{
    NotificationError, NotificationRecovery, NotificationSink, NotificationSinkFuture,
    ProjectionNotifier, PublishError, PublishOutcome,
};
#[cfg(test)]
pub(crate) use process_driver::CompletedActivationDriverParts;
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use process_driver::MailboxError as DriverMailboxError;
#[cfg(any(test, all(windows, feature = "acp-fixture")))]
pub(crate) use process_driver::StdinTransport;
#[cfg(all(test, windows, feature = "acp-fixture"))]
pub(crate) use process_driver::{
    ActivationDriverError, ActiveCloseBeginState, BlockingStdoutReader,
};
pub(crate) use process_driver::{
    ActivationProcessDriver, ActivationProjectionRecoveryHandoff, ActivationStartFailure,
    ActivationZeroProcessHandoff, DriverClock, DriverError, DriverMailbox, FirstTurnWorkerContext,
    LiveRunnerExit, LiveRunnerHandle, PreparedProcessWrite, PreparedWriteError, ProcessLivePort,
    ProcessTreeControl, ReaderTerminal, RunnerStartSubstep, SystemDriverClock, TeardownCause,
};
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use process_driver::{ProcessAdapterDriver, TreeLeaseOwner};
#[cfg(test)]
pub(crate) use projection_publisher::ProjectionPublishError;
pub(crate) use projection_publisher::{ProjectionPublishStatus, ProjectionPublisher, TurnNotice};
pub(crate) use runtime_host::HostState;
pub(crate) use runtime_host::{
    ConfigCompletionSink, HostCancelIntent, HostCloseIntent, HostError, HostOutcome,
    HostPromptIntent, HostSteerIntent, NativeRpcCompletionSink, PromptWrite, RuntimeEngine,
    RuntimeHost, RuntimeShutdownRecovery, SteerCompletionSink, TerminalNotification,
};
pub(crate) use session_coordinator::{
    ActivationRecovery, SessionCoordinator, SessionCoordinatorError, SessionTreeLease,
    ZeroProcessProof,
};

#[cfg(test)]
mod journal_admission_tests;
#[cfg(test)]
mod notifier_tests;
#[cfg(test)]
mod tests;
