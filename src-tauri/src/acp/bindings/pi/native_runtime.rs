use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        runtime::{NativeRuntime, RuntimeFactory},
    },
    process::LaunchSpec,
};
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};

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
        // Pi loads its configured MCP extension. Sage's browser MCP descriptor
        // is not forwarded until this binding supports that admission surface.
        drop(self.mcp);
        self.binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let mut adapter = PiRuntime::from_native_binding(
            self.binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        super::bridge::install::installed(self.binding.harness_version())
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        adapter.enable_bridge_for_root(&self.root)?;
        Ok(NativeRuntime::new(adapter.with_launch(InstalledPi {
            binding: self.binding,
            root: Some(self.root),
        })))
    }
}

struct InstalledPi {
    binding: Arc<AdmittedPiBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
}

impl PiLaunch for InstalledPi {
    fn revalidate_title(&self) -> Result<(), AdapterRuntimeError> {
        self.binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)
    }
    fn launch_spec(&mut self, epoch: RuntimeEpoch) -> Result<LaunchSpec, AdapterRuntimeError> {
        let root = self
            .root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        self.binding.launch_spec(epoch.get(), root)
    }
}
