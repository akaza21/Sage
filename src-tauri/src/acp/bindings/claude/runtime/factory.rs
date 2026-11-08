//! Builds a Claude runtime for one activation.

use super::*;

pub(crate) struct ClaudeRuntimeFactory {
    binding: Arc<AdmittedClaudeBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    mcp: McpAdmission,
    /// The session's durable binding scope when it differs from the admitted
    /// digest: a proven-compatible drift continues the session's lineage
    /// under a new exact build. `None` stamps the admitted digest.
    session_scope: Option<[u8; 32]>,
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
            session_scope: None,
        }
    }

    /// Continues a session whose durable binding scope predates this exact
    /// build. Only a classified compatible drift constructs it.
    pub(crate) fn continuing(
        binding: Arc<AdmittedClaudeBinding>,
        project_root: crate::project_registry::ProjectRootAuthority,
        mcp: McpAdmission,
        session_scope: [u8; 32],
    ) -> Self {
        Self {
            session_scope: Some(session_scope),
            ..Self::with_mcp(binding, project_root, mcp)
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
            session_scope,
        } = *self;
        let mut runtime = ClaudeRuntime::from_admitted_binding(
            binding,
            session_scope,
            runtime_epoch,
            session_id,
        )?;
        runtime.mcp = mcp;
        runtime.pending_project_root = Some(project_root);
        Ok(NativeRuntime::new(runtime))
    }
}
