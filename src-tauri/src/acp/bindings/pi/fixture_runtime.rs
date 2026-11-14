use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        runtime::{NativeRuntime, RuntimeFactory},
    },
    process::LaunchSpec,
};
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};

use super::{PiLaunch, PiRuntime, fixture_admission::PiFixtureBinding};

pub(crate) struct PiFixtureRuntimeFactory {
    pub(crate) binding: Arc<PiFixtureBinding>,
    pub(crate) root: crate::project_registry::ProjectRootAuthority,
    pub(crate) mcp: crate::acp::mcp::McpAdmission,
}

impl RuntimeFactory for PiFixtureRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        if !self.mcp.is_empty() {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        let mut adapter =
            PiRuntime::from_fixture_binding(&self.binding, runtime_epoch, session_id)?;
        if self.binding.bridge_installed() {
            adapter.enable_bridge_for_root(&self.root)?;
        }
        Ok(NativeRuntime::new(adapter.with_launch(FixturePi {
            binding: self.binding,
            root: Some(self.root),
            mcp: self.mcp,
        })))
    }
}

struct FixturePi {
    binding: Arc<PiFixtureBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
    mcp: crate::acp::mcp::McpAdmission,
}

impl PiLaunch for FixturePi {
    fn launch_spec(&mut self, epoch: RuntimeEpoch) -> Result<LaunchSpec, AdapterRuntimeError> {
        let root = self
            .root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        self.binding.launch(epoch.get(), root, &self.mcp)
    }
}
