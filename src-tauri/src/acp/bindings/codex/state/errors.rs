use crate::acp::bindings::codex::{
    incoming::{IncomingEvent, IngressDisposition},
    manifest::Platform,
    request::RequestError,
};

pub(super) fn observed_platform() -> Platform {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Platform::MacosAarch64
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Platform::MacosX86_64
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Platform::LinuxX86_64
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Platform::WindowsX86_64
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error(transparent)]
    Contract(#[from] sage_acp_protocol::contract::ContractError),
    #[error(transparent)]
    Ingress(#[from] crate::acp::bindings::codex::incoming::IngressError),
    #[error("outstanding request ledger is full")]
    PendingFull,
    #[error("duplicate outstanding request id")]
    DuplicateRequestId,
    #[error("binding capabilities are unavailable or do not admit this operation")]
    CapabilityUnavailable,
    #[error("the adapter was already initialized")]
    AlreadyInitialized,
    #[error("session identity is unavailable")]
    SessionUnavailable,
    #[error("a Turn start is already pending or active")]
    TurnAlreadyActive,
    #[error("active turn identity is unavailable")]
    TurnUnavailable,
    #[error("outbound request does not belong to this adapter")]
    ForeignRequest,
    #[error("external identity conflicts with durable binding")]
    IdentityConflict,
    #[error("external identity belongs to a foreign session, Turn, or epoch")]
    ForeignIdentity,
    #[error("Turn start intent is stale or conflicting")]
    TurnIntentConflict,
    #[error("terminal item ledger is full")]
    TerminalLedgerFull,
    #[error("permission decline lifecycle failed; runtime teardown is required")]
    PermissionTeardownRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnStartedRejection {
    ForeignThread,
    OverlappingTurn,
    MissingCorrelation,
    Other,
}

impl TurnStartedRejection {
    pub(crate) fn from_adapter_error(error: &AdapterError) -> Self {
        match error {
            AdapterError::ForeignIdentity => Self::ForeignThread,
            AdapterError::IdentityConflict => Self::OverlappingTurn,
            AdapterError::TurnIntentConflict => Self::MissingCorrelation,
            _ => Self::Other,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ForeignThread => "foreignThread",
            Self::OverlappingTurn => "overlappingTurn",
            Self::MissingCorrelation => "missingCorrelation",
            Self::Other => "other",
        }
    }
}

impl From<IncomingEvent> for IngressDisposition {
    fn from(event: IncomingEvent) -> Self {
        Self::Events(vec![event])
    }
}
