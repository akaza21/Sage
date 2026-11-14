use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{AdmittedCodexBinding, CodexProjectRootAuthority},
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
    },
    process::LaunchSpec,
};
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};

use super::CodexRuntime;

/// Only an admitted binding can supply factory authority; model validation
/// remains with the existing caller.
pub(crate) struct CodexRuntimeFactory {
    binding: Arc<AdmittedCodexBinding>,
    project_root: CodexProjectRootAuthority,
    mcp: crate::acp::mcp::McpAdmission,
}

impl CodexRuntimeFactory {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
    ) -> Self {
        Self::with_mcp(binding, project_root, Default::default())
    }

    pub(crate) fn with_mcp(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        mcp: crate::acp::mcp::McpAdmission,
    ) -> Self {
        Self {
            binding,
            project_root,
            mcp,
        }
    }
}

impl RuntimeFactory for CodexRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        CodexRuntime::from_admitted_binding_with_mcp(
            self.binding,
            self.project_root,
            runtime_epoch,
            session_id,
            self.mcp,
        )
        .map(NativeRuntime::new)
    }
}

impl LaunchableRuntime for CodexRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        CodexRuntime::launch_spec(self)
    }
}
