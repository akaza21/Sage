//! Launchable Pi runtime with read-only fallback and optional curated bridge.

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
        let mut adapter = PiRuntime::from_native_binding(
            self.binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        if super::bridge::install::installed(self.binding.harness_version()).is_ok() {
            adapter.enable_bridge_for_root(&self.root)?;
        }
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
    fn native_rpc_pending(&self) -> bool {
        self.adapter.native_rpc_pending()
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.adapter.take_native_rpc_reply()
    }
    fn usage(&self) -> Option<Arc<crate::acp::usage::UsageCell>> {
        self.adapter.usage()
    }
    fn read_usage(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.adapter.read_usage()
    }
    fn abandon_passive_read(&mut self) {
        self.adapter.abandon_passive_read();
    }
    fn session_control(
        &mut self,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.adapter.session_control(control)
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
