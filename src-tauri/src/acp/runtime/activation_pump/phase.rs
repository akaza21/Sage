use super::{ActivationMode, ActivationStartupStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Dormant,
    InitializeQueued,
    AwaitingInitialized,
    Initialized,
    SessionQueued,
    AwaitingSession,
    Ready,
    RecoveryRequired,
    Teardown(ActivationStartupStep),
}

impl Phase {
    /// Adapter requests that join the bounded initialization budget: the
    /// handshake itself, and ACP sign-in while the session is not yet ready
    /// (`authenticate`, then `session/new` again).
    pub(super) fn admits_initialization_request(self, session_ready: bool) -> bool {
        self == Self::AwaitingInitialized || (self == Self::AwaitingSession && !session_ready)
    }

    pub(super) fn startup_step(self, mode: &ActivationMode) -> ActivationStartupStep {
        match self {
            Self::Dormant | Self::InitializeQueued | Self::AwaitingInitialized => {
                ActivationStartupStep::Initialize
            }
            Self::Initialized | Self::SessionQueued | Self::AwaitingSession => match mode {
                ActivationMode::New { .. } => ActivationStartupStep::SessionNew,
                ActivationMode::Resume { .. } => ActivationStartupStep::SessionResume,
                ActivationMode::Load { .. } => ActivationStartupStep::SessionLoad,
            },
            // Only an unresolved activation commit asks for recovery.
            Self::RecoveryRequired => ActivationStartupStep::ActivationCommit,
            Self::Ready => ActivationStartupStep::ReadyHandoff,
            Self::Teardown(step) => step,
        }
    }
}
