use std::sync::Arc;

use crate::{
    acp::{
        bindings::{
            adapter::AdapterRuntimeError,
            runtime::{NativeRuntime, RuntimeFactory},
        },
        contract::{RuntimeEpoch, SageAgentSessionId},
        mcp::McpAdmission,
    },
    project_registry::ProjectRootAuthority,
};

use super::{super::AdmittedStandardAcpBinding, StandardAcpRuntime};

/// Where a standard ACP runtime re-proves its admitted binding before launch:
/// OpenCode's discovered install, or a user-defined agent's settings.
pub(crate) trait StandardAcpBindingSource: Send + Sync {
    /// The binding as admitted now, revalidated against the disk.
    fn refresh_binding(&self) -> Option<Arc<AdmittedStandardAcpBinding>>;

    /// Keeps the sign-in notice the agent's methods produced, so the harness
    /// list can show it. Curated sources already carry their own guidance.
    fn record_sign_in(&self, _notice: &str) {}

    /// A session opened, so any recorded sign-in notice is stale.
    fn session_ready(&self) {}
}

pub(crate) struct StandardAcpRuntimeFactory {
    source: Arc<dyn StandardAcpBindingSource>,
    binding: Arc<AdmittedStandardAcpBinding>,
    project_root: ProjectRootAuthority,
    mcp: McpAdmission,
}

impl StandardAcpRuntimeFactory {
    #[cfg(test)]
    pub(crate) fn from_admitted_binding(
        source: Arc<dyn StandardAcpBindingSource>,
        binding: Arc<AdmittedStandardAcpBinding>,
        project_root: ProjectRootAuthority,
    ) -> Self {
        Self::with_mcp(source, binding, project_root, McpAdmission::default())
    }

    pub(crate) fn with_mcp(
        source: Arc<dyn StandardAcpBindingSource>,
        binding: Arc<AdmittedStandardAcpBinding>,
        project_root: ProjectRootAuthority,
        mcp: McpAdmission,
    ) -> Self {
        Self {
            source,
            binding,
            project_root,
            mcp,
        }
    }
}

impl RuntimeFactory for StandardAcpRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        let Self {
            source,
            binding,
            project_root,
            mcp,
        } = *self;
        Ok(NativeRuntime::new(StandardAcpRuntime::new(
            binding.profile(),
            source,
            binding.binding_digest(),
            runtime_epoch,
            session_id,
            project_root,
            mcp,
        )))
    }
}
