//! Launching Claude Code for one admitted session.

use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        claude::admission::AdmittedClaudeBinding,
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
        standard_acp::{AcpLaunchAuthority, ClaudeLaunch},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    mcp::McpAdmission,
    process::LaunchSpec,
};

use super::ClaudeRuntime;

impl ClaudeRuntime {
    fn launch_spec(
        &mut self,
        project_root: crate::project_registry::ProjectRootAuthority,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        let binding = self
            .launch_binding
            .as_deref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if binding.is_fixture() {
            return binding.fixture_launch(self.runtime_epoch.get(), project_root, &self.mcp);
        }
        let candidate = binding.candidate();
        let environment = crate::acp::process::BoundedEnvironment::new(
            binding
                .launch_environment()
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        AcpLaunchAuthority::for_claude(ClaudeLaunch {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch.get(),
            runtime_path: candidate.runtime_path.clone(),
            runtime_sha256: candidate.runtime_sha256.clone(),
            runtime_byte_len: std::fs::metadata(&candidate.runtime_path)
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?
                .len(),
            entrypoint: candidate.artifact_path.clone(),
            environment,
        })?
        .into_launch_spec(project_root)
    }
}

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

impl LaunchableRuntime for ClaudeRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let project_root = self
            .pending_project_root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        ClaudeRuntime::launch_spec(self, project_root)
    }
}
