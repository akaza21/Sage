use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::{
            AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
            EgressToken, HarnessAdapter, ResourceMapper,
        },
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
    },
    contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::CapabilitySnapshotV1,
    transport::Frame,
};

use super::{PiRuntime, fixture_admission::PiFixtureBinding};

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
        let adapter = PiRuntime::from_fixture_binding(&self.binding, runtime_epoch, session_id)?;
        Ok(NativeRuntime::new(PiFixtureRuntime {
            adapter,
            binding: self.binding,
            root: Some(self.root),
            epoch: runtime_epoch,
            mcp: self.mcp,
        }))
    }
}

struct PiFixtureRuntime {
    adapter: PiRuntime,
    binding: Arc<PiFixtureBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
    epoch: RuntimeEpoch,
    mcp: crate::acp::mcp::McpAdmission,
}

impl LaunchableRuntime for PiFixtureRuntime {
    fn launch_spec(&mut self) -> Result<crate::acp::process::LaunchSpec, AdapterRuntimeError> {
        let root = self
            .root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        self.binding.launch(self.epoch.get(), root, &self.mcp)
    }
}

impl HarnessAdapter for PiFixtureRuntime {
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        self.adapter.available_commands()
    }
    fn discover_commands(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.adapter.discover_commands()
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.adapter.session_config()
    }
    fn config_pending(&self) -> bool {
        self.adapter.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.adapter.configure(intent)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.adapter.capabilities()
    }
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.adapter.command(command)
    }
    fn cancel_permission(
        &mut self,
        request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.adapter.cancel_permission(request_id)
    }
    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adapter.ingest(frame, resources)
    }
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.adapter.commit_mandatory_egress(token)
    }
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.adapter.resolve_egress(token, resolution)
    }
}
