//! Factories provide adapter/launch authority only; they do not own a scheduler, operation
//! gate, process lease, or supervisor.

use crate::acp::{process::LaunchSpec, transport::Frame};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::CapabilitySnapshotV1,
    session_config::ConfigMutation,
};

mod state;

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
    /// Learns the choices stored for a new session before it opens it. A
    /// harness that reports no value for an option until a prompt carries
    /// one (Codex's thinking level on a new thread) starts with the stored
    /// value; any other gets its choices from the restore once it reports.
    fn bind_launch_choices(&mut self, _choices: &[ConfigMutation]) {}
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError>;
}

/// No serialization or Debug implementation: the enclosed authority can contain native paths.
pub(crate) struct NativeRuntime(
    Box<dyn LaunchableRuntime>,
    Option<(
        sage_acp_protocol::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::session_config::ConfigChannel>,
    )>,
    Option<(
        sage_acp_protocol::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::goal::GoalChannel>,
    )>,
    Option<(
        sage_acp_protocol::scheduler::RuntimeOwner,
        std::sync::Arc<crate::acp::slash_commands::CommandChannel>,
    )>,
    Option<(
        sage_acp_protocol::scheduler::RuntimeOwner,
        std::sync::Arc<sage_acp_protocol::usage::UsageChannel>,
    )>,
    Option<crate::acp::service::chat_title::TitleScheduler>,
);

impl NativeRuntime {
    pub(crate) fn attach_titles(
        &mut self,
        titles: crate::acp::service::chat_title::TitleScheduler,
    ) {
        self.5 = Some(titles);
    }

    pub(crate) fn new(runtime: impl LaunchableRuntime + 'static) -> Self {
        Self(Box::new(runtime), None, None, None, None, None)
    }

    pub(crate) fn attach_commands(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::slash_commands::CommandChannel>,
    ) {
        self.3 = Some((owner, channel));
    }
    /// Lends the binding's usage cell to the lane once; a binding without
    /// one leaves the lane empty, which reads as unsupported.
    pub(crate) fn attach_usage(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        channel: std::sync::Arc<sage_acp_protocol::usage::UsageChannel>,
    ) {
        if let Some(cell) = self.0.usage() {
            channel.attach(owner.clone(), cell);
            self.4 = Some((owner, channel));
        }
    }
    pub(crate) fn attach_config(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::session_config::ConfigChannel>,
    ) {
        self.1 = Some((owner, channel));
    }

    pub(crate) fn attach_goal(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::goal::GoalChannel>,
    ) {
        self.2 = Some((owner, channel));
    }

    pub(crate) fn bind_activation(&mut self, mode: &crate::acp::runtime::ActivationMode) {
        self.0.bind_activation(mode);
    }

    pub(crate) fn bind_launch_choices(&mut self, choices: &[ConfigMutation]) {
        self.0.bind_launch_choices(choices);
    }

    pub(crate) fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        self.0.launch_spec()
    }
}

impl HarnessAdapter for NativeRuntime {
    fn foreground_turn(&self) -> Option<crate::acp::service::chat_title::ForegroundLease> {
        self.5.as_ref().map(|titles| titles.foreground_turn())
    }
    fn start_title(&mut self, input: &str) -> Result<(), AdapterRuntimeError> {
        self.0.start_title(input)
    }
    fn title_requires_teardown(&self) -> bool {
        self.0.title_requires_teardown()
    }
    fn take_title_result(&mut self) -> Option<Option<String>> {
        self.0.take_title_result()
    }

    fn title_completed(&mut self, intent: &crate::acp::runtime::PumpIntent) {
        if let Some(titles) = &mut self.5 {
            titles.completed(intent, &mut *self.0);
        }
    }
    fn poll_title(&mut self) -> Option<AdapterEgress> {
        match &mut self.5 {
            Some(titles) => titles.poll(&mut *self.0),
            None => self.0.poll_title(),
        }
    }
    fn cancel_title(&mut self) {
        if let Some(titles) = &mut self.5 {
            titles.cancel(&mut *self.0);
        } else {
            self.0.cancel_title();
        }
    }
    fn title_pending(&self) -> bool {
        self.5
            .as_ref()
            .map_or_else(|| self.0.title_pending(), |titles| titles.pending())
    }

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
        let batch = self.0.discard_invalid_response(frame)?;
        self.publish_config()?;
        Ok(batch)
    }
    fn model_config_hydration(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let egress = self.0.model_config_hydration()?;
        // The session now waits on what it offers; say so before the
        // restore that follows activation reads it.
        self.publish_config()?;
        Ok(egress)
    }
    fn finish_model_config_hydration(&mut self) {
        self.0.finish_model_config_hydration();
    }
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let egress = self.0.goal_get();
        self.publish_goal();
        egress
    }
    fn goal_state(&self) -> Option<&crate::acp::goal::GoalState> {
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
    fn native_read(
        &mut self,
        read: &crate::acp::goal::NativeRead,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.native_read(read)
    }
    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.fork_session()
    }
    fn release_fork(
        &mut self,
        fork: &sage_acp_protocol::intent::ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.release_fork(fork, committed)
    }
    fn defer_fork_release(
        &mut self,
        fork: &sage_acp_protocol::intent::ExternalId,
        done: bool,
    ) -> bool {
        self.0.defer_fork_release(fork, done)
    }
    fn deferred_fork_release(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.0.deferred_fork_release()
    }
    fn accept_oversized_response(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        let result = self.0.accept_oversized_response(id);
        if let Some((owner, channel)) = &self.3 {
            channel.update(owner, self.0.available_commands());
        }
        result
    }
    fn skips_oversize_frames(&self) -> bool {
        self.0.skips_oversize_frames()
    }
    fn oversize_frame_skipped(&mut self) {
        self.0.oversize_frame_skipped();
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.0.take_native_rpc_reply()
    }
    fn usage(&self) -> Option<std::sync::Arc<sage_acp_protocol::usage::UsageCell>> {
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
    fn answered_control(
        &mut self,
        control: &sage_acp_protocol::session_control::SessionControl,
    ) -> bool {
        self.0.answered_control(control)
    }
    fn session_control(
        &mut self,
        control: &sage_acp_protocol::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.0.session_control(control)
    }
    fn prepare_goal_command(
        &mut self,
        action: &crate::acp::goal::GoalAction,
    ) -> Result<bool, AdapterRuntimeError> {
        self.0.prepare_goal_command(action)
    }
    fn mutate_goal(
        &mut self,
        action: crate::acp::goal::GoalAction,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let egress = self.0.mutate_goal(action)?;
        self.publish_goal();
        Ok(egress)
    }
    fn session_config(&self) -> Option<sage_acp_protocol::session_config::SessionConfig> {
        self.0.session_config()
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        self.0.refusal_detail()
    }
    fn config_pending(&self) -> bool {
        self.0.config_pending()
    }
    fn config_hydrating(&self) -> bool {
        self.0.config_hydrating()
    }
    fn configure(
        &mut self,
        intent: &sage_acp_protocol::session_config::ConfigIntent,
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
        if let AdapterCommand::Prompt { prompt, .. } | AdapterCommand::Steer { prompt, .. } =
            &command
            && !prompt.action_admitted(self.capabilities().as_ref())
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if let AdapterCommand::Prompt { prompt, .. } | AdapterCommand::Steer { prompt, .. } =
            &command
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

    fn unfinished_replies(
        &mut self,
    ) -> Result<Vec<super::adapter::AdapterEvent>, AdapterRuntimeError> {
        self.0.unfinished_replies()
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
            channel.update_observed(owner, goal);
        }
        Ok(batch)
    }

    fn adopt_turn(
        &mut self,
        client_message_id: sage_acp_protocol::contract::RequiredText<512>,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.0.adopt_turn(client_message_id, resources)
    }

    fn harness_turn_mode(&self) -> Option<&'static str> {
        self.0.harness_turn_mode()
    }

    fn answers_delivered(
        &mut self,
        resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        self.0.answers_delivered(resources)
    }

    fn decline_turn(&mut self) -> Result<Vec<AdapterEgress>, AdapterRuntimeError> {
        self.0.decline_turn()
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
        // A request refused before it was written (a catalogue page among
        // them) no longer holds the session's options back.
        let published = self.publish_config();
        result.and(published)
    }
    fn held_batch(&mut self) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        self.0.held_batch()
    }
}
