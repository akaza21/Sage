use super::super::{ActiveTurn, PiTranslator, TurnPhase};
use crate::acp::bindings::pi::{AdapterError, AdapterOutput, TerminalOutcome};
use serde_json::Value;

impl PiTranslator {
    pub(super) fn session_turn(&mut self) -> Result<&mut ActiveTurn, AdapterError> {
        let turn = self.live_turn_any_phase()?;
        if matches!(
            turn.lifecycle.phase,
            TurnPhase::Reserved | TurnPhase::AwaitingAcknowledgment
        ) {
            return Err(AdapterError::EventSequence);
        }
        Ok(turn)
    }

    pub(super) fn agent_end(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.session_turn()?;
        if !turn.lifecycle.end_model()? {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
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
        turn.settlement_message = if turn.settlement_outcome == TerminalOutcome::Failed {
            failure_detail(message.and_then(|message| message.get("errorMessage")))
                .or_else(|| turn.settlement_message.take())
        } else {
            None
        };
        Ok(message
            .and_then(|message| message.get("usage"))
            .map(|usage| {
                let tokens = |key| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
                // Cached prompt tokens still occupy the context window.
                AdapterOutput::Usage {
                    input_tokens: tokens("input")
                        .saturating_add(tokens("cacheRead"))
                        .saturating_add(tokens("cacheWrite")),
                    output_tokens: tokens("output"),
                }
            })
            .into_iter()
            .collect())
    }

    pub(super) fn agent_settled(&mut self) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.active.as_ref().ok_or(AdapterError::StaleIntent)?;
        if turn.lifecycle.terminal().is_some() {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
        if turn.lifecycle.phase != TurnPhase::ModelEnded {
            return Err(AdapterError::EventSequence);
        }
        let outcome = if turn.retry_failed {
            TerminalOutcome::Failed
        } else {
            turn.settlement_outcome
        };
        self.finish_turn(outcome)
    }

    pub(super) fn retry_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        let Some(attempt) = value
            .get("attempt")
            .and_then(Value::as_u64)
            .filter(|attempt| *attempt > 0)
        else {
            return Ok(vec![AdapterOutput::Absorbed]);
        };
        let turn = self.session_turn()?;
        if turn.retry_attempt == Some(attempt) {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
        if turn.retry_notices >= 32 {
            return Err(AdapterError::Limit("retry notices per Turn"));
        }
        turn.retry_attempt = Some(attempt);
        turn.retry_notices += 1;
        turn.retry_failed = false;
        turn.retry_final_error = None;
        turn.settlement_message =
            failure_detail(value.get("errorMessage")).or_else(|| turn.settlement_message.take());
        Ok(vec![AdapterOutput::AttemptFailed {
            attempt,
            max_attempts: value
                .get("maxAttempts")
                .and_then(Value::as_u64)
                .filter(|maximum| *maximum >= attempt),
            message: turn.settlement_message.clone().unwrap_or_default(),
        }])
    }

    pub(super) fn retry_end(&mut self, value: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let Some(success) = value.get("success").and_then(Value::as_bool) else {
            return Ok(vec![AdapterOutput::Absorbed]);
        };
        let turn = self.session_turn()?;
        if success {
            turn.settlement_message = None;
            turn.retry_attempt = None;
            turn.retry_failed = false;
            turn.retry_final_error = None;
        } else {
            turn.retry_failed = true;
            turn.retry_final_error = failure_detail(value.get("finalError"));
        }
        Ok(vec![AdapterOutput::Absorbed])
    }
}

pub(super) fn failure_detail(value: Option<&Value>) -> Option<String> {
    let detail = value?.as_str()?;
    let detail: String = detail
        .chars()
        .filter_map(|character| {
            if !character.is_control() || matches!(character, '\n' | '\t') {
                Some(character)
            } else {
                character.is_whitespace().then_some(' ')
            }
        })
        .collect();
    if detail.trim().is_empty() {
        return None;
    }
    crate::acp::runtime::terminal_detail::protocol_error(detail.trim())
}
