//! Native-only runtime construction and ownership boundary.
//!
//! The service retains one runtime through activation, live execution, and
//! failed-start recovery. Factories provide adapter/launch authority only;
//! they do not own a scheduler, operation gate, process lease, or supervisor.

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::CapabilitySnapshotV1,
    process::LaunchSpec,
    transport::Frame,
};

use super::adapter::{
    AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
    EgressToken, HarnessAdapter, ResourceMapper,
};

pub(crate) trait RuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError>;
}

pub(crate) trait LaunchableRuntime: HarnessAdapter + Send {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError>;
}

/// Owns the exact adapter and its native launch authority, including while a
/// failed activation driver is retained for recovery. No serialization or
/// Debug implementation: the enclosed authority can contain native paths.
pub(crate) struct NativeRuntime(
    Box<dyn LaunchableRuntime>,
    Option<(
        crate::acp::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::session_config::ConfigChannel>,
    )>,
);

impl NativeRuntime {
    pub(crate) fn new(runtime: impl LaunchableRuntime + 'static) -> Self {
        Self(Box::new(runtime), None)
    }

    pub(crate) fn attach_config(
        &mut self,
        owner: crate::acp::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::session_config::ConfigChannel>,
    ) {
        self.1 = Some((owner, channel));
    }

    pub(crate) fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        self.0.launch_spec()
    }
}

impl HarnessAdapter for NativeRuntime {
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.0.session_config()
    }
    fn config_pending(&self) -> bool {
        self.0.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let (owner, channel) = self.1.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if owner != &intent.owner {
            return Err(AdapterRuntimeError::Operation);
        }
        channel
            .validate(intent)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.0.configure(intent)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.0.capabilities()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.command(command)
    }

    fn cancel_permission(
        &mut self,
        request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.cancel_permission(request_id)
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let batch = self.0.ingest(frame, resources)?;
        if let Some((owner, channel)) = &self.1 {
            match self.0.session_config() {
                Some(config) => channel
                    .update(owner, config)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                None => channel
                    .remove(owner)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            }
        }
        Ok(batch)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.0.commit_mandatory_egress(token)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.0.resolve_egress(token, resolution)
    }
}
