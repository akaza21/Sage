use crate::acp::bindings::adapter::AdapterRuntimeError;

use super::PumpJournalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NonfatalRefusal {
    #[error("the harness rejected the session configuration mutation")]
    ConfigMutation,
    #[error("the harness requires sign-in before it can continue")]
    SignIn,
}

impl NonfatalRefusal {
    pub(crate) fn from_adapter(error: &AdapterRuntimeError) -> Option<Self> {
        match error {
            AdapterRuntimeError::ConfigMutationRejected => Some(Self::ConfigMutation),
            AdapterRuntimeError::SignInRequired => Some(Self::SignIn),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterPumpError {
    #[error("adapter event or command has stale or foreign intent")]
    Intent,
    #[error("there is no exact active Turn for cancellation")]
    NoActiveTurn,
    #[error("adapter runtime is tearing down fail closed")]
    TeardownRequired,
    #[error("adapter batch exceeds a frozen count or actual-byte limit")]
    BatchCapacity,
    #[error("adapter egress is backpressured at its bounded capacity")]
    Backpressure,
    #[error("durable adapter journal failed")]
    Journal(#[from] PumpJournalError),
    #[error("projection ownership changed; authoritative bootstrap is required")]
    BootstrapRequired,
    #[error("projection delivery failed; authoritative recovery is required")]
    RecoveryRequired,
    #[error("adapter frame is malformed")]
    Frame,
    #[error(transparent)]
    Refused(#[from] NonfatalRefusal),
    #[error(transparent)]
    Adapter(#[from] AdapterRuntimeError),
}
