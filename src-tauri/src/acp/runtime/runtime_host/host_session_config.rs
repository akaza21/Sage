use super::*;

impl RuntimeHost {
    pub(crate) fn configure_session(
        &self,
        intent: crate::acp::session_config::ConfigIntent,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ConfigureSession(intent))
    }
}
