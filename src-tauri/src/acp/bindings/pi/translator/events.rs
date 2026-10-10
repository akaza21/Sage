use serde_json::Value;

mod message;
mod permission;
mod tool_result;
mod turn;
use tool_result::{tool_images, tool_text};

use super::{ActiveTurn, EventPhase, PiTranslator, TOOLS_PER_TURN};
use crate::acp::bindings::pi::{
    decode::{bounded_string, external_id, redact_structured_text},
    protocol::{AdapterError, AdapterOutput, TerminalOutcome, ToolOutcome},
};

impl PiTranslator {
    pub(super) fn ingest_event(
        &mut self,
        kind: &str,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if matches!(
            kind,
            "thinking_level_changed" | "session_info_changed" | "queue_update" | "entry_appended"
        ) {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if kind == "extension_ui_request" {
            return self.extension_request(value);
        }
        if self.active.is_none() {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if kind != "agent_settled"
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.terminal.is_some())
        {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if !matches!(kind, "agent_end" | "agent_settled")
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.authority_denied)
        {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if matches!(kind, "turn_start" | "turn_end")
            || kind.starts_with("auto_retry_")
            || kind.starts_with("summarization_retry_")
        {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        match kind {
            "agent_start" => {
                let turn = self.live_turn_any_phase()?;
                if !matches!(
                    turn.event_phase,
                    EventPhase::PromptAccepted | EventPhase::AgentEnded
                ) {
                    return Err(AdapterError::EventSequence);
                }
                turn.event_phase = EventPhase::AgentRunning;
                Ok(vec![AdapterOutput::TurnRunning {
                    turn_key: turn.turn_key.clone(),
                }])
            }
            "agent_end" => self.agent_end(value),
            "agent_settled" => self.agent_settled(),
            "message_start" => {
                self.live_turn()?;
                if !assistant_message(value) {
                    return Ok(Vec::new());
                }
                Ok(vec![AdapterOutput::TextStarted {
                    external_id: message_id(value)?,
                }])
            }
            "message_update" => self.message_update(value),
            "message_end" => {
                self.live_turn()?;
                if !assistant_message(value) {
                    return Ok(Vec::new());
                }
                let outcome = match value
                    .get("message")
                    .and_then(|message| message.get("stopReason"))
                    .and_then(Value::as_str)
                {
                    Some("aborted") => TerminalOutcome::Interrupted,
                    Some("error") => TerminalOutcome::Failed,
                    _ => TerminalOutcome::Completed,
                };
                let mut output = self.complete_thought(outcome);
                output.push(AdapterOutput::TextCompleted {
                    external_id: message_id(value)?,
                });
                Ok(output)
            }
            "tool_execution_start" => self.tool_start(value),
            "tool_execution_update" => self.tool_update(value),
            "tool_execution_end" => self.tool_end(value),
            "compaction_start" => {
                let turn = self.session_turn()?;
                if turn.compaction_active {
                    return Ok(vec![AdapterOutput::DuplicateIgnored]);
                }
                turn.compaction_active = true;
                Ok(vec![AdapterOutput::CompactionStarted])
            }
            "compaction_end" => {
                let turn = self.session_turn()?;
                let aborted = value.get("aborted").and_then(Value::as_bool) == Some(true);
                let failed = value.get("errorMessage").is_some()
                    || !value.get("result").is_some_and(Value::is_object);
                let mut output = Vec::new();
                if !turn.compaction_active {
                    if !aborted && !failed {
                        return Ok(vec![AdapterOutput::LateIgnored]);
                    }
                    output.push(AdapterOutput::CompactionStarted);
                }
                turn.compaction_active = false;
                output.push(if aborted {
                    AdapterOutput::CompactionInterrupted
                } else if failed {
                    AdapterOutput::CompactionFailed
                } else {
                    AdapterOutput::CompactionCompleted
                });
                Ok(output)
            }
            _ => self.unknown_event(kind),
        }
    }

    fn unknown_event(&mut self, kind: &str) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.live_turn_any_phase()?;
        if turn.unknown_kinds.contains(kind) {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        if turn.unknown_kinds.len() >= super::UNKNOWN_KINDS_PER_TURN {
            return Ok(vec![
                if std::mem::replace(&mut turn.unknown_suppression_emitted, true) {
                    AdapterOutput::Absorbed
                } else {
                    AdapterOutput::UnknownEventsSuppressed
                },
            ]);
        }
        turn.unknown_kinds.insert(kind.to_owned());
        Ok(vec![AdapterOutput::Unsupported {
            kind: redact_structured_text(kind),
        }])
    }

    fn live_turn(&mut self) -> Result<&mut ActiveTurn, AdapterError> {
        let turn = self.live_turn_any_phase()?;
        if turn.event_phase != EventPhase::AgentRunning {
            return Err(AdapterError::EventSequence);
        }
        Ok(turn)
    }

    fn live_turn_any_phase(&mut self) -> Result<&mut ActiveTurn, AdapterError> {
        let turn = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
        if turn.terminal.is_some() {
            return Err(AdapterError::StaleIntent);
        }
        Ok(turn)
    }

    pub(super) fn finish_turn(
        &mut self,
        incoming: TerminalOutcome,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_none())
        {
            self.close_config_requests();
        }
        let turn = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
        match turn.terminal {
            Some(current) if current == incoming => Ok(vec![AdapterOutput::DuplicateIgnored]),
            Some(_) => Err(AdapterError::TerminalConflict),
            None => {
                turn.terminal = Some(incoming);
                let mut events = Vec::new();
                if turn.thought_active {
                    turn.thought_active = false;
                    events.push(AdapterOutput::ThoughtCompleted { outcome: incoming });
                }
                if turn.compaction_active {
                    turn.compaction_active = false;
                    events.push(match incoming {
                        TerminalOutcome::Failed => AdapterOutput::CompactionFailed,
                        TerminalOutcome::Completed | TerminalOutcome::Interrupted => {
                            AdapterOutput::CompactionInterrupted
                        }
                    });
                }
                events.push(AdapterOutput::TurnTerminal {
                    turn_key: turn.turn_key.clone(),
                    outcome: incoming,
                });
                Ok(events)
            }
        }
    }

    pub(super) fn close_config_requests(&mut self) {
        self.closed_config_requests.extend(
            self.pending
                .iter()
                .filter(|(_, kind)| kind.is_config())
                .map(|(id, _)| id.clone()),
        );
    }

    fn complete_thought(&mut self, outcome: TerminalOutcome) -> Vec<AdapterOutput> {
        if let Some(turn) = self.active.as_mut()
            && std::mem::take(&mut turn.thought_active)
        {
            vec![AdapterOutput::ThoughtCompleted { outcome }]
        } else {
            Vec::new()
        }
    }

    fn tool_start(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let id = required_tool_id(value)?;
        let name = bounded_string(
            value.get("toolName").or_else(|| value.get("tool_name")),
            128,
        )?
        .ok_or(AdapterError::Malformed)?;
        if name != "read" {
            self.live_turn()?.authority_denied = true;
            return Ok(vec![AdapterOutput::AuthorityBlocked {
                external_id: id,
                tool: name,
            }]);
        }
        let path = value
            .get("args")
            .and_then(|args| args.get("path"))
            .map(|path| {
                let path = path.as_str().ok_or(AdapterError::Malformed)?;
                if path.len()
                    > crate::acp::limits::LIMITS
                        .wire
                        .absolute_path_bytes_native_only as usize
                    || path.contains('\0')
                {
                    return Err(AdapterError::Limit("tool path bytes"));
                }
                Ok(path.to_owned())
            })
            .transpose()?;
        let turn = self.live_turn()?;
        if turn.tools.contains_key(&id) {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
        if turn.tools.len() >= TOOLS_PER_TURN {
            return Err(AdapterError::Limit("tools per Turn"));
        }
        turn.tools.insert(id.clone(), None);
        Ok(vec![AdapterOutput::ToolStarted {
            external_id: id,
            name,
            path,
        }])
    }

    fn tool_update(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let id = required_tool_id(value)?;
        let state = self
            .live_turn()?
            .tools
            .get(&id)
            .ok_or(AdapterError::Correlation)?;
        if state.is_some() {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        let output = tool_text(
            value
                .get("partialResult")
                .or_else(|| value.get("partial_result")),
        )?;
        Ok(vec![AdapterOutput::ToolUpdated {
            external_id: id,
            output,
        }])
    }

    fn tool_end(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let id = required_tool_id(value)?;
        let outcome = if value
            .get("isError")
            .or_else(|| value.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            ToolOutcome::Failed
        } else {
            ToolOutcome::Completed
        };
        let output = tool_text(value.get("result"))?;
        let images = tool_images(value.get("result"))?;
        let state = self
            .live_turn()?
            .tools
            .get_mut(&id)
            .ok_or(AdapterError::Correlation)?;
        match state {
            Some(current) if *current == outcome => Ok(vec![AdapterOutput::DuplicateIgnored]),
            Some(_) => Err(AdapterError::TerminalConflict),
            slot @ None => {
                *slot = Some(outcome);
                let mut outputs = vec![AdapterOutput::ToolTerminal {
                    external_id: id,
                    outcome,
                    output,
                }];
                outputs.extend(images);
                Ok(outputs)
            }
        }
    }
}

impl PiTranslator {
    pub(crate) fn latch_authority_denial(&mut self) {
        self.close_config_requests();
        if let Some(turn) = self.active.as_mut() {
            turn.authority_denied = true;
        }
        self.pending_ui = None;
    }
}

fn required_tool_id(value: &Value) -> Result<String, AdapterError> {
    external_id(
        value
            .get("toolCallId")
            .or_else(|| value.get("tool_call_id"))
            .or_else(|| value.get("id")),
    )?
    .ok_or(AdapterError::Malformed)
}

fn message_id(value: &Value) -> Result<Option<String>, AdapterError> {
    external_id(
        value
            .get("message")
            .and_then(|message| message.get("id"))
            .or_else(|| value.get("messageId"))
            .or_else(|| value.get("message_id")),
    )
}

fn assistant_message(value: &Value) -> bool {
    value
        .get("message")
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        .is_none_or(|role| role == "assistant")
}
