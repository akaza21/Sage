use serde_json::Value;

use super::super::PiTranslator;
use super::{assistant_message, message_id, turn::failure_detail};
use crate::acp::bindings::pi::{
    decode::{ITEM_BYTES, bounded_delta},
    protocol::{AdapterError, AdapterOutput, TerminalOutcome},
};

impl PiTranslator {
    pub(super) fn message_end(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        self.live_turn()?;
        if !assistant_message(value) {
            return Ok(Vec::new());
        }
        let message = value.get("message");
        let outcome = match message
            .and_then(|message| message.get("stopReason"))
            .and_then(Value::as_str)
        {
            Some("aborted") => TerminalOutcome::Interrupted,
            Some("error") => TerminalOutcome::Failed,
            _ => TerminalOutcome::Completed,
        };
        self.live_turn()?.settlement_message = if outcome == TerminalOutcome::Failed {
            failure_detail(message.and_then(|message| message.get("errorMessage")))
        } else {
            None
        };
        let mut output = self.complete_thought(outcome);
        output.push(AdapterOutput::TextCompleted {
            external_id: message_id(value)?,
            outcome,
        });
        Ok(output)
    }

    pub(super) fn message_update(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        self.live_turn_any_phase()?;
        let event = value
            .get("assistantMessageEvent")
            .or_else(|| value.get("assistant_message_event"));
        match event
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
        {
            Some(
                "text_start" | "text_end" | "toolcall_start" | "toolcall_delta" | "toolcall_end",
            ) => {
                return Ok(vec![AdapterOutput::Absorbed]);
            }
            Some("thinking_start") => {
                self.live_turn()?.thought_active = true;
                return Ok(vec![AdapterOutput::ThoughtStarted]);
            }
            Some("thinking_end") => {
                self.live_turn()?;
                return Ok(self.complete_thought(TerminalOutcome::Completed));
            }
            _ => {}
        }
        self.live_turn()?;
        let delta = event.and_then(|event| event.get("delta"));
        let Some((text, redacted_tail)) = bounded_delta(delta, ITEM_BYTES)? else {
            return self.unknown_event("message_update_without_text_delta");
        };
        match event
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
        {
            Some("thinking_delta") => {
                self.live_turn()?.thought_active = true;
                Ok(vec![AdapterOutput::ThoughtDelta {
                    text,
                    redacted_tail,
                }])
            }
            Some("text_delta") => Ok(vec![AdapterOutput::TextDelta {
                text,
                redacted_tail,
            }]),
            _ => self.unknown_event("message_update_non_text_delta"),
        }
    }
}
