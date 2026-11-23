//! How a worker settles a prompt whose Turn failed to prepare: which host
//! error the caller sees, and whether the worker stays usable or enters
//! recovery holding the failure.

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        CoordinatorActivationJournal, CoordinatorError, HostError, JournalError, SessionTreeLease,
        SqliteAdapterBatchJournal,
    },
    store::StoreError,
};

use super::super::super::{
    CompletedActivationDriverParts, CompletedDriverParts, CoordinatorZeroToken,
    PrepareActivationTurnError, PrepareActivationTurnFailure, PrepareDriverTurnError,
    PrepareDriverTurnFailure, ProcessTreeControl, StdinTransport, worker_command::WorkerResponder,
};
use super::super::{FirstTurnWorkerRecovery, FirstTurnWorkerState};

type ReusableParts<A, R, T, C> =
    CompletedDriverParts<A, SqliteAdapterBatchJournal, R, T, C, SessionTreeLease>;
type ActivationParts<A, R, T, C> =
    CompletedActivationDriverParts<A, CoordinatorActivationJournal, R, T, C>;

/// The first Turn of an activated session failed to prepare.
pub(super) fn settle_first<A, R, T, C>(
    failure: Box<PrepareActivationTurnFailure<ActivationParts<A, R, T, C>>>,
    responder: WorkerResponder,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    responder.respond(Err(map_prepare(&failure.error)));
    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::Prepare(failure)))
}

/// A reusable session's next Turn failed to prepare.
pub(super) fn settle_next<A, R, T, C>(
    failure: Box<PrepareDriverTurnFailure<ReusableParts<A, R, T, C>>>,
    responder: WorkerResponder,
) -> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if let Some(pause) = repository_busy(&failure) {
        responder.respond(Err(HostError::RepositoryBusy(pause)));
        return FirstTurnWorkerState::Reusable(Box::new(failure.parts));
    }
    responder.respond(Err(map_prepare_next(&failure.error)));
    FirstTurnWorkerState::Recovery(Box::new(FirstTurnWorkerRecovery::PrepareNext(failure)))
}

// Each of these follows the PrepareTurn commit or finds a turn already
// prepared, so the worker enters recovery and the prompt may have been kept:
// none is the clean refusal (`Intent`) that tells the chat it never started.
fn map_prepare(error: &PrepareActivationTurnError) -> HostError {
    match error {
        PrepareActivationTurnError::Duplicate
        | PrepareActivationTurnError::Identity
        | PrepareActivationTurnError::Prepare(_) => HostError::RecoveryRequired,
    }
}

fn map_prepare_next(error: &PrepareDriverTurnError) -> HostError {
    match error {
        PrepareDriverTurnError::Duplicate
        | PrepareDriverTurnError::Identity
        | PrepareDriverTurnError::MissingLease
        | PrepareDriverTurnError::Prepare(_)
        | PrepareDriverTurnError::Rebind(_) => HostError::RecoveryRequired,
    }
}

/// The store refused the Turn before anything committed because Sage holds a
/// repository lease over the Project. Nothing reached the harness and the
/// Turn slot is released, so the worker stays reusable and the prompt is
/// refused as busy, like any other prompt the worker cannot take now.
fn repository_busy<P>(
    failure: &PrepareDriverTurnFailure<P>,
) -> Option<crate::repository_mutation_guard::RepositoryPause> {
    if failure.recovery.is_some() {
        return None;
    }
    match failure.error {
        PrepareDriverTurnError::Prepare(CoordinatorError::Journal(JournalError::Store(
            StoreError::RepositoryBusy(pause),
        ))) => Some(pause),
        _ => None,
    }
}
