use serde_json::Value;

use super::{ActiveTurn, EventPhase, PiTranslator, TOOLS_PER_TURN};
use crate::acp::bindings::pi::{
    decode::{ITEM_BYTES, bounded_string, external_id, redact_structured_text},
    protocol::{AdapterError, AdapterOutput, TerminalOutcome, ToolOutcome},
};

impl PiTranslator {
    pub(super) fn ingest_event(
        &mut self,
        kind: &str,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if kind != "agent_end"
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.terminal.is_some())
        {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        if kind != "agent_end" && self.active.as_ref().is_some_and(|turn| turn.denial_latched) {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        match kind {
            "agent_start" => {
                let turn = self.live_turn_any_phase()?;
                if turn.event_phase != EventPhase::PromptAccepted {
                    return Err(AdapterError::EventSequence);
                }
                turn.event_phase = EventPhase::AgentRunning;
                Ok(vec![AdapterOutput::TurnRunning {
                    turn_key: turn.turn_key.clone(),
                }])
            }
            "agent_end" => {
                let active = self.active.as_ref().ok_or(AdapterError::StaleIntent)?;
                if active.terminal.is_none() && active.event_phase != EventPhase::AgentRunning {
                    return Err(AdapterError::EventSequence);
                }
                let outcome = if active.abort_requested || active.denial_latched {
                    TerminalOutcome::Interrupted
                } else {
                    TerminalOutcome::Completed
                };
                let is_duplicate = active.terminal == Some(outcome);
                let mut output = if is_duplicate {
                    Vec::new()
                } else {
                    parse_usage(value).into_iter().collect()
                };
                output.extend(self.finish_turn(outcome)?);
                Ok(output)
            }
            "message_start" => {
                self.live_turn()?;
                Ok(vec![AdapterOutput::TextStarted {
                    external_id: message_id(value)?,
                }])
            }
            "message_update" => {
                self.live_turn()?;
                let delta = value
                    .get("assistantMessageEvent")
                    .or_else(|| value.get("assistant_message_event"))
                    .and_then(|event| event.get("delta"))
                    .or_else(|| value.get("delta"));
                let Some(text) = bounded_string(delta, ITEM_BYTES)? else {
                    return Ok(vec![AdapterOutput::Unsupported {
                        kind: "message_update_without_text_delta".into(),
                    }]);
                };
                Ok(vec![AdapterOutput::TextDelta { text }])
            }
            "message_end" => {
                self.live_turn()?;
                Ok(vec![AdapterOutput::TextCompleted {
                    external_id: message_id(value)?,
                }])
            }
            "tool_execution_start" => self.tool_start(value),
            "tool_execution_update" => self.tool_update(value),
            "tool_execution_end" => self.tool_end(value),
            "turn_start"
            | "turn_end"
            | "auto_compaction_start"
            | "auto_compaction_end"
            | "auto_retry_start"
            | "auto_retry_end" => {
                self.live_turn()?;
                Ok(vec![AdapterOutput::Unsupported {
                    kind: kind.to_owned(),
                }])
            }
            "extension_ui_request" | "permission_request" => {
                self.latch_denial()?;
                let id = external_id(
                    value
                        .get("id")
                        .or_else(|| value.get("requestId"))
                        .or_else(|| value.get("request_id")),
                )?;
                let method = value
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let options = value
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|options| {
                        options
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(id) = id.clone() {
                    self.remember_pending_ui(super::PendingUiRequest {
                        id,
                        method,
                        options,
                    });
                }
                Ok(vec![AdapterOutput::PermissionDeclined {
                    external_id: id,
                    tool: "extension request".into(),
                }])
            }
            _ => {
                self.live_turn()?;
                Ok(vec![AdapterOutput::Unsupported {
                    kind: redact_structured_text(kind),
                }])
            }
        }
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

    fn latch_denial(&mut self) -> Result<(), AdapterError> {
        self.live_turn()?.denial_latched = true;
        Ok(())
    }

    pub(super) fn finish_turn(
        &mut self,
        incoming: TerminalOutcome,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
        match turn.terminal {
            Some(current) if current == incoming => Ok(vec![AdapterOutput::DuplicateIgnored]),
            Some(_) => Err(AdapterError::TerminalConflict),
            None => {
                turn.terminal = Some(incoming);
                Ok(vec![AdapterOutput::TurnTerminal {
                    turn_key: turn.turn_key.clone(),
                    outcome: incoming,
                }])
            }
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
            self.live_turn()?.denial_latched = true;
            self.remember_pending_ui(super::PendingUiRequest {
                id: id.clone(),
                method: String::new(),
                options: Vec::new(),
            });
            return Ok(vec![AdapterOutput::PermissionDeclined {
                external_id: Some(id),
                tool: name,
            }]);
        }
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
        Ok(vec![AdapterOutput::ToolUpdated { external_id: id }])
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
                Ok(vec![AdapterOutput::ToolTerminal {
                    external_id: id,
                    outcome,
                }])
            }
        }
    }
}

impl PiTranslator {
    pub(crate) fn release_permission_wait(&mut self) {
        if let Some(turn) = self.active.as_mut() {
            turn.denial_latched = false;
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

fn parse_usage(value: &Value) -> Option<AdapterOutput> {
    let usage = value.get("usage")?;
    Some(AdapterOutput::Usage {
        input_tokens: usage
            .get("inputTokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("outputTokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}
