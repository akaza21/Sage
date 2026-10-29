//! Launchable production Pi runtime: the read-only RPC translator plus the
//! curated native launch of an admitted Pi package (AP-B B3).

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

use super::{PiRuntime, native_admission::AdmittedPiBinding};

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
        let adapter = PiRuntime::from_native_binding(
            self.binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        Ok(NativeRuntime::new(PiNativeRuntime {
            adapter,
            binding: self.binding,
            root: Some(self.root),
            epoch: runtime_epoch,
        }))
    }
}

struct PiNativeRuntime {
    adapter: PiRuntime,
    binding: Arc<AdmittedPiBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
    epoch: RuntimeEpoch,
}

impl LaunchableRuntime for PiNativeRuntime {
    fn launch_spec(&mut self) -> Result<crate::acp::process::LaunchSpec, AdapterRuntimeError> {
        let root = self
            .root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        self.binding.launch_spec(self.epoch.get(), root)
    }
}

impl HarnessAdapter for PiNativeRuntime {
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
