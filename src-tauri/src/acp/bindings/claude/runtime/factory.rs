//! Builds a Claude runtime for one activation.

use super::*;

pub(crate) struct ClaudeRuntimeFactory {
    binding: Arc<AdmittedClaudeBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    mcp: McpAdmission,
    requested_access_mode: Option<crate::settings_store::SessionAccessMode>,
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
            requested_access_mode: None,
        }
    }

    /// The access mode the session asked for, which decides the native mode a
    /// new session starts in; without one the binding's default applies.
    #[allow(
        dead_code,
        reason = "activation does not carry the requested access mode yet"
    )]
    pub(crate) fn with_requested_access_mode(
        mut self,
        mode: Option<crate::settings_store::SessionAccessMode>,
    ) -> Self {
        self.requested_access_mode = mode;
        self
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
            requested_access_mode,
        } = *self;
        let mut runtime = ClaudeRuntime::from_admitted_binding(binding, runtime_epoch, session_id)?;
        runtime.mcp = mcp;
        runtime.requested_access_mode = requested_access_mode;
        runtime.pending_project_root = Some(project_root);
        Ok(NativeRuntime::new(runtime))
    }
}
