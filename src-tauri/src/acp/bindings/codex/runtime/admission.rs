use std::sync::Arc;

use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
    contract::{RuntimeEpoch, SageAgentSessionId},
};
use crate::project_registry::ProjectRootAuthority;

use super::{CodexRuntime, RuntimeProjectRoot};

impl CodexRuntime {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: &ProjectRootAuthority,
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
        runtime.project_root = Some(RuntimeProjectRoot {
            project_id: project_root.project_id().clone(),
            generation: project_root.generation(),
            root_id: project_root.root_id().clone(),
        });
        Ok(runtime)
    }

    pub(crate) fn launch_environment(&self) -> Result<&[(String, String)], AdapterRuntimeError> {
        self.admission
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)
            .map(|binding| binding.environment().entries())
    }
}
