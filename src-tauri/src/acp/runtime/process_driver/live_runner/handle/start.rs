use std::{
    sync::{Arc, Weak, mpsc},
    time::Instant,
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::StdoutEndpoint,
    runtime::{
        ActivationProcessDriver, ActivationProjectionRecoveryHandoff, ActivationTransport,
        ActivationZeroProcessHandoff, CoordinatorActivationJournal, HostError,
        ProjectIntentAuthority, RuntimeHost,
    },
};

use crate::acp::runtime::process_driver::{
    ActivationStartupStep, DriverMailbox, FirstTurnWorkerContext, StdinTransport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunnerStartSubstep {
    Spawn,
    StdoutReader,
    ProcessExitWatcher,
    Initialize,
    SessionNew,
    SessionResume,
    SessionLoad,
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    Permission,
    ReadyHandoff,
    Timeout,
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    UnexpectedMethod,
    Unexpected,
}

impl From<ActivationStartupStep> for RunnerStartSubstep {
    fn from(value: ActivationStartupStep) -> Self {
        match value {
            ActivationStartupStep::Initialize => Self::Initialize,
            ActivationStartupStep::SessionNew => Self::SessionNew,
            ActivationStartupStep::SessionResume => Self::SessionResume,
            ActivationStartupStep::SessionLoad => Self::SessionLoad,
            ActivationStartupStep::ReadyHandoff => Self::ReadyHandoff,
        }
    }
}

// Recovery variants deliberately retain their unique driver/handoff by value;
// callers must consume exactly one authority instead of sharing indirection.
#[allow(
    dead_code,
    reason = "production callers only drop these; tests inspect the fields"
)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ActivationStartFailure<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    Teardown {
        handoff: ActivationZeroProcessHandoff<
            A,
            CoordinatorActivationJournal,
            R,
            crate::acp::process::ZeroProcessToken,
        >,
        error: HostError,
        substep: RunnerStartSubstep,
    },
    Projection {
        handoff: ActivationProjectionRecoveryHandoff<
            A,
            CoordinatorActivationJournal,
            R,
            crate::acp::process::ZeroProcessToken,
        >,
        error: HostError,
        substep: RunnerStartSubstep,
    },
    Retained {
        driver: ActivationProcessDriver<
            A,
            CoordinatorActivationJournal,
            R,
            T,
            crate::acp::process::TreeControl,
        >,
        error: HostError,
        substep: RunnerStartSubstep,
    },
    OwnedByRunner {
        error: HostError,
        substep: RunnerStartSubstep,
    },
}

pub(crate) struct RunInput<A, R, T, Auth>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
    Auth: ProjectIntentAuthority,
{
    pub(crate) context: FirstTurnWorkerContext<Auth>,
    pub(crate) stdout: StdoutEndpoint,
    pub(crate) host: Weak<RuntimeHost>,
    pub(crate) mailbox: Arc<DriverMailbox>,
    pub(crate) control: crate::acp::process::TreeControl,
    pub(crate) ready: mpsc::SyncSender<Result<(), ActivationStartFailure<A, R, T>>>,
    pub(crate) cleanup_deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveRunnerExit {
    Shutdown,
    Failed,
}
