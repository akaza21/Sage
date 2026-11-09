use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
    },
    contract::{RequiredText, SafeText},
    intent::{
        ExternalId, NormalizedPayload,
        session_event::{SessionNotice, SessionScopedEvent},
    },
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
};

use super::PiRuntime;
use crate::acp::bindings::pi::{AdapterOutput, TerminalOutcome, ToolOutcome};

mod authority;
mod bridge;
mod capabilities;
mod compaction;
mod config;
mod config_reply;
mod delivery;
mod dialog;
mod errors;
mod identity;
mod image;
mod permission;
mod requests;
mod session;
mod steer;
pub(super) use steer::Steering;
mod stream;
mod thought;
mod tool_cache;
use capabilities::capabilities;
pub(super) use errors::map_pi;
pub(super) use identity::unsupported;
use identity::{external, model_event, text_payload};

impl PiRuntime {
    pub(super) fn translate_outputs(
        &mut self,
        outputs: Vec<AdapterOutput>,
        staged: Option<AdapterEgress>,
        request_digest: [u8; 32],
        resources: &dyn crate::acp::bindings::adapter::ResourceMapper,
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
        let mut initialized = None;
        for output in outputs {
            let (event, egress) = self.translate_output(output, request_digest, resources)?;
            let mandatory = egress
                .as_ref()
                .is_some_and(|value| value.token().is_mandatory());
            if let Some(event) = event {
                if matches!(event, AdapterEvent::Initialized { .. }) {
                    initialized = Some(event);
                } else {
                    builder
                        .push_event(event)
                        .map_err(|error| self.batch_failure(error, mandatory, staged_id))?;
                }
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
        let batch = builder.finish();
        if let Some(initialized) = initialized {
            let (events, egress) = batch.into_parts();
            AdapterBatch::new(std::iter::once(initialized).chain(events).collect(), egress)
        } else {
            Ok(batch)
        }
    }

    fn translate_output(
        &mut self,
        output: AdapterOutput,
        request_digest: [u8; 32],
        resources: &dyn crate::acp::bindings::adapter::ResourceMapper,
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
            AdapterOutput::CommandAccepted { command } => {
                let refresh = if command == "prompt"
                    && (self.refresh_thinking_after_model || self.available_thinking.is_empty())
                    && self.thinking_request_command.is_none()
                {
                    self.refresh_thinking_after_model = true;
                    self.begin_one(
                        crate::acp::bindings::pi::Operation::GetState,
                        super::RequestKind::RefreshState,
                    )
                    .ok()
                } else {
                    None
                };
                (Some(AdapterEvent::Acknowledged), refresh)
            }
            AdapterOutput::Commands(value) => {
                self.commands_pending = false;
                self.commands.replace(&value, true)?;
                (Some(AdapterEvent::Absorbed), None)
            }
            AdapterOutput::SessionOpened(id) => self.open_session(id)?,

            AdapterOutput::State(state) => self.state_reply(state)?,
            AdapterOutput::Models(models) => {
                let event = model_event(models.clone(), self.current_model.as_deref())?;
                if let AdapterEvent::Models { models, .. } = &event {
                    self.model_count = models
                        .len()
                        .try_into()
                        .map_err(|_| AdapterRuntimeError::BatchCount)?;
                }
                self.available_models = models;
                (Some(event), None)
            }
            AdapterOutput::ModelSelected(model) => self.model_selected(model)?,
            AdapterOutput::ThinkingLevels(levels) => self.thinking_levels(levels),
            AdapterOutput::ThinkingSelected(level) => self.thinking_selected(level)?,
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
                    TerminalOutcome::Cancelled => TurnState::Cancelled,
                    TerminalOutcome::Interrupted => TurnState::Interrupted,
                    TerminalOutcome::Failed => TurnState::Failed,
                };
                let event = AdapterEvent::Turn {
                    id: self.turn_identity()?,
                    state,
                };
                if let Some(bridge) = self.bridge.as_mut() {
                    bridge.pending = None;
                    bridge.used.clear();
                }
                self.active_turn = None;
                self.clear_thinking_request();
                self.deny = None;
                (Some(event), self.finish_steering())
            }
            AdapterOutput::FailureDetail { message } => (Some(self.failure_detail(message)?), None),
            AdapterOutput::AttemptFailed {
                attempt,
                max_attempts,
                message,
            } => (
                Some(self.attempt_failed(attempt, max_attempts, message)?),
                None,
            ),
            // The answer's item opens with its first text, so thinking that
            // streams first keeps its place above the answer.
            AdapterOutput::TextStarted { external_id } => {
                self.start_text(external_id)?;
                (Some(AdapterEvent::Absorbed), None)
            }
            AdapterOutput::TextDelta {
                text,
                redacted_tail,
            } => {
                let (id, snapshot) = self.append_text(text, redacted_tail)?;
                let item = (!snapshot.is_empty())
                    .then(|| self.item(id, ItemLifecycle::Streaming, None, text_payload(snapshot)))
                    .transpose()?;
                (Some(item.unwrap_or(AdapterEvent::Absorbed)), None)
            }
            AdapterOutput::ThoughtDelta {
                text,
                redacted_tail,
            } => {
                let (id, snapshot) = self.append_thought(text, redacted_tail)?;
                (
                    Some(
                        self.item(
                            id,
                            ItemLifecycle::Streaming,
                            None,
                            NormalizedPayload::ThoughtSummary {
                                summary: SafeText::new("thoughtSummary", snapshot)
                                    .map_err(|_| AdapterRuntimeError::Frame)?,
                            },
                        )?,
                    ),
                    None,
                )
            }
            AdapterOutput::ThoughtStarted => (Some(self.start_thought()?), None),
            AdapterOutput::ThoughtCompleted { outcome } => (self.complete_thought(outcome)?, None),
            AdapterOutput::TextCompleted {
                external_id,
                outcome,
            } => {
                let (id, snapshot) = self.complete_text(external_id)?;
                if snapshot.is_empty() {
                    return Ok((Some(AdapterEvent::Absorbed), None));
                }
                let lifecycle = match outcome {
                    TerminalOutcome::Completed => ItemLifecycle::Completed,
                    TerminalOutcome::Failed => ItemLifecycle::Failed,
                    TerminalOutcome::Cancelled | TerminalOutcome::Interrupted => {
                        ItemLifecycle::Cancelled
                    }
                };
                (
                    Some(self.item(id, lifecycle, None, text_payload(snapshot))?),
                    None,
                )
            }
            AdapterOutput::CompactionStarted => (Some(self.start_compaction()?), None),
            AdapterOutput::CompactionCompleted => (Some(self.finish_compaction()?), None),
            AdapterOutput::CompactionInterrupted => (Some(self.interrupt_compaction()?), None),
            AdapterOutput::CompactionFailed => (Some(self.fail_compaction()?), None),
            AdapterOutput::ToolStarted {
                external_id,
                name,
                path,
            } => {
                let id = external("piItemId", external_id.clone())?;
                let payload = identity::tool_start_payload(name, path.as_deref(), resources)?;
                self.cache_tool(external_id, payload.clone(), false)?;
                (
                    Some(self.item(
                        id,
                        ItemLifecycle::Started,
                        Some(ToolState::InProgress),
                        payload,
                    )?),
                    None,
                )
            }
            AdapterOutput::ToolUpdated {
                external_id,
                output,
            } => {
                let id = external("piItemId", external_id.clone())?;
                let payload = self.update_tool(&external_id, output, false)?;
                (
                    Some(self.item(
                        id,
                        ItemLifecycle::Streaming,
                        Some(ToolState::InProgress),
                        payload,
                    )?),
                    None,
                )
            }
            AdapterOutput::ToolTerminal {
                external_id,
                outcome,
                output,
            } => {
                let id = external("piItemId", external_id.clone())?;
                // A finished edit shows its diff; Pi's own success line adds nothing.
                let output = output.filter(|_| {
                    outcome != ToolOutcome::Completed || !self.is_edit_tool(&external_id)
                });
                let payload = self.update_tool(&external_id, output, true)?;
                let (lifecycle, state) = match outcome {
                    ToolOutcome::Completed => (ItemLifecycle::Completed, ToolState::Completed),
                    ToolOutcome::Failed => (ItemLifecycle::Failed, ToolState::Failed),
                    ToolOutcome::Declined => (ItemLifecycle::Cancelled, ToolState::Declined),
                    ToolOutcome::Cancelled => (ItemLifecycle::Cancelled, ToolState::Cancelled),
                };
                (Some(self.item(id, lifecycle, Some(state), payload)?), None)
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
                        context_window: self.current_context_window(),
                    },
                }),
                None,
            ),
            AdapterOutput::SteerResolved {
                command_id,
                accepted,
            } => (Some(self.steer_resolved(&command_id, accepted)), None),
            AdapterOutput::Notice {
                level,
                source,
                text,
            } => (
                Some(
                    self.session_event(SessionScopedEvent::Notice(SessionNotice {
                        level,
                        text: RequiredText::new("noticeText", text)
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                        source: Some(
                            RequiredText::new("noticeSource", source)
                                .map_err(|_| AdapterRuntimeError::Frame)?,
                        ),
                    })),
                ),
                None,
            ),
            AdapterOutput::Title(title) => (
                Some(
                    crate::acp::bindings::session_title::harness_title_event(&title)
                        .map_or(AdapterEvent::IgnoredLate, |event| self.session_event(event)),
                ),
                None,
            ),
            AdapterOutput::PermissionRequested { external_id } => (
                Some(self.permission_event(external_id, request_digest)?),
                None,
            ),
            AdapterOutput::AuthorityBlocked { external_id, tool } => {
                self.block_authority(external_id, tool, resources)?
            }
            AdapterOutput::DialogCancelled { external_id } => self.cancel_dialog(external_id)?,
            AdapterOutput::DialogExpired { external_id } => self.expire_dialog(external_id)?,
            AdapterOutput::Image { media_type, data } => {
                (Some(self.image_event(media_type, data)?), None)
            }
            AdapterOutput::Error { code } => (
                Some(AdapterEvent::Error(NormalizedPayload::Error {
                    kind: None,
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
            AdapterOutput::UnknownEventsSuppressed => (
                Some(unsupported(
                    "pi_unknown_event_limit",
                    "Further unknown Pi RPC event notices are suppressed for this turn",
                )),
                None,
            ),
            AdapterOutput::DuplicateIgnored => (Some(AdapterEvent::IgnoredDuplicate), None),
            AdapterOutput::LateIgnored => (Some(AdapterEvent::IgnoredLate), None),
            AdapterOutput::Absorbed => (Some(AdapterEvent::Absorbed), None),
        };
        Ok(pair)
    }
}
