use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::SqliteAdapterBatchJournal,
};

use super::super::{
    CoordinatorZeroToken, PreparedProcessWrite, PreparedWriteError, ProcessTreeControl,
    StdinTransport, WorkerEnvelope,
};
use super::turn::map_write;
use super::{FirstTurnWorkerRecovery, FirstTurnWorkerState};

// Recovery owns this already-boxed typestate; unboxing and reboxing it at the
// caller would add churn without reducing the bounded live-state footprint.
#[allow(clippy::boxed_local, clippy::type_complexity)]
pub(super) fn retain_active_after_write_error<A, R, T, C>(
    failure: Box<
        super::super::PreparedWriteFailure<
            PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
        >,
    >,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    envelope.respond(Err(map_write(&failure.error)));
    let active = if durable_commit_unknown(&failure.error) {
        failure.state.mark_durable_mutation_unknown()
    } else {
        failure.state
    };
    FirstTurnWorkerState::Active(active)
}

#[allow(clippy::type_complexity)]
pub(super) fn classify_begin_send_failure<A, R, T, C>(
    failure: Box<
        super::super::PreparedWriteFailure<
            PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
        >,
    >,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    envelope.respond(Err(map_write(&failure.error)));
    if failure.state.projection_retry_pending() {
        FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginSend {
            failure,
            owner,
            turn_id,
        }))
    } else if definitely_uncommitted(&failure.error) {
        FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::BeginSendRetry {
            failure,
            owner,
            turn_id,
        }))
    } else {
        let active = if durable_commit_unknown(&failure.error) {
            failure.state.mark_durable_mutation_unknown()
        } else {
            failure.state
        };
        FirstTurnWorkerState::Active(active)
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn classify_observe_failure<A, R, T, C>(
    failure: Box<
        super::super::PreparedWriteFailure<
            PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
        >,
    >,
    owner: crate::acp::scheduler::RuntimeOwner,
    turn_id: crate::acp::contract::SageTurnId,
    observation: crate::acp::store::SendObservation,
    envelope: WorkerEnvelope,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    envelope.respond(Err(map_write(&failure.error)));
    if failure.state.projection_retry_pending() {
        FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Observe {
            failure,
            owner,
            turn_id,
            observation,
        }))
    } else if definitely_uncommitted(&failure.error) {
        FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::ObserveRetry {
            failure,
            owner,
            turn_id,
            observation,
        }))
    } else {
        let active = if durable_commit_unknown(&failure.error) {
            failure.state.mark_durable_mutation_unknown()
        } else {
            failure.state
        };
        FirstTurnWorkerState::Active(active)
    }
}

fn definitely_uncommitted(error: &PreparedWriteError) -> bool {
    matches!(
        error,
        PreparedWriteError::Coordinator(crate::acp::runtime::CoordinatorError::Journal(
            crate::acp::runtime::JournalError::Capacity { .. }
                | crate::acp::runtime::JournalError::Deadline
        ))
    )
}

pub(super) fn durable_commit_unknown(error: &PreparedWriteError) -> bool {
    matches!(
        error,
        PreparedWriteError::Coordinator(crate::acp::runtime::CoordinatorError::Journal(
            crate::acp::runtime::JournalError::Store(crate::acp::store::StoreError::CommitUnknown)
        ))
    )
}
