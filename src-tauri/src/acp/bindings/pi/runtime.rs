use std::collections::{BTreeMap, VecDeque};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    transport::{Frame, request_payload_digest},
};
use sage_acp_protocol::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
};

use super::{AdapterError, ModelSummary, Operation, Outbound, PiTranslator};

mod construct;
mod ingress;
mod launch;
mod support;

pub(crate) use launch::PiLaunch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestKind {
    InitializeFirst,
    InitializeSecond,
    Models,
    SetModel,
    RefreshState,
    ThinkingLevels,
    Commands,
    SetThinkingLevel,
    SetAutoCompaction,
    NewSession,
    ResumeSession,
    Prompt,
    Steer,
    ClearQueue,
    Cancel,
    /// A native RPC, written straight to Pi whether idle or mid-Turn.
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryPhase {
    Reserved,
    Queued,
}

struct RequestReservation {
    command_id: String,
    kind: RequestKind,
    phase: DeliveryPhase,
}

pub(super) struct TurnIntent {
    external_id: ExternalId,
    client_message_id: RequiredText<512>,
    text_id: Option<ExternalId>,
    text_snapshot: Option<crate::acp::redaction::text::ReplyStream>,
    compaction_id: Option<ExternalId>,
    dialog_notices: u8,
    thought_id: Option<ExternalId>,
    thought_snapshot: Option<crate::acp::redaction::text::ReplyStream>,
    tools: BTreeMap<String, sage_acp_protocol::intent::NormalizedPayload>,
    tool_snapshot_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DenyStage {
    Prepared,
    Committed,
    Queued,
    Written,
}

pub(super) struct DenyIntent {
    pub(super) request_id: ExternalId,
    pub(super) command_id: String,
    pub(super) stage: DenyStage,
    /// A declined question only cancels its dialog, so the Turn goes on.
    pub(super) releases: bool,
}

pub(crate) struct PiRuntime {
    pub(super) bridge: Option<super::bridge::Bridge>,
    pub(super) translator: PiTranslator,
    pub(super) binding_digest: [u8; 32],
    pub(super) runtime_epoch: RuntimeEpoch,
    pub(super) session_id: SageAgentSessionId,
    pub(super) external_session: Option<ExternalId>,
    session_dir: Option<std::path::PathBuf>,
    /// Pi's own session file, which `pi --session` reopens in a terminal.
    session_file: Option<RequiredText<3072>>,
    pub(super) active_turn: Option<TurnIntent>,
    steering: support::Steering,
    pub(super) deny: Option<DenyIntent>,
    cancelled_dialogs: BTreeMap<String, DenyStage>,
    recent_cancelled_dialogs: VecDeque<String>,
    questions: support::Questions,
    staged_initialize: Option<Outbound>,
    reservations: BTreeMap<u64, RequestReservation>,
    next_egress: u64,
    next_item: u64,
    pub(super) model_count: u16,
    pub(super) current_model: Option<String>,
    pub(super) available_models: Vec<ModelSummary>,
    pub(super) current_thinking: Option<String>,
    pub(super) available_thinking: Vec<String>,
    /// Pi's auto-compaction setting, once `get_state` has reported it.
    pub(super) auto_compaction: Option<bool>,
    commands: std::sync::Arc<crate::acp::slash_commands::CommandState>,
    commands_pending: bool,
    commands_discovered: bool,
    pub(super) thinking_request_session: Option<ExternalId>,
    pub(super) thinking_request_owner: Option<(SageAgentSessionId, RuntimeEpoch)>,
    pub(super) thinking_request_command: Option<String>,
    pub(super) refresh_thinking_after_model: bool,
    pub(super) initialized: bool,
    poisoned: bool,
    pub(super) native_pending: Option<String>,
    pub(super) native_rpc_reply: Option<crate::acp::goal::NativeRpcReply>,
    pub(super) usage: std::sync::Arc<sage_acp_protocol::usage::UsageCell>,
    /// A fork under way, while Pi is away from the chat's session.
    fork: Option<support::Fork>,
    /// Unsettled forks and refused copies whose cleanup has not succeeded.
    forks: Vec<support::ForkCopy>,
    launch: Option<Box<dyn PiLaunch>>,
    title_launch: Option<super::title_launch::TitleLaunch>,
    title_request: Option<super::title_request::TitleRequest>,
    title_result: Option<Option<String>>,
    #[cfg(test)]
    pub(super) remove_file_for_test: fn(&std::path::Path) -> std::io::Result<()>,
}

impl HarnessAdapter for PiRuntime {
    fn next_request_deadline(&self) -> Option<std::time::Instant> {
        self.next_question_deadline()
    }
    fn poll_request_deadlines(
        &mut self,
        _resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        self.expire_queued_questions()
    }
    fn cancel_permission(
        &mut self,
        request_id: &sage_acp_protocol::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.cancel_pending_question(request_id.as_str())
    }
    fn defer_cancel(&mut self) -> bool {
        if !self.questions.idle_turn() {
            return false;
        }
        self.questions.cancel();
        self.questions.reply_pending = true;
        true
    }
    fn start_title(&mut self, input: &str) -> Result<(), AdapterRuntimeError> {
        self.begin_title_request(input)
    }
    fn poll_title(&mut self) -> Option<AdapterEgress> {
        self.poll_title_request()
    }
    fn cancel_title(&mut self) {
        if let Some(request) = &mut self.title_request {
            request.cancel();
        }
    }
    fn title_pending(&self) -> bool {
        self.title_request.is_some()
    }
    fn take_title_result(&mut self) -> Option<Option<String>> {
        self.poll_title();
        self.title_result.take()
    }
    /// Pi ends each run with `agent_end`, which repeats the whole run and can outgrow a frame.
    /// Everything Sage needs from it already arrived in the streamed events.
    fn skips_oversize_frames(&self) -> bool {
        self.translator.skips_oversize_frames()
    }
    fn oversize_frame_skipped(&mut self) {
        self.translator.note_dropped_frame();
    }
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
        self.project_terminal_handoff();
        std::sync::Arc::clone(&self.commands)
    }
    fn discover_commands(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if self.poisoned
            || self.external_session.is_none()
            || self.active_turn.is_some()
            || self.commands_pending
            || self.commands_discovered
        {
            return Ok(None);
        }
        let egress = self.begin_one(Operation::GetCommands, RequestKind::Commands)?;
        self.commands_pending = true;
        self.commands_discovered = true;
        Ok(Some(egress))
    }
    fn session_config(&self) -> Option<sage_acp_protocol::session_config::SessionConfig> {
        self.model_session_config()
    }

    fn config_pending(&self) -> bool {
        self.translator.model_config_pending()
    }

    fn configure(
        &mut self,
        intent: &sage_acp_protocol::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.configure_session(intent).map(Some)
    }

    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.initialized.then(|| self.snapshot())
    }

    fn native_rpc_pending(&self) -> bool {
        self.native_pending.is_some()
    }

    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.native_rpc_reply.take()
    }

    fn usage(&self) -> Option<std::sync::Arc<sage_acp_protocol::usage::UsageCell>> {
        Some(std::sync::Arc::clone(&self.usage))
    }

    fn read_usage(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.begin_native(super::NativeRequest::SessionStats)
    }

    fn abandon_passive_read(&mut self) {
        self.abandon_native();
    }

    fn fork_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.begin_fork()
    }

    fn release_fork(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> crate::acp::bindings::adapter::AdapterOptionalEgress {
        self.release_fork_copy(fork, committed)
    }

    // Releasing touches only Pi's saved copy, never the running session.
    fn defer_fork_release(&mut self, fork: &ExternalId, committed: bool) -> bool {
        self.release_fork_copy(fork, committed).is_ok()
    }

    fn session_control(
        &mut self,
        control: &sage_acp_protocol::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.begin_control(control)
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        match command {
            AdapterCommand::Steer { prompt, .. } => self.begin_steer(&prompt),
            AdapterCommand::Initialize => self.begin_initialize(),
            AdapterCommand::ModelPage { cursor: None } if self.initialized => {
                self.begin_one(Operation::GetModels, RequestKind::Models)
            }
            AdapterCommand::ModelPage { .. } => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::NewSession {
                model: Some(model), ..
            } if self.initialized && self.current_model.as_deref() == Some(model.as_str()) => {
                self.begin_one(Operation::NewSession, RequestKind::NewSession)
            }
            AdapterCommand::NewSession { model: Some(_), .. } => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
            AdapterCommand::NewSession { .. } if self.initialized => {
                self.begin_one(Operation::NewSession, RequestKind::NewSession)
            }
            AdapterCommand::NewSession { .. } => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResumeSession {
                external_session_id,
                ..
            } => self.begin_resume(&external_session_id),
            AdapterCommand::LoadSession { .. } | AdapterCommand::CloseSession => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                access_mode,
            } => {
                self.require_bridge_ready()?;
                if self.external_session.is_none() || self.active_turn.is_some() {
                    return Err(AdapterRuntimeError::Operation);
                }
                let external_id =
                    ExternalId::new("piTurnId", client_message_id.as_str().to_owned())
                        .map_err(|_| AdapterRuntimeError::Operation)?;
                let egress = self.begin_prompt(&prompt, external_id.as_str())?;
                self.commands_discovered = false;
                self.active_turn = Some(TurnIntent::new(external_id, client_message_id));
                // Bridge authority is scoped to the turn that was actually reserved.
                self.begin_bridge_turn(access_mode);
                Ok(egress)
            }
            AdapterCommand::Cancel if self.active_turn.is_some() => {
                if let Some(reply) = self.cancel_question_turn()? {
                    return Ok(reply);
                }
                self.discard_bridge_edit();
                self.begin_one(Operation::Abort, RequestKind::Cancel)
            }
            AdapterCommand::Cancel => Err(AdapterRuntimeError::Operation),
            AdapterCommand::ResolvePermission {
                request_id,
                decision,
            } => self.resolve_later_permission(&request_id, decision),
        }
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.ingest_frame(frame, resources)
    }

    fn unfinished_replies(&mut self) -> Result<Vec<AdapterEvent>, AdapterRuntimeError> {
        self.whole_unfinished_replies()
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        if token.cancelled_permission_request().is_some() {
            return self.commit_dialog_cancel(token);
        }
        self.commit_deny(token)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.resolve_bridge_egress(&token, resolution)?;
        if token.is_notification() {
            return if matches!(
                resolution,
                EgressResolution::Queued | EgressResolution::Written
            ) {
                Ok(())
            } else {
                self.discard_bridge_edit();
                self.poisoned = true;
                Err(AdapterRuntimeError::MandatoryDenyTeardown)
            };
        }
        if token.cancelled_permission_request().is_some() {
            return self.resolve_dialog_cancel(token, resolution);
        }
        if token.deny_request().is_some() {
            if resolution == EgressResolution::Written {
                self.question_reply_written(token.deny_request().expect("deny token").0);
            }
            return self.resolve_deny(token, resolution);
        }
        if token.allow_request().is_some() {
            return match resolution {
                EgressResolution::Queued => Ok(()),
                EgressResolution::Written => {
                    self.question_reply_written(token.allow_request().expect("allow token"));
                    self.deny = None;
                    Ok(())
                }
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => Err(AdapterRuntimeError::EgressIntent),
            };
        }
        self.resolve_request(&token, resolution)
    }
    fn held_batch(&mut self) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        self.cancelled_questions()
    }
    fn harness_turn_authority(
        &self,
    ) -> Option<crate::acp::bindings::adapter::HarnessTurnAuthority> {
        self.questions
            .starting()
            .then_some(crate::acp::bindings::adapter::HarnessTurnAuthority::Questions)
    }
    fn adopt_turn(
        &mut self,
        client_message_id: RequiredText<512>,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adopt_questions(client_message_id)
    }
    fn answers_delivered(
        &mut self,
        _resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        self.advance_questions()
    }
    fn decline_turn(&mut self) -> Result<Vec<AdapterEgress>, AdapterRuntimeError> {
        self.decline_questions()
    }
}

#[cfg(test)]
mod tests;
