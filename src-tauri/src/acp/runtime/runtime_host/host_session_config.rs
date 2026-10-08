use super::*;

impl RuntimeHost {
    pub(crate) fn native_rpc(
        &self,
        intent: crate::acp::goal::NativeRpcIntent,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::NativeRpc(intent))
    }
    pub(crate) fn configure_session(
        &self,
        intent: crate::acp::session_config::ConfigIntent,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ConfigureSession(intent))
    }
}
