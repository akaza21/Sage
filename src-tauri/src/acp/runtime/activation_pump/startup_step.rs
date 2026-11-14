/// The activation step a startup is at, or failed closed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationStartupStep {
    Initialize,
    SessionNew,
    SessionResume,
    SessionLoad,
    /// Recording the ready session in the store.
    ActivationCommit,
    ReadyHandoff,
}

impl ActivationStartupStep {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::SessionNew => "session/new",
            Self::SessionResume => "session/resume",
            Self::SessionLoad => "session/load",
            Self::ActivationCommit => "activation-commit",
            Self::ReadyHandoff => "ready-handoff",
        }
    }
}
