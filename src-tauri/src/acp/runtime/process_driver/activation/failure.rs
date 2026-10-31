use super::*;

impl<A, J, R, T, C> ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + crate::acp::runtime::ActivationTransport,
    C: ProcessTreeControl,
{
    pub(super) fn pump_error(&mut self, error: ActivationPumpError) -> ActivationDriverError {
        if matches!(
            error,
            ActivationPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::ResumeRefused
            )
        ) {
            self.pending_exit = None;
        }
        if let Some(error) = self.latch_pending_exit() {
            return error;
        }
        if matches!(
            error,
            ActivationPumpError::BootstrapRequired | ActivationPumpError::RecoveryRequired
        ) {
            self.state = State::ProjectionRecovery;
            return ActivationDriverError::ProjectionRecoveryRequired;
        }
        self.latch(match error {
            ActivationPumpError::Frame | ActivationPumpError::Intent => {
                TeardownCause::ProtocolFailure
            }
            ActivationPumpError::RefusedVersion(observed) => {
                TeardownCause::RefusedVersion { observed }
            }
            ActivationPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::ResumeRefused,
            ) => TeardownCause::ResumeRefused,
            ActivationPumpError::Adapter(
                crate::acp::bindings::adapter::AdapterRuntimeError::MandatoryDenyTeardown,
            ) => TeardownCause::MandatoryDenyFailure,
            _ => TeardownCause::PumpFailure,
        })
    }
}
