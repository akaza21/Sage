use crate::acp::bindings::{adapter::AdapterRuntimeError, pi::AdapterError};

pub(in crate::acp::bindings::pi::runtime) fn map_pi(error: AdapterError) -> AdapterRuntimeError {
    match error {
        AdapterError::ConfigMutationRejected => AdapterRuntimeError::ConfigMutationRejected,
        AdapterError::UnsupportedOperation(_) => AdapterRuntimeError::UnsupportedCommand,
        AdapterError::LaunchNotAdmitted => AdapterRuntimeError::LaunchUnavailable,
        AdapterError::Malformed | AdapterError::Limit(_) => AdapterRuntimeError::Frame,
        AdapterError::PermissionViolation => AdapterRuntimeError::MandatoryDenyTeardown,
        _ => AdapterRuntimeError::Operation,
    }
}
