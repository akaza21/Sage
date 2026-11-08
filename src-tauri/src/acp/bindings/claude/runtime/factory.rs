//! Builds a Claude runtime for one activation.

use super::*;

pub(crate) struct ClaudeRuntimeFactory {
    binding: Arc<AdmittedClaudeBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    mcp: McpAdmission,
}

impl ClaudeRuntimeFactory {
    #[cfg(test)]
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedClaudeBinding>,
        project_root: crate::project_registry::ProjectRootAuthority,
    ) -> Self {
        Self::with_mcp(binding, project_root, McpAdmission::default())
    }

    pub(crate) fn with_mcp(
        binding: Arc<AdmittedClaudeBinding>,
        project_root: crate::project_registry::ProjectRootAuthority,
        mcp: McpAdmission,
    ) -> Self {
        Self {
            binding,
            project_root,
            mcp,
        }
    }
}

impl RuntimeFactory for ClaudeRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        let Self {
            binding,
            project_root,
            mcp,
        } = *self;
        let mut runtime = ClaudeRuntime::from_admitted_binding(binding, runtime_epoch, session_id)?;
        runtime.mcp = mcp;
        runtime.pending_project_root = Some(project_root);
        Ok(NativeRuntime::new(runtime))
    }
}
