#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationStartupStep {
    Initialize,
    SessionNew,
    SessionResume,
    SessionLoad,
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
