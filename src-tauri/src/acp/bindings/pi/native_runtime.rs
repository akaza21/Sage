//! Launchable Pi runtime with read-only fallback and optional curated bridge.

use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        runtime::{NativeRuntime, RuntimeFactory},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    process::LaunchSpec,
};

use super::{PiLaunch, PiRuntime, native_admission::AdmittedPiBinding};

pub(crate) struct PiNativeRuntimeFactory {
    pub(crate) binding: Arc<AdmittedPiBinding>,
    pub(crate) root: crate::project_registry::ProjectRootAuthority,
    pub(crate) mcp: crate::acp::mcp::McpAdmission,
}

impl RuntimeFactory for PiNativeRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        // Pi has no MCP surface: an offered browser MCP descriptor is not
        // forwarded, and no browser tools are advertised to the model.
        drop(self.mcp);
        self.binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let mut adapter = PiRuntime::from_native_binding(
            self.binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        if super::bridge::install::installed(self.binding.harness_version()).is_ok() {
            adapter.enable_bridge_for_root(&self.root)?;
        }
        Ok(NativeRuntime::new(adapter.with_launch(InstalledPi {
            binding: self.binding,
            root: Some(self.root),
        })))
    }
}

/// The installed Pi release, launched in the chat's project root.
struct InstalledPi {
    binding: Arc<AdmittedPiBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
}

impl PiLaunch for InstalledPi {
    fn launch_spec(&mut self, epoch: RuntimeEpoch) -> Result<LaunchSpec, AdapterRuntimeError> {
        let root = self
            .root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        self.binding.launch_spec(epoch.get(), root)
    }
}
