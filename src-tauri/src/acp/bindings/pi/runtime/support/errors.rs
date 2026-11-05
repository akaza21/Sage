use super::PiRuntime;
use crate::acp::{
    bindings::{
        adapter::{AdapterEvent, AdapterRuntimeError},
        pi::AdapterError,
    },
    contract::SafeText,
    intent::{ErrorKind, NormalizedPayload},
    lifecycle::{ItemLifecycle, TurnTerminalReason},
};

const ATTEMPT_FAILURE_CODE: &str = "Attempt failed";

impl PiRuntime {
    pub(super) fn failure_detail(
        &self,
        message: Option<String>,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        Ok(AdapterEvent::TerminalReason {
            id: self.turn_identity()?,
            reason: TurnTerminalReason::Other,
            message: message
                .map(|message| SafeText::new("turnTerminalReasonMessage", message))
                .transpose()
                .map_err(|_| AdapterRuntimeError::Frame)?,
        })
    }

    pub(super) fn attempt_failed(
        &mut self,
        attempt: u64,
        max_attempts: Option<u64>,
        message: String,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self.next_local_item("attempt")?;
        let code = match max_attempts {
            Some(maximum) => format!("{ATTEMPT_FAILURE_CODE} (attempt {attempt}/{maximum})"),
            None => format!("{ATTEMPT_FAILURE_CODE} (attempt {attempt})"),
        };
        self.item(
            id,
            ItemLifecycle::Failed,
            None,
            NormalizedPayload::Error {
                kind: Some(ErrorKind::Attempt),
                code: SafeText::new("errorCode", code).map_err(|_| AdapterRuntimeError::Frame)?,
                message: SafeText::new("errorMessage", message)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                retryable: false,
            },
        )
    }
}

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
