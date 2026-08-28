#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexLaunchSetupError {
    ActiveOperation,
    ControlRegistration,
    DriverAssembly,
    HostActivation,
    HostAssembly,
    Mailbox,
    PumpAssembly,
    RuntimeSlot,
    RunnerStartSpawn,
    RunnerStartStdoutReader,
    RunnerStartProcessExitWatcher,
    RunnerStartInitialize,
    RunnerStartSessionNew,
    RunnerStartSessionResume,
    RunnerStartSessionLoad,
    RunnerStartPermission,
    RunnerStartReadyHandoff,
    RunnerStartTimeout,
    RunnerStartUnexpectedMethod,
    RunnerStartUnexpected,
}

impl From<crate::acp::runtime::RunnerStartSubstep> for CodexLaunchSetupError {
    fn from(value: crate::acp::runtime::RunnerStartSubstep) -> Self {
        use crate::acp::runtime::RunnerStartSubstep;

        match value {
            RunnerStartSubstep::Spawn => Self::RunnerStartSpawn,
            RunnerStartSubstep::StdoutReader => Self::RunnerStartStdoutReader,
            RunnerStartSubstep::ProcessExitWatcher => Self::RunnerStartProcessExitWatcher,
            RunnerStartSubstep::Initialize => Self::RunnerStartInitialize,
            RunnerStartSubstep::SessionNew => Self::RunnerStartSessionNew,
            RunnerStartSubstep::SessionResume => Self::RunnerStartSessionResume,
            RunnerStartSubstep::SessionLoad => Self::RunnerStartSessionLoad,
            RunnerStartSubstep::Permission => Self::RunnerStartPermission,
            RunnerStartSubstep::ReadyHandoff => Self::RunnerStartReadyHandoff,
            RunnerStartSubstep::Timeout => Self::RunnerStartTimeout,
            RunnerStartSubstep::UnexpectedMethod => Self::RunnerStartUnexpectedMethod,
            RunnerStartSubstep::Unexpected => Self::RunnerStartUnexpected,
        }
    }
}
