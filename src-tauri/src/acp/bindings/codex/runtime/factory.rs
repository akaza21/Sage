use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{AdmittedCodexBinding, CodexProjectRootAuthority},
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    process::LaunchSpec,
};

use super::CodexRuntime;

/// Consumes the same admitted binding and single-root authority as the concrete
/// constructor. Only an admitted binding can supply factory authority; model
/// validation remains with the existing caller.
pub(crate) struct CodexRuntimeFactory {
    binding: Arc<AdmittedCodexBinding>,
    project_root: CodexProjectRootAuthority,
}

impl CodexRuntimeFactory {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
    ) -> Self {
        Self {
            binding,
            project_root,
        }
    }
}

impl RuntimeFactory for CodexRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        CodexRuntime::from_admitted_binding(
            self.binding,
            self.project_root,
            runtime_epoch,
            session_id,
        )
        .map(NativeRuntime::new)
    }
}

impl LaunchableRuntime for CodexRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        CodexRuntime::launch_spec(self)
    }
}
