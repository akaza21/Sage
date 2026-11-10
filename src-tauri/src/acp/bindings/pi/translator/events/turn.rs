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
        let message = value
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().rev().find(|message| {
                    message.get("role").and_then(Value::as_str) == Some("assistant")
                })
            });
        self.end_run(message)
    }

    /// An oversized event of the running Turn was dropped unread.
    pub(crate) fn note_dropped_frame(&mut self) {
        if let Some(turn) = self.active.as_mut() {
            turn.dropped_frame = true;
        }
    }

    /// Whether an oversized id-less frame may be read past. Only between
    /// the items of a running model loop with no read outstanding, so the
    /// frame can only repeat what the stream already delivered, such as the
    /// run's `agent_end`. Inside a reply, tool call or compaction, or between
    /// Turns, an oversized frame still ends the session.
    pub(crate) fn skips_oversize_frames(&self) -> bool {
        !self.awaits_large_response()
            && self.active.as_ref().is_some_and(|turn| {
                turn.lifecycle.phase == TurnPhase::ModelRunning
                    && !turn.reply_open
                    && !turn.compaction_active
                    && turn.tools.values().all(Option::is_some)
            })
    }

    pub(super) fn agent_start(&mut self) -> Result<Vec<AdapterOutput>, AdapterError> {
        // A retry or a continuation after compaction starts a new run, so a
        // previous run whose `agent_end` was dropped ends here first.
        let mut output = self.end_dropped_run()?;
        let turn = self.live_turn_any_phase()?;
        turn.lifecycle.start_model()?;
        turn.settlement_message = None;
        turn.last_reply = None;
        output.push(AdapterOutput::TurnRunning {
            turn_key: turn.turn_key.clone(),
        });
        Ok(output)
    }

    /// Ends a run whose `agent_end` was too large to read from the last
    /// reply the stream already delivered.
    fn end_dropped_run(&mut self) -> Result<Vec<AdapterOutput>, AdapterError> {
        let Some(turn) = self.active.as_mut() else {
            return Ok(Vec::new());
        };
        if turn.lifecycle.phase != TurnPhase::ModelRunning || !turn.dropped_frame {
            return Ok(Vec::new());
        }
        let reply = turn.last_reply.take();
        self.end_run(reply.as_deref())
    }

    /// Ends the model run from its last assistant reply.
    fn end_run(&mut self, message: Option<&Value>) -> Result<Vec<AdapterOutput>, AdapterError> {
        let turn = self.session_turn()?;
        turn.dropped_frame = false;
        if !turn.lifecycle.end_model()? {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
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
        let turn = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
        if turn.lifecycle.terminal().is_some() {
            return Ok(vec![AdapterOutput::DuplicateIgnored]);
        }
        // The run's `agent_end` was too large to read: end the run from the
        // last reply the stream already delivered.
        let mut output = self.end_dropped_run()?;
        let turn = self.active.as_ref().ok_or(AdapterError::StaleIntent)?;
        if turn.lifecycle.phase != TurnPhase::ModelEnded {
            return Err(AdapterError::EventSequence);
        }
        let outcome = if turn.retry_failed {
            TerminalOutcome::Failed
        } else {
            turn.settlement_outcome
        };
        output.extend(self.finish_turn(outcome)?);
        Ok(output)
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

/// Pi retrying a compaction or branch summary is shown as a notice; the
/// compaction itself already has its own item.
pub(super) fn summary_retry(value: &Value) -> AdapterOutput {
    let attempt = |key| value.get(key).and_then(Value::as_u64).unwrap_or(1);
    let reason = super::display_text(value.get("errorMessage"), 512)
        .map_or_else(String::new, |reason| format!(": {reason}"));
    let text = Value::String(format!(
        "Pi is retrying a summary (attempt {} of {}){reason}",
        attempt("attempt"),
        attempt("maxAttempts"),
    ));
    super::notice(super::SessionNoticeLevel::Warning, "Pi", Some(&text))
}
