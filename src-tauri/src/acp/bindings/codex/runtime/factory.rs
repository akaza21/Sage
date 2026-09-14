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
/// constructor. Admission and model validation remain with the existing caller.
pub(crate) struct CodexRuntimeFactory {
    pub(crate) binding: Arc<AdmittedCodexBinding>,
    pub(crate) project_root: CodexProjectRootAuthority,
}

impl RuntimeFactory for CodexRuntimeFactory {
    fn construct(
        self,
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
