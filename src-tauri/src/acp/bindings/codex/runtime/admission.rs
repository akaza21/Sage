use std::sync::Arc;

use super::{CodexRuntime, RuntimeLaunchAuthority, authority::CodexProjectRootAuthority};
use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, codex::admission::AdmittedCodexBinding},
    contract::{RuntimeEpoch, SageAgentSessionId},
};

impl CodexRuntime {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let mut runtime = Self::new(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        runtime.launch_authority = RuntimeLaunchAuthority::admitted(binding, project_root)?;
        Ok(runtime)
    }

    pub(crate) fn launch_environment(&self) -> Result<&[(String, String)], AdapterRuntimeError> {
        self.launch_authority
            .binding()
            .map(|binding| binding.environment().entries())
    }
}
