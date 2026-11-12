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

impl CodexLaunchSetupError {
    /// The launch step that failed, as the chat names it.
    pub(crate) fn step(self) -> &'static str {
        match self {
            Self::ActiveOperation => "claiming the launch",
            Self::ControlRegistration => "registering session control",
            Self::DriverAssembly => "assembling the process driver",
            Self::HostActivation => "activating the runtime host",
            Self::HostAssembly => "assembling the runtime host",
            Self::Mailbox => "opening the driver mailbox",
            Self::PumpAssembly => "assembling the adapter pump",
            Self::RuntimeSlot => "claiming a live runtime slot",
            Self::RunnerStartSpawn => "starting the harness process",
            Self::RunnerStartStdoutReader => "reading the harness output",
            Self::RunnerStartProcessExitWatcher => "watching the harness process",
            Self::RunnerStartInitialize => "initializing the harness",
            Self::RunnerStartSessionNew => "starting a new harness session",
            Self::RunnerStartSessionResume => "resuming the harness session",
            Self::RunnerStartSessionLoad => "loading the harness session",
            Self::RunnerStartPermission => "settling the harness's launch permission",
            Self::RunnerStartReadyHandoff => "handing the ready session over",
            Self::RunnerStartTimeout => "waiting for the harness to start",
            Self::RunnerStartUnexpectedMethod => "reading an unexpected harness request",
            Self::RunnerStartUnexpected => "starting the harness",
        }
    }
}
