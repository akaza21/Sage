use crate::acp::runtime::HostError;

use super::super::super::{PrepareActivationTurnError, PrepareDriverTurnError};

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
