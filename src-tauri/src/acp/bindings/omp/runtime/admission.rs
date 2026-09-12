use super::*;

impl OmpRuntime {
    pub(crate) fn from_binding(
        binding: &ResearchBinding,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let _ = (binding_digest, runtime_epoch, session_id);
        binding
            .require_launch_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Err(AdapterRuntimeError::LaunchUnavailable)
    }
}
