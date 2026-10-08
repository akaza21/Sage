use super::*;

impl RuntimeHost {
    pub(crate) fn native_rpc(
        &self,
        intent: crate::acp::goal::NativeRpcIntent,
    ) -> Result<crate::acp::goal::NativeRpcReply, HostError> {
        match self.submit(HostCommand::NativeRpc(intent))? {
            HostReply::NativeRpc(reply) => Ok(reply),
            _ => Err(HostError::State),
        }
    }
    pub(crate) fn configure_session(
        &self,
        intent: crate::acp::session_config::ConfigIntent,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ConfigureSession(intent))
    }
}
