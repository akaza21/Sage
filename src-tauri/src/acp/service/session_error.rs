use crate::acp::runtime::SessionCoordinatorError;

use super::CodexLaunchSetupError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexSessionError {
    #[error("the exact Codex binding is unavailable")]
    Binding(crate::acp::bindings::codex::CodexAdmissionUnavailable),
    #[error("the Codex session intent is stale or foreign")]
    Intent,
    #[error("The pending undo could not be applied; the rewind remains pending.")]
    RewindUnavailable,
    #[error(transparent)]
    Session(#[from] SessionCoordinatorError),
    #[error("Codex runtime capacity is occupied")]
    Capacity,
    #[error("runtime capacity is occupied while an idle runtime answers a session control")]
    CapacityAnswering,
    #[error("the harness requires sign-in before it can continue")]
    SignInRequired,
    #[error("Codex live runtime requires recovery")]
    Recovery,
    #[error("the Steer acknowledgement timed out; the message may have been delivered")]
    SteerOutcomeUnknown,
    #[error("Codex process launch failed: {0:?}")]
    LaunchFailure(sage_process::error::LaunchError),
    #[error("Codex process launch setup failed: {0:?}")]
    LaunchSetup(CodexLaunchSetupError),
}

pub(crate) enum SessionDispatchError {
    Binding,
    Operation(CodexSessionError),
}
