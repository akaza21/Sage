use super::super::{ActiveTurn, EventPhase, PiTranslator};
use crate::acp::bindings::pi::{AdapterError, AdapterOutput, TerminalOutcome};
use serde_json::Value;

impl PiTranslator {
    pub(super) fn session_turn(&mut self) -> Result<&mut ActiveTurn, AdapterError> {
        let turn = self.live_turn_any_phase()?;
        if turn.event_phase == EventPhase::PromptIssued {
            return Err(AdapterError::EventSequence);
        }
        Ok(turn)
    }

    pub(super) fn agent_end(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.session_turn()?;
        if turn.event_phase == EventPhase::AgentEnded {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
        if turn.event_phase != EventPhase::AgentRunning {
            return Err(AdapterError::EventSequence);
        }
        turn.event_phase = EventPhase::AgentEnded;
        let message = value
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().rev().find(|message| {
                    message.get("role").and_then(Value::as_str) == Some("assistant")
                })
            });
        turn.settlement_outcome = match message
            .and_then(|message| message.get("stopReason"))
            .and_then(Value::as_str)
        {
            Some("error") => TerminalOutcome::Failed,
            Some("aborted") => TerminalOutcome::Interrupted,
            _ => TerminalOutcome::Completed,
        };
        Ok(message
            .and_then(|message| message.get("usage"))
            .map(|usage| AdapterOutput::Usage {
                input_tokens: usage.get("input").and_then(Value::as_u64).unwrap_or(0),
                output_tokens: usage.get("output").and_then(Value::as_u64).unwrap_or(0),
            })
            .into_iter()
            .collect())
    }

    pub(super) fn agent_settled(&mut self) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.active.as_ref().ok_or(AdapterError::StaleIntent)?;
        if turn.terminal.is_none() && turn.event_phase != EventPhase::AgentEnded {
            return Err(AdapterError::EventSequence);
        }
        let outcome = if turn.abort_requested || turn.denial_latched {
            TerminalOutcome::Interrupted
        } else {
            turn.settlement_outcome
        };
        self.finish_turn(outcome)
    }
}
