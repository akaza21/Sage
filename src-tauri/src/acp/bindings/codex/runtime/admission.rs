use std::sync::Arc;

use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
    contract::{RuntimeEpoch, SageAgentSessionId},
};

use super::CodexRuntime;

impl CodexRuntime {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let mut runtime = Self::new(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        runtime.admission = Some(binding);
        Ok(runtime)
    }

    pub(crate) fn launch_environment(&self) -> Result<&[(String, String)], AdapterRuntimeError> {
        self.admission
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)
            .map(|binding| binding.environment().entries())
    }
}
