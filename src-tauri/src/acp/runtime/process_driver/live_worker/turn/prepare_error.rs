use crate::acp::runtime::HostError;

use super::super::super::{PrepareActivationTurnError, PrepareDriverTurnError};

pub(super) fn map_prepare(error: &PrepareActivationTurnError) -> HostError {
    match error {
        PrepareActivationTurnError::Duplicate | PrepareActivationTurnError::Identity => {
            HostError::Intent
        }
        PrepareActivationTurnError::Prepare(_) => HostError::RecoveryRequired,
    }
}

pub(super) fn map_prepare_next(error: &PrepareDriverTurnError) -> HostError {
    match error {
        PrepareDriverTurnError::Duplicate | PrepareDriverTurnError::Identity => HostError::Intent,
        PrepareDriverTurnError::MissingLease
        | PrepareDriverTurnError::Prepare(_)
        | PrepareDriverTurnError::Rebind(_) => HostError::RecoveryRequired,
    }
}
