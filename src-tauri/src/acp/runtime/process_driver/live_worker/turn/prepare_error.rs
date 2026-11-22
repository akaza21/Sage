use crate::acp::{
    runtime::{CoordinatorError, HostError, JournalError},
    store::StoreError,
};

use super::super::super::{
    PrepareActivationTurnError, PrepareDriverTurnError, PrepareDriverTurnFailure,
};

// Each of these follows the PrepareTurn commit or finds a turn already
// prepared, so the worker enters recovery and the prompt may have been kept:
// none is the clean refusal (`Intent`) that tells the chat it never started.
pub(super) fn map_prepare(error: &PrepareActivationTurnError) -> HostError {
    match error {
        PrepareActivationTurnError::Duplicate
        | PrepareActivationTurnError::Identity
        | PrepareActivationTurnError::Prepare(_) => HostError::RecoveryRequired,
    }
}

pub(super) fn map_prepare_next(error: &PrepareDriverTurnError) -> HostError {
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
pub(super) fn repository_busy<P>(failure: &PrepareDriverTurnFailure<P>) -> bool {
    failure.recovery.is_none()
        && matches!(
            failure.error,
            PrepareDriverTurnError::Prepare(CoordinatorError::Journal(JournalError::Store(
                StoreError::RepositoryBusy
            )))
        )
}
