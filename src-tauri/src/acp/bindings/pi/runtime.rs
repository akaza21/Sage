use std::collections::{BTreeMap, VecDeque};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    transport::{Frame, request_payload_digest},
};

use super::{AdapterError, ModelSummary, Operation, Outbound, PiTranslator};

mod ingress;
mod support;

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
    NewSession,
    Prompt,
    Steer,
    ClearQueue,
    Cancel,
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
    text_snapshot: Option<String>,
    text_redacted_tail: bool,
    compaction_id: Option<ExternalId>,
    dialog_notices: u8,
    thought_id: Option<ExternalId>,
    thought_snapshot: Option<String>,
    thought_redacted_tail: bool,
    tools: BTreeMap<String, crate::acp::intent::NormalizedPayload>,
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
}

pub(crate) struct PiRuntime {
    pub(super) bridge: Option<super::bridge::Bridge>,
    pub(super) translator: PiTranslator,
    pub(super) binding_digest: [u8; 32],
    pub(super) runtime_epoch: RuntimeEpoch,
    pub(super) session_id: SageAgentSessionId,
    pub(super) external_session: Option<ExternalId>,
    pub(super) active_turn: Option<TurnIntent>,
    steering: support::Steering,
    pub(super) deny: Option<DenyIntent>,
    cancelled_dialogs: BTreeMap<String, DenyStage>,
    recent_cancelled_dialogs: VecDeque<String>,
    staged_initialize: Option<Outbound>,
    reservations: BTreeMap<u64, RequestReservation>,
    next_egress: u64,
    next_session: u64,
    next_item: u64,
    pub(super) model_count: u16,
    pub(super) current_model: Option<String>,
    pub(super) available_models: Vec<ModelSummary>,
    pub(super) current_thinking: Option<String>,
    pub(super) available_thinking: Vec<String>,
    commands: std::sync::Arc<crate::acp::slash_commands::CommandState>,
    commands_pending: bool,
    commands_discovered: bool,
    pub(super) thinking_request_session: Option<ExternalId>,
    pub(super) thinking_request_owner: Option<(SageAgentSessionId, RuntimeEpoch)>,
    pub(super) thinking_request_command: Option<String>,
    pub(super) refresh_thinking_after_model: bool,
    pub(super) initialized: bool,
    poisoned: bool,
}

impl PiRuntime {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn from_fixture_binding(
        binding: &super::fixture_admission::PiFixtureBinding,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        binding.revalidate()?;
        Ok(Self::new(
            PiTranslator::fixture_admitted(runtime_epoch.get()),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        ))
    }

    pub(crate) fn from_native_binding(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self::new(
            PiTranslator::native_admitted(runtime_epoch.get()),
            binding_digest,
            runtime_epoch,
            session_id,
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture(runtime_epoch: RuntimeEpoch, session_id: SageAgentSessionId) -> Self {
        Self::new(
            PiTranslator::fixture_admitted(runtime_epoch.get()),
            [7; 32],
            runtime_epoch,
            session_id,
        )
    }

    fn new(
        translator: PiTranslator,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            bridge: None,
            translator,
            binding_digest,
            runtime_epoch,
            session_id,
            external_session: None,
            active_turn: None,
            steering: support::Steering::default(),
            deny: None,
            cancelled_dialogs: BTreeMap::new(),
            recent_cancelled_dialogs: VecDeque::new(),
            staged_initialize: None,
            reservations: BTreeMap::new(),
            next_egress: 0,
            next_session: 0,
            next_item: 0,
            model_count: 0,
            current_model: None,
            available_models: Vec::new(),
            current_thinking: None,
            available_thinking: Vec::new(),
            commands: std::sync::Arc::default(),
            commands_pending: false,
            commands_discovered: false,
            thinking_request_session: None,
            thinking_request_owner: None,
            thinking_request_command: None,
            refresh_thinking_after_model: false,
            initialized: false,
            poisoned: false,
        }
    }
}

impl HarnessAdapter for PiRuntime {
    fn available_commands(&self) -> std::sync::Arc<crate::acp::slash_commands::CommandState> {
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
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.model_session_config()
    }

    fn config_pending(&self) -> bool {
        self.translator.model_config_pending()
    }

    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.configure_session(intent).map(Some)
    }

    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.initialized.then(|| self.snapshot())
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
            AdapterCommand::ResumeSession { .. }
            | AdapterCommand::LoadSession { .. }
            | AdapterCommand::CloseSession => Err(AdapterRuntimeError::UnsupportedCommand),
            AdapterCommand::Prompt {
                prompt,
                client_message_id,
                access_mode,
            } => {
                if self.external_session.is_none() || self.active_turn.is_some() {
                    return Err(AdapterRuntimeError::Operation);
                }
                let extension_command = prompt.command_revision.is_some()
                    && self.commands.snapshot().commands.iter().any(|c| {
                        c.extension
                            && prompt
                                .as_str()
                                .strip_prefix('/')
                                .and_then(|s| s.split_whitespace().next())
                                == Some(c.name.as_str())
                    });
                let external_id =
                    ExternalId::new("piTurnId", client_message_id.as_str().to_owned())
                        .map_err(|_| AdapterRuntimeError::Operation)?;
                let egress = self.begin_one(
                    Operation::Prompt {
                        turn_key: external_id.as_str().to_owned(),
                        message: prompt.as_str().to_owned(),
                    },
                    RequestKind::Prompt,
                )?;
                if extension_command {
                    self.translator
                        .mark_extension_command()
                        .map_err(support::map_pi)?;
                }
                self.commands_discovered = false;
                self.active_turn = Some(TurnIntent {
                    external_id,
                    client_message_id,
                    text_id: None,
                    text_snapshot: None,
                    text_redacted_tail: false,
                    compaction_id: None,
                    dialog_notices: 0,
                    thought_id: None,
                    thought_snapshot: None,
                    thought_redacted_tail: false,
                    tools: BTreeMap::new(),
                    tool_snapshot_bytes: 0,
                });
                // Bridge authority is scoped to the turn that was actually reserved.
                self.begin_bridge_turn(access_mode);
                Ok(egress)
            }
            AdapterCommand::Cancel if self.active_turn.is_some() => {
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
            return self.resolve_deny(token, resolution);
        }
        if token.allow_request().is_some() {
            return match resolution {
                EgressResolution::Queued => Ok(()),
                EgressResolution::Written => {
                    self.deny = None;
                    Ok(())
                }
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => Err(AdapterRuntimeError::EgressIntent),
            };
        }
        let id = token
            .request_id()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        match resolution {
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes => self.rollback(id),
            EgressResolution::Queued => {
                let reservation = self
                    .reservations
                    .get_mut(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                reservation.phase = DeliveryPhase::Queued;
                Ok(())
            }
            EgressResolution::Written => {
                let reservation = self
                    .reservations
                    .get(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                if reservation.phase != DeliveryPhase::Queued
                    && !matches!(
                        reservation.kind,
                        RequestKind::SetModel
                            | RequestKind::SetThinkingLevel
                            | RequestKind::Commands
                    )
                {
                    return Err(AdapterRuntimeError::EgressIntent);
                }
                if reservation.kind == RequestKind::Prompt {
                    self.translator
                        .mark_prompt_written(&reservation.command_id)
                        .map_err(support::map_pi)?;
                }
                self.reservations.remove(&id);
                Ok(())
            }
            EgressResolution::DeliveryUnknown => {
                self.reservations
                    .remove(&id)
                    .ok_or(AdapterRuntimeError::EgressIntent)?;
                self.staged_initialize = None;
                self.poisoned = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
