use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    lifecycle::TurnState,
    runtime::{
        CoordinatorActivationJournal, HostCancelIntent, HostError, JournalHandle,
        ProjectIntentAuthority, ProjectionPublisher, SendCoordinator, SqliteAdapterBatchJournal,
    },
    scheduler::RuntimeOwner,
};

use super::{
    ActiveCloseBeginState, CloseError, CloseFailure, ClosingProcessWrite,
    CompletedActivationDriverParts, CompletedDriverParts, CoordinatorZeroToken, DriverClock,
    DriverMailbox, FirstTurnBindFailure, IdleCloseBeginState, IdleCloseFailure, IdleClosingState,
    IdleZeroState, PrepareActivationTurnFailure, PrepareDriverTurnFailure, PreparedActivationTurn,
    PreparedProcessWrite, PreparedWriteFailure, ProcessTreeControl, ReusableCloseBeginState,
    ReusableClosingState, ReusableZeroState, StdinTransport, WorkerCommand, WorkerEnvelope,
    WorkerReply, ZeroCloseState,
};

mod active_close;
mod idle;
mod recovery;
mod reusable;
mod session_config;
mod turn;
mod turn_park;
mod write_recovery;

pub(crate) struct FirstTurnWorkerContext<Auth> {
    pub(crate) owner: RuntimeOwner,
    pub(crate) coordinator: SendCoordinator<Auth>,
    pub(crate) journal: JournalHandle,
    pub(crate) publisher: Arc<dyn ProjectionPublisher>,
    pub(crate) clock: Arc<dyn DriverClock>,
    pub(crate) mailbox: Arc<DriverMailbox>,
    pub(crate) zero_sequence: AtomicU64,
}

impl<Auth> FirstTurnWorkerContext<Auth> {
    fn next_zero_token(&self) -> Option<u64> {
        self.zero_sequence
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .ok()
            .and_then(|previous| previous.checked_add(1))
    }
}

// Each variant owns an exact typestate and is capped by the live-tree
// scheduler. Keeping it inline preserves deterministic drop/zero-proof order.
#[allow(clippy::large_enum_variant)]
pub(crate) enum FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    Idle(CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>),
    Prepared {
        turn: PreparedActivationTurn<A, R, T, C>,
        prompt: crate::acp::intent::Prompt,
    },
    Active(PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>),
    Reusable(
        CompletedDriverParts<
            A,
            SqliteAdapterBatchJournal,
            R,
            T,
            C,
            crate::acp::runtime::SessionTreeLease,
        >,
    ),
    ReusableDraining(ReusableClosingState<A, SqliteAdapterBatchJournal, R, T, C>),
    ReusableZero(ReusableZeroState<A, SqliteAdapterBatchJournal, R, T, C>),
    Draining(ClosingProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>),
    Zero(ZeroCloseState<A, SqliteAdapterBatchJournal, R, T, C>),
    IdleDraining(IdleClosingState<A, R, T, C>),
    IdleZero(IdleZeroState<A, R, T, C>),
    Closed,
    Recovery(Box<FirstTurnWorkerRecovery<A, R, T, C>>),
}

// The explicit generic states are intentionally verbose: each recovery
// variant retains exactly one non-replayable operation and its ownership.
#[allow(clippy::type_complexity)]
pub(crate) enum FirstTurnWorkerRecovery<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    Prepare(
        Box<
            PrepareActivationTurnFailure<
                CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>,
            >,
        >,
    ),
    PrepareNext(
        Box<
            PrepareDriverTurnFailure<
                CompletedDriverParts<
                    A,
                    SqliteAdapterBatchJournal,
                    R,
                    T,
                    C,
                    crate::acp::runtime::SessionTreeLease,
                >,
            >,
        >,
    ),
    Bind(Box<FirstTurnBindFailure<A, R, T, C>>),
    BeginSend {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
    },
    BeginSendRetry {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
    },
    BeginCancel {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        intent: HostCancelIntent,
    },
    Observe {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
        observation: crate::acp::store::SendObservation,
    },
    ObserveRetry {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
        observation: crate::acp::store::SendObservation,
    },
    Terminal {
        failure:
            Box<PreparedWriteFailure<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>,
        owner: RuntimeOwner,
        cursor: crate::acp::store::AdapterTurnCursor,
    },
    BeginClose(Box<CloseFailure<ActiveCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>>>),
    Converge(Box<CloseFailure<ClosingProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>>),
    Finalize(Box<CloseFailure<ZeroCloseState<A, SqliteAdapterBatchJournal, R, T, C>>>),
    IdleBegin(Box<IdleCloseFailure<IdleCloseBeginState<A, R, T, C>>>),
    IdleConverge(Box<IdleCloseFailure<IdleClosingState<A, R, T, C>>>),
    IdleFinalize(Box<IdleCloseFailure<IdleZeroState<A, R, T, C>>>),
    ReusableBegin(
        Box<CloseFailure<ReusableCloseBeginState<A, SqliteAdapterBatchJournal, R, T, C>>>,
    ),
    ReusableConverge(
        Box<CloseFailure<ReusableClosingState<A, SqliteAdapterBatchJournal, R, T, C>>>,
    ),
    ReusableFinalize(Box<CloseFailure<ReusableZeroState<A, SqliteAdapterBatchJournal, R, T, C>>>),
}

impl<A, R, T, C> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) async fn handle<Auth: ProjectIntentAuthority>(
        self,
        context: &FirstTurnWorkerContext<Auth>,
        envelope: WorkerEnvelope,
    ) -> Self {
        match self {
            Self::Idle(parts) => turn::handle_idle(parts, context, envelope).await,
            Self::Prepared { turn, prompt } => {
                turn::handle_prepared(turn, prompt, context, envelope).await
            }
            Self::Active(active) => turn::handle_active(active, context, envelope).await,
            Self::Reusable(parts) => turn::handle_reusable(parts, context, envelope).await,
            Self::ReusableDraining(closing) => {
                reusable::handle_draining(closing, context, envelope).await
            }
            Self::ReusableZero(zero) => reusable::handle_zero(zero, context, envelope).await,
            Self::Draining(closing) => {
                active_close::handle_draining(closing, context, envelope).await
            }
            Self::Zero(zero) => active_close::handle_zero(zero, context, envelope).await,
            Self::IdleDraining(closing) => idle::handle_draining(closing, context, envelope).await,
            Self::IdleZero(zero) => idle::handle_zero(zero, context, envelope).await,
            Self::Closed => {
                if matches!(envelope.command, WorkerCommand::Shutdown) {
                    envelope.respond(Ok(WorkerReply::Unit));
                } else {
                    envelope.reject(HostError::Closed);
                }
                Self::Closed
            }
            Self::Recovery(recovery) => match *recovery {
                FirstTurnWorkerRecovery::Finalize(failure) => {
                    active_close::handle_zero(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::BeginClose(failure) => {
                    active_close::handle_begin(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::Converge(failure) => {
                    active_close::handle_draining(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::IdleBegin(failure) => {
                    idle::handle_begin(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::IdleConverge(failure) => {
                    idle::handle_draining(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::IdleFinalize(failure) => {
                    idle::handle_zero(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::ReusableBegin(failure) => {
                    reusable::handle_begin(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::ReusableConverge(failure) => {
                    reusable::handle_draining(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::ReusableFinalize(failure) => {
                    reusable::handle_zero(failure.state, context, envelope).await
                }
                FirstTurnWorkerRecovery::BeginCancel { failure, intent } => {
                    recovery::handle_begin_cancel(failure.state, intent, envelope).await
                }
                FirstTurnWorkerRecovery::BeginSend {
                    failure,
                    owner,
                    turn_id,
                } => recovery::handle_begin_send(failure.state, owner, turn_id, envelope),
                FirstTurnWorkerRecovery::BeginSendRetry {
                    failure,
                    owner,
                    turn_id,
                } => {
                    recovery::handle_begin_send_retry(
                        failure.state,
                        owner,
                        turn_id,
                        context,
                        envelope,
                    )
                    .await
                }
                FirstTurnWorkerRecovery::Observe {
                    failure,
                    owner,
                    turn_id,
                    observation,
                } => recovery::handle_observe(failure.state, owner, turn_id, observation, envelope),
                FirstTurnWorkerRecovery::ObserveRetry {
                    failure,
                    owner,
                    turn_id,
                    observation,
                } => {
                    recovery::handle_observe_retry(
                        failure.state,
                        owner,
                        turn_id,
                        observation,
                        context,
                        envelope,
                    )
                    .await
                }
                FirstTurnWorkerRecovery::Terminal {
                    failure,
                    owner,
                    cursor,
                } => recovery::handle_terminal(failure.state, owner, cursor, context, envelope),
                retained => {
                    envelope.reject(HostError::RecoveryRequired);
                    Self::Recovery(Box::new(retained))
                }
            },
        }
    }
}

impl<A, R, T, C> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn close_stdin_after_durable_completed(&mut self) {
        match self {
            Self::Reusable(parts) => {
                parts.write_close_session();
                parts.close_stdin();
            }
            Self::Active(active) => {
                if active.has_durable_terminal() {
                    active.write_close_session();
                }
                active.close_stdin();
            }
            _ => {}
        }
    }

    pub(crate) async fn park_after_zero(&mut self, token: crate::acp::process::ZeroProcessToken) {
        match self {
            Self::Reusable(parts) => parts.park_after_zero(token).await,
            Self::Active(active) if active.has_durable_terminal() => {
                active.park_after_zero(token).await;
            }
            _ => {}
        }
    }

    pub(crate) fn has_durable_completed_cursor(&self) -> bool {
        self.durable_cursor_is(TurnState::Completed)
    }

    pub(crate) fn has_durable_cancelled_cursor(&self) -> bool {
        match self {
            Self::Reusable(parts) => matches!(
                parts.terminal_cursor().state(),
                TurnState::Cancelled | TurnState::Interrupted
            ),
            Self::Active(active) => {
                active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .is_some_and(|cursor| {
                        matches!(
                            cursor.state(),
                            TurnState::Cancelled | TurnState::Interrupted
                        )
                    })
            }
            _ => false,
        }
    }

    fn durable_cursor_is(&self, expected: TurnState) -> bool {
        match self {
            Self::Reusable(parts) => parts.terminal_cursor().state() == expected,
            Self::Active(active) => active
                .turn
                .driver
                .latest_durable_turn_cursor()
                .is_some_and(|cursor| cursor.state() == expected),
            _ => false,
        }
    }
}

fn map_close(error: &CloseError) -> HostError {
    match error {
        CloseError::Intent => HostError::Intent,
        CloseError::Session(_)
        | CloseError::Write(_)
        | CloseError::Driver(_)
        | CloseError::Proof => HostError::RecoveryRequired,
    }
}
