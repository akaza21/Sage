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
    EgressToken, HarnessAdapter, RefusalDetail, ResourceMapper,
};

pub(crate) trait RuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError>;
}

pub(crate) trait LaunchableRuntime: HarnessAdapter + Send {
    /// Learns which session the launch opens, before `launch_spec`. A harness
    /// that names its session on the command line needs it; one that opens
    /// sessions over its protocol ignores it.
    fn bind_activation(&mut self, _mode: &crate::acp::runtime::ActivationMode) {}
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
    Option<(
        crate::acp::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::goal::GoalChannel>,
    )>,
    Option<(
        crate::acp::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::slash_commands::CommandChannel>,
    )>,
    Option<(
        crate::acp::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::usage::UsageChannel>,
    )>,
);

impl NativeRuntime {
    pub(crate) fn new(runtime: impl LaunchableRuntime + 'static) -> Self {
        Self(Box::new(runtime), None, None, None, None)
    }

    pub(crate) fn attach_commands(
        &mut self,
        owner: crate::acp::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::slash_commands::CommandChannel>,
    ) {
        self.3 = Some((owner, channel));
    }
    /// Lends the binding's usage cell to the lane once; a binding without
    /// one leaves the lane empty, which reads as unsupported.
    pub(crate) fn attach_usage(
        &mut self,
        owner: crate::acp::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::usage::UsageChannel>,
    ) {
        if let Some(cell) = self.0.usage() {
            channel.attach(owner.clone(), cell);
            self.4 = Some((owner, channel));
        }
    }
    pub(crate) fn attach_config(
        &mut self,
        owner: crate::acp::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::session_config::ConfigChannel>,
    ) {
        self.1 = Some((owner, channel));
    }

    pub(crate) fn attach_goal(
        &mut self,
        owner: crate::acp::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::goal::GoalChannel>,
    ) {
        self.2 = Some((owner, channel));
    }

    pub(crate) fn bind_activation(&mut self, mode: &crate::acp::runtime::ActivationMode) {
        self.0.bind_activation(mode);
    }

    pub(crate) fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        self.0.launch_spec()
    }
}

impl HarnessAdapter for NativeRuntime {
    fn receive_command_update(&mut self, frame: &Frame) -> Result<(), AdapterRuntimeError> {
        let result = self.0.receive_command_update(frame);
        if let Some((owner, channel)) = &self.3 {
            channel.update(owner, self.0.available_commands());
        }
        result
    }
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        self.0.available_commands()
    }
    fn discover_commands(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.discover_commands()
    }
    fn frames_repeat_terminal(&self, frames: &[Frame], resources: &dyn ResourceMapper) -> bool {
        self.0.frames_repeat_terminal(frames, resources)
    }
    fn discard_invalid_response(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.0.discard_invalid_response(frame)
    }
    fn model_config_hydration(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.model_config_hydration()
    }
    fn finish_model_config_hydration(&mut self) {
        self.0.finish_model_config_hydration();
    }
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let egress = self.0.goal_get();
        self.publish_goal();
        egress
    }
    fn goal_state(&self) -> Option<crate::acp::goal::GoalState> {
        self.0.goal_state()
    }
    fn goal_pending(&self) -> bool {
        self.0.goal_pending()
    }
    // Native RPCs (child reads, forks) must reach the concrete adapter: the
    // trait defaults would report no pending call and never a typed reply.
    fn native_rpc_pending(&self) -> bool {
        self.0.native_rpc_pending()
    }
    fn child_read_pending(&self) -> bool {
        self.0.child_read_pending()
    }
    fn read_child(
        &mut self,
        child_thread_id: &str,
        parent_thread_id: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.read_child(child_thread_id, parent_thread_id)
    }
    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.fork_session()
    }
    fn release_fork(
        &mut self,
        fork: &crate::acp::intent::ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.release_fork(fork, committed)
    }
    fn defer_fork_release(&mut self, fork: &crate::acp::intent::ExternalId, done: bool) -> bool {
        self.0.defer_fork_release(fork, done)
    }
    fn deferred_fork_release(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.deferred_fork_release()
    }
    fn accept_oversized_response(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        self.0.accept_oversized_response(id)
    }
    fn skips_oversize_frames(&self) -> bool {
        self.0.skips_oversize_frames()
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.0.take_native_rpc_reply()
    }
    fn usage(&self) -> Option<std::sync::Arc<crate::acp::usage::UsageCell>> {
        self.0.usage()
    }
    fn read_usage(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.read_usage()
    }
    fn abandon_passive_read(&mut self) {
        self.0.abandon_passive_read()
    }
    fn background_terminals(
        &mut self,
        action: &crate::acp::background_terminals::TerminalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.background_terminals(action)
    }
    fn mutate_goal(
        &mut self,
        action: crate::acp::goal::GoalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let egress = self.0.mutate_goal(action)?;
        self.publish_goal();
        Ok(egress)
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.0.session_config()
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        self.0.refusal_detail()
    }
    fn config_pending(&self) -> bool {
        self.0.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let (owner, channel) = self.1.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if owner != &intent.owner {
            return Err(AdapterRuntimeError::Operation);
        }
        channel
            .validate(intent)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let egress = self.0.configure(intent)?;
        if egress.is_none() {
            self.publish_config()?;
        }
        Ok(egress)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.0.capabilities()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let AdapterCommand::Prompt { prompt, .. } | AdapterCommand::Steer { prompt, .. } =
            &command
            && ((!prompt.resources.0.is_empty()
                && !self.capabilities().is_some_and(|c| c.prompt_resources))
                || (!prompt.images.is_empty()
                    && !self.capabilities().is_some_and(|c| c.prompt_images)))
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if let AdapterCommand::Prompt { prompt, .. } = &command
            && !prompt.action_admitted(self.capabilities().as_ref())
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if let AdapterCommand::Prompt { prompt, .. } = &command
            && let Some(revision) = prompt.command_revision
        {
            self.0
                .available_commands()
                .validate(revision, prompt.as_str())?;
        }
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
        let result = self.0.ingest(frame, resources);
        if let Some((owner, channel)) = &self.3 {
            channel.update(owner, self.0.available_commands());
        }
        let batch = result?;
        self.publish_config()?;
        if let Some((owner, channel)) = &self.2
            && let Some(goal) = self.0.goal_state()
        {
            channel.update(owner, goal);
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
        let result = self.0.resolve_egress(token, resolution);
        self.publish_goal();
        result
    }
}

impl NativeRuntime {
    fn publish_config(&self) -> Result<(), AdapterRuntimeError> {
        if let Some((owner, channel)) = &self.1 {
            match self.0.session_config() {
                Some(config) => channel.update(owner, config),
                None => channel.remove(owner),
            }
            .map_err(|_| AdapterRuntimeError::Frame)?;
        }
        Ok(())
    }

    fn publish_goal(&self) {
        if let Some((owner, channel)) = &self.2
            && let Some(goal) = self.0.goal_state()
        {
            channel.update(owner, goal);
        }
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        if let Some((owner, channel)) = &self.3 {
            channel.clear(owner);
        }
        if let Some((owner, channel)) = &self.4 {
            channel.clear(owner);
        }
    }
}
