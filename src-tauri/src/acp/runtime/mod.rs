#![allow(dead_code)] // Removed when the isolated packet receives central wiring.

mod activation_pump;
mod adapter_pump;
mod adapter_store_journal;
mod bounded_thread;
mod coordinator;
mod journal;
mod journal_support;
mod live_capacity;
mod live_engine;
pub(crate) mod later_permission;
mod mailbox;
mod normalizer;
mod notifier;
pub(crate) mod permissions;

#[allow(unused_imports)]
pub(crate) use activation_pump::{
    ActivatedSession, ActivationFlush, ActivationIntent, ActivationJournal, ActivationJournalError,
    ActivationMode, ActivationPump, ActivationPumpError, ActivationReceipt, ActivationTransport,
    CoordinatorActivationHandoff, CoordinatorActivationJournal, CoordinatorShutdownHandoff,
    IdleActivationParts, RecoveryActivationParts, TeardownActivationParts,
};
#[allow(unused_imports)]
pub(crate) use adapter_pump::{
    ActiveTurnIntent, AdapterBatchJournal, AdapterPump, AdapterPumpAdmission, AdapterPumpError,
    CompletedPumpParts, DurableAdapterEvent, EgressTransport, FlushDisposition, JournalReceipt,
    PumpIntent, PumpJournalError, QueueDisposition, SessionPumpIntent, TurnScope,
    normalized_payload_bytes,
};
#[allow(unused_imports)]
pub(crate) use adapter_store_journal::SqliteAdapterBatchJournal;
pub(crate) use bounded_thread::{BoundedThread, ThreadWaitError};
mod process_driver;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use process_driver::{
    CancelDispatchObservation, observe_process_live_port_cancel_at_capacity,
    protocol_failure_requests_tree_exit,
};
mod projection_publisher;
mod runtime_host;
mod session_coordinator;

#[allow(unused_imports)]
pub(crate) use coordinator::{
    ActiveDispatch, AmbiguousTurnReservation, CoordinatorError, PrepareDispatch, PrepareFailure,
    PrepareRecovery, ProjectIntentAuthority, SendCoordinator,
};
#[allow(unused_imports)]
pub(crate) use journal::{JournalError, JournalHandle, JournalLane, JournalRunner};
#[allow(unused_imports)]
pub(crate) use live_capacity::{
    CapacityScope, LiveCapacityError, LiveCapacityLease, LiveCapacityPool,
};
#[allow(unused_imports)]
pub(crate) use live_engine::{LiveRuntimeEngine, LiveRuntimePort, LiveWrite};
#[allow(unused_imports)]
pub(crate) use mailbox::{MailboxError, ProjectionMailbox, PublishDisposition};
#[allow(unused_imports)]
pub(crate) use normalizer::{NormalizerInput, normalize};
#[allow(unused_imports)]
pub(crate) use notifier::{
    NotificationError, NotificationRecovery, NotificationSink, NotificationSinkFuture,
    ProjectionNotifier, PublishError, PublishOutcome,
};
#[allow(unused_imports)]
pub(crate) use process_driver::{
    ActivationDriverError, ActivationProcessDriver, ActivationProjectionRecoveryHandoff,
    ActivationStartFailure, ActivationZeroProcessHandoff, ActiveCloseBeginState, BindTurnResult,
    BlockingStdout, BlockingStdoutReader, CancelIntent, CompleteTurnResult,
    CompletedActivationDriverParts, CompletedDriverParts, CoordinatorZeroProcessHandoff,
    DriverAdmission, DriverClock, DriverError, DriverMailbox, DriverProgress, DriverStep,
    FirstDeliveryUnknown, FirstTurnBindError, FirstTurnBindFailure, FirstTurnDriverResult,
    FirstTurnRecoveryParts, FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState,
    LiveRunnerExit, LiveRunnerHandle, MailboxError as DriverMailboxError,
    PrepareActivationTurnError, PrepareActivationTurnFailure, PrepareDriverTurnError,
    PrepareDriverTurnFailure, PreparedActivationTurn, PreparedDriverTurn, PreparedProcessWrite,
    PreparedWriteError, PreparedWriteFailure, ProcessAdapterDriver, ProcessExitWatcher,
    ProcessLivePort, ProcessTreeControl, PromptIntent, ReaderTerminal, RebindFailure,
    RecoveryActivationDriverParts, RetainedActivationDriverConstruction, StdinTransport,
    SystemDriverClock, TeardownCause, TreeLeaseOwner, ZeroProcessHandoff,
};
#[allow(unused_imports)]
pub(crate) use projection_publisher::{
    ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher,
};
#[allow(unused_imports)]
pub(crate) use runtime_host::{
    HostCancelIntent, HostCloseIntent, HostError, HostOutcome, HostPromptIntent, HostState,
    PromptWrite, RuntimeEngine, RuntimeHost, RuntimeShutdownRecovery, TerminalNotification,
};
#[allow(unused_imports)]
pub(crate) use session_coordinator::{
    ActivationFailure, ActivationRecovery, SessionCoordinator, SessionCoordinatorError,
    SessionTreeLease, ZeroProcessProof,
};

#[cfg(test)]
mod journal_admission_tests;
#[cfg(test)]
mod notifier_tests;
#[cfg(test)]
mod tests;
