use super::TreeCapacityLease;
use super::supervisor::{LaunchSpec, ZeroProcessToken};
use sage_process::error::LaunchError;

#[cfg(not(windows))]
use super::unix;
#[cfg(windows)]
use super::windows;

#[cfg(not(windows))]
pub(crate) use unix::{LaunchRecovery, RunningTree, StdinEndpoint, StdoutEndpoint, TreeControl};
#[cfg(windows)]
pub(crate) use windows::{LaunchRecovery, RunningTree, StdinEndpoint, StdoutEndpoint, TreeControl};

#[must_use = "launch failure owns either zero-process proof or recovery authority"]
pub(crate) enum LaunchFailure {
    ZeroProcess {
        error: LaunchError,
        token: ZeroProcessToken,
    },
    ConvergenceUnknown {
        error: LaunchError,
        recovery: Box<LaunchRecovery>,
    },
}

impl LaunchFailure {
    pub(super) fn zero_process(error: LaunchError, token: ZeroProcessToken) -> Self {
        Self::ZeroProcess { error, token }
    }

    pub(super) fn convergence_unknown(error: LaunchError, recovery: LaunchRecovery) -> Self {
        Self::ConvergenceUnknown {
            error,
            recovery: Box::new(recovery),
        }
    }

    pub(crate) fn error(&self) -> LaunchError {
        match self {
            Self::ZeroProcess { error, .. } | Self::ConvergenceUnknown { error, .. } => *error,
        }
    }

    pub(crate) fn into_zero_process(self) -> Result<ZeroProcessToken, Self> {
        match self {
            Self::ZeroProcess { token, .. } => Ok(token),
            unknown => Err(unknown),
        }
    }

    pub(crate) fn into_recovery(self) -> Result<(LaunchError, LaunchRecovery), Self> {
        match self {
            Self::ConvergenceUnknown { error, recovery } => Ok((error, *recovery)),
            zero => Err(zero),
        }
    }
}

impl std::fmt::Debug for LaunchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct(match self {
                Self::ZeroProcess { .. } => "ZeroProcessLaunchFailure",
                Self::ConvergenceUnknown { .. } => "ConvergenceUnknownLaunchFailure",
            })
            .field("error", &self.error())
            .finish_non_exhaustive()
    }
}

pub(crate) struct PlatformSupervisor;

impl PlatformSupervisor {
    pub(crate) fn launch(
        spec: &LaunchSpec,
        lease: TreeCapacityLease,
    ) -> Result<RunningTree, LaunchFailure> {
        #[cfg(windows)]
        {
            windows::launch(spec, lease)
        }
        #[cfg(not(windows))]
        {
            unix::launch(spec, lease)
        }
    }
}
