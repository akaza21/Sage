use crate::acp::runtime::SessionCoordinatorError;

use super::CodexLaunchSetupError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexSessionError {
    #[error("the exact Codex binding is unavailable")]
    Binding(crate::acp::bindings::codex::CodexAdmissionUnavailable),
    #[error("the Codex session intent is stale or foreign")]
    Intent,
    #[error("this native Codex thread was started on an earlier Codex version")]
    EarlierCodexVersion,
    #[error(transparent)]
    Session(#[from] SessionCoordinatorError),
    #[error("Codex runtime capacity is occupied")]
    Capacity,
    #[error("the harness requires sign-in before it can continue")]
    SignInRequired,
    #[error("Codex browser tools are unavailable for this session")]
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
    )]
    BrowserToolsUnavailable,
    #[error("Codex live runtime requires recovery")]
    Recovery,
    #[error("the Steer acknowledgement timed out; the message may have been delivered")]
    SteerOutcomeUnknown,
    #[error("Codex process launch failed: {0:?}")]
    LaunchFailure(crate::acp::process::LaunchError),
    #[error("Codex process launch setup failed: {0:?}")]
    LaunchSetup(CodexLaunchSetupError),
}

pub(crate) enum SessionDispatchError {
    Binding,
    Operation(CodexSessionError),
}
