use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
    contract::{RuntimeEpoch, SageAgentSessionId},
};

use super::CodexRuntime;

impl CodexRuntime {
    pub(crate) fn from_admitted_binding(
        binding: &AdmittedCodexBinding,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        ))
    }
}
