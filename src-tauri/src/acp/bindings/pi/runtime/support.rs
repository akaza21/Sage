use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressToken,
    },
    contract::{RequiredText, SafeText, SchemaVersionV1},
    intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
};

use super::{DenyIntent, DenyStage, PiRuntime};
use crate::acp::bindings::pi::{AdapterError, AdapterOutput, TerminalOutcome, ToolOutcome};

mod compaction;
mod delivery;
mod identity;
use identity::{external, model_event, model_key, text_payload, tool_payload, unsupported};

impl PiRuntime {
    pub(super) fn resolve_later_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let pending = self
            .deny
            .as_ref()
            .is_some_and(|intent| intent.request_id.as_str() == request_id.as_str());
        if !pending {
            return Err(AdapterRuntimeError::LaterPermissionMiss);
        }
        let reply = match decision {
            crate::acp::runtime::permissions::PermissionDecision::Ask => {
                let _ = crate::acp::runtime::later_permission::resolve_later_permission(
                    request_id, decision, None,
                )
                .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                return Err(AdapterRuntimeError::Operation);
            }
            crate::acp::runtime::permissions::PermissionDecision::AllowOnce => {
                let pending = self
                    .translator
                    .take_pending_ui()
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)?;
                let body = match pending.method.as_str() {
                    "select" => {
                        let Some(value) = pending.options.iter().find(|option| {
                            matches!(option.to_ascii_lowercase().as_str(), "allow" | "allow once")
                        }) else {
                            self.translator.remember_pending_ui(pending);
                            return Err(AdapterRuntimeError::LaterPermissionMiss);
                        };
                        serde_json::json!({
                            "type": "extension_ui_response",
                            "id": pending.id,
                            "value": value,
                        })
                    }
                    "confirm" => serde_json::json!({
                        "type": "extension_ui_response",
                        "id": pending.id,
                        "confirmed": true,
                    }),
                    _ => {
                        self.translator.remember_pending_ui(pending);
                        return Err(AdapterRuntimeError::LaterPermissionMiss);
                    }
                };
                self.translator.release_permission_wait();
                let bytes = serde_json::to_vec(&body).map_err(|_| AdapterRuntimeError::Frame)?;
                let frame = crate::acp::transport::OutboundFrame::new(bytes)
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                AdapterEgress::new(frame, EgressToken::protocol_notification())
            }
            crate::acp::runtime::permissions::PermissionDecision::AllowAlways => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            crate::acp::runtime::permissions::PermissionDecision::Deny => {
                self.mandatory_abort()?
                    .ok_or(AdapterRuntimeError::Operation)?
            }
        };
        crate::acp::runtime::later_permission::resolve_later_permission(
            request_id,
            decision,
            Some(reply),
        )
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
    }
}

pub(super) fn capabilities(model_count: u16) -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "pi-rpc-0.83.0")
            .expect("fixed protocol version"),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count,
        config_count: 0,
    }
}

impl PiRuntime {
    pub(super) fn translate_outputs(
        &mut self,
        outputs: Vec<AdapterOutput>,
        staged: Option<AdapterEgress>,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if outputs.len() > LIMITS.queues.journal_facts_per_transaction as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        if self.deny.is_some()
            && outputs
                .iter()
                .any(|value| matches!(value, AdapterOutput::Error { .. }))
        {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let staged_id = staged.as_ref().and_then(|value| value.token().request_id());
        let mut builder = AdapterBatchBuilder::new();
        for output in outputs {
            let (event, egress) = self.translate_output(output, request_digest)?;
            let mandatory = egress
                .as_ref()
                .is_some_and(|value| value.token().is_mandatory());
            if let Some(event) = event {
                builder
                    .push_event(event)
                    .map_err(|error| self.batch_failure(error, mandatory, staged_id))?;
            }
            if let Some(egress) = egress {
                builder
                    .push_egress(egress)
                    .map_err(|error| self.batch_failure(error, mandatory, staged_id))?;
            }
        }
        if let Some(staged) = staged {
            builder
                .push_egress(staged)
                .map_err(|error| self.batch_failure(error, false, staged_id))?;
        }
        Ok(builder.finish())
    }

    fn translate_output(
        &mut self,
        output: AdapterOutput,
        request_digest: [u8; 32],
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let pair = match output {
            AdapterOutput::InitializationProgress => (None, None),
            AdapterOutput::Initialized => {
                self.initialized = true;
                (
                    Some(AdapterEvent::Initialized {
                        capabilities: self.snapshot(),
                    }),
                    None,
                )
            }
            AdapterOutput::CommandAccepted { .. } => (Some(AdapterEvent::Acknowledged), None),
            AdapterOutput::SessionReset => {
                self.next_session = self
                    .next_session
                    .checked_add(1)
                    .filter(|value| *value <= i64::MAX as u64)
                    .ok_or(AdapterRuntimeError::Operation)?;
                let external = ExternalId::new(
                    "piSessionId",
                    format!(
                        "pi-session-{}-{}",
                        self.runtime_epoch.get(),
                        self.next_session
                    ),
                )
                .map_err(|_| AdapterRuntimeError::Operation)?;
                self.external_session = Some(external.clone());
                (
                    Some(AdapterEvent::SessionReady {
                        id: self.identity(external, None, None),
                        loaded: false,
                    }),
                    None,
                )
            }
            AdapterOutput::State(state) => {
                self.current_model = state.model.as_ref().map(model_key).transpose()?;
                let _ = (state.thinking_level, state.has_session, state.is_streaming);
                (None, None)
            }
            AdapterOutput::Models(models) => {
                let event = model_event(models, self.current_model.as_deref())?;
                if let AdapterEvent::Models { models, .. } = &event {
                    self.model_count = models
                        .len()
                        .try_into()
                        .map_err(|_| AdapterRuntimeError::BatchCount)?;
                }
                (Some(event), None)
            }
            AdapterOutput::Messages(messages) => {
                let _ = messages;
                (
                    Some(unsupported(
                        "pi_messages",
                        "Pi history page is not projected",
                    )),
                    None,
                )
            }
            AdapterOutput::Entries(entries) => {
                let _ = entries;
                (
                    Some(unsupported("pi_entries", "Pi entry page is not projected")),
                    None,
                )
            }
            AdapterOutput::TurnRunning { turn_key } => {
                self.require_turn(&turn_key)?;
                (
                    Some(AdapterEvent::Turn {
                        id: self.turn_identity()?,
                        state: TurnState::Running,
                    }),
                    None,
                )
            }
            AdapterOutput::TurnTerminal { turn_key, outcome } => {
                self.require_turn(&turn_key)?;
                let state = match outcome {
                    TerminalOutcome::Completed => TurnState::Completed,
                    TerminalOutcome::Interrupted => TurnState::Interrupted,
                    TerminalOutcome::Failed => TurnState::Failed,
                };
                let event = AdapterEvent::Turn {
                    id: self.turn_identity()?,
                    state,
                };
                self.active_turn = None;
                self.deny = None;
                (Some(event), None)
            }
            AdapterOutput::TextStarted { external_id } => {
                let id = self.start_text(external_id)?;
                (
                    Some(self.item(id, ItemLifecycle::Started, None, text_payload(""))?),
                    None,
                )
            }
            AdapterOutput::TextDelta { text } => {
                let (id, snapshot) = self.append_text(text)?;
                (
                    Some(self.item(id, ItemLifecycle::Streaming, None, text_payload(snapshot))?),
                    None,
                )
            }
            AdapterOutput::TextCompleted { external_id } => {
                let (id, snapshot) = self.complete_text(external_id)?;
                (
                    Some(self.item(id, ItemLifecycle::Completed, None, text_payload(snapshot))?),
                    None,
                )
            }
            AdapterOutput::CompactionStarted => (Some(self.start_compaction()?), None),
            AdapterOutput::CompactionCompleted => (Some(self.finish_compaction()?), None),
            AdapterOutput::CompactionInterrupted => (Some(self.interrupt_compaction()?), None),
            AdapterOutput::ToolStarted { external_id, name } => {
                let id = external("piItemId", external_id)?;
                (
                    Some(self.item(
                        id,
                        ItemLifecycle::Started,
                        Some(ToolState::InProgress),
                        tool_payload(name)?,
                    )?),
                    None,
                )
            }
            AdapterOutput::ToolUpdated { external_id } => {
                let id = external("piItemId", external_id)?;
                (
                    Some(self.item(
                        id,
                        ItemLifecycle::Streaming,
                        Some(ToolState::InProgress),
                        tool_payload("Pi tool update")?,
                    )?),
                    None,
                )
            }
            AdapterOutput::ToolTerminal {
                external_id,
                outcome,
            } => {
                let id = external("piItemId", external_id)?;
                let (lifecycle, state) = match outcome {
                    ToolOutcome::Completed => (ItemLifecycle::Completed, ToolState::Completed),
                    ToolOutcome::Failed => (ItemLifecycle::Failed, ToolState::Failed),
                    ToolOutcome::Declined => (ItemLifecycle::Cancelled, ToolState::Declined),
                    ToolOutcome::Cancelled => (ItemLifecycle::Cancelled, ToolState::Cancelled),
                };
                (
                    Some(self.item(id, lifecycle, Some(state), tool_payload("Pi tool result")?)?),
                    None,
                )
            }
            AdapterOutput::Usage {
                input_tokens,
                output_tokens,
            } => (
                Some(AdapterEvent::Usage {
                    turn_id: self.turn_identity()?,
                    payload: NormalizedPayload::Usage {
                        input_tokens,
                        output_tokens,
                        context_window: None,
                    },
                }),
                None,
            ),
            AdapterOutput::PermissionDeclined { external_id, tool } => {
                let id = match external_id {
                    Some(id) => external("piPermissionId", id)?,
                    None => self.next_local_item("permission")?,
                };
                let turn_id = self.turn_identity()?;
                let item_id = self.identity(
                    id.clone(),
                    Some(turn_id.external_id.clone()),
                    turn_id.turn_client_message_id.clone(),
                );
                self.deny = Some(DenyIntent {
                    request_id: id.clone(),
                    command_id: String::new(),
                    stage: DenyStage::Prepared,
                });
                (
                    Some(AdapterEvent::PermissionRequest {
                        turn_id,
                        item_id,
                        external_request_id: id,
                        payload: NormalizedPayload::PermissionRequest {
                            category: ToolCategory::Other,
                            summary: SafeText::new(
                                "permissionSummary",
                                format!(
                                    "Pi requested authority for {tool}; awaiting your decision"
                                ),
                            )
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                            offered_option_kinds: None,
                        },
                        request_payload_digest: request_digest,
                    }),
                    None,
                )
            }
            AdapterOutput::AbortRequired => (None, self.mandatory_abort()?),
            AdapterOutput::Error { code } => (
                Some(AdapterEvent::Error(NormalizedPayload::Error {
                    code: SafeText::new("errorCode", code)
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    message: SafeText::new("errorMessage", "Pi RPC command failed")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    retryable: false,
                })),
                None,
            ),
            AdapterOutput::Unsupported { kind } => {
                (Some(unsupported(kind, "Unsupported Pi RPC event")), None)
            }
            AdapterOutput::DuplicateIgnored => (Some(AdapterEvent::IgnoredDuplicate), None),
            AdapterOutput::LateIgnored => (Some(AdapterEvent::IgnoredLate), None),
        };
        Ok(pair)
    }
}

pub(super) fn map_pi(error: AdapterError) -> AdapterRuntimeError {
    match error {
        AdapterError::UnsupportedOperation(_) => AdapterRuntimeError::UnsupportedCommand,
        AdapterError::LaunchNotAdmitted => AdapterRuntimeError::LaunchUnavailable,
        AdapterError::Malformed | AdapterError::Limit(_) => AdapterRuntimeError::Frame,
        AdapterError::PermissionViolation => AdapterRuntimeError::MandatoryDenyTeardown,
        _ => AdapterRuntimeError::Operation,
    }
}
