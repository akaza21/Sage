use crate::acp::bindings::pi::decode::redact_structured_text;
use crate::acp::redaction::text::{clip_display, neutralized};
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload};
use serde_json::Value;

/// Displayed extension messages shown in one turn; the next adds one notice
/// that further ones are hidden, and the rest are dropped.
const EXTENSION_MESSAGES_PER_TURN: u8 = 64;

use super::super::PiTranslator;
use super::{assistant_message, message_id, turn::failure_detail};
use crate::acp::bindings::pi::{
    decode::{ITEM_BYTES, bounded_text},
    protocol::{AdapterError, AdapterOutput, TerminalOutcome},
};

impl PiTranslator {
    pub(super) fn message_end(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        self.live_turn_any_phase()?;
        if !assistant_message(value) {
            return self.other_message(&value["message"]);
        }
        let message = value.get("message");
        let kept = |key| message.and_then(|message| message.get(key)).cloned();
        self.live_turn()?.reply_open = false;
        self.live_turn()?.last_reply = Some(Box::new(serde_json::json!({
            "role": "assistant",
            "stopReason": kept("stopReason"),
            "errorMessage": kept("errorMessage"),
            "usage": kept("usage"),
        })));
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

    fn other_message(&mut self, message: &Value) -> Result<Vec<AdapterOutput>, AdapterError> {
        let role = message["role"].as_str().unwrap_or("");
        if role == "custom" {
            match message["display"].as_bool() {
                Some(false) => return Ok(vec![AdapterOutput::Absorbed]),
                Some(true) => {}
                None => return self.unknown_event("custom_message_without_display"),
            }
        }
        // Pi 1.5.0 roles Sage renders elsewhere or that only carry Pi's own
        // context: tool results, system prompt sections (`message-types.md`),
        // RPC shell runs Sage never starts, and summary context messages.
        if matches!(
            role,
            "toolResult" | "system" | "bashExecution" | "branchSummary" | "compactionSummary"
        ) {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        // Pi echoes prompts and steers as user messages; Sage already shows
        // those. An extension's injected user turn has no reliable marker yet.
        if role == "user" {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        if role != "custom" {
            return self.unknown_event("message_role");
        }
        let Some(label) = message["customType"].as_str() else {
            return self.unknown_event("custom_message_without_type");
        };
        let turn = self.live_turn_any_phase()?;
        if turn.extension_messages > EXTENSION_MESSAGES_PER_TURN {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        turn.extension_messages += 1;
        if turn.extension_messages > EXTENSION_MESSAGES_PER_TURN {
            return Ok(vec![AdapterOutput::Message(
                NormalizedPayload::ExtensionMessage {
                    label: SafeText::new("extensionLabel", "Extension messages".to_owned())
                        .map_err(|_| AdapterError::Malformed)?,
                    text: SafeText::new(
                        "extensionText",
                        "Further extension messages in this turn are hidden.".to_owned(),
                    )
                    .map_err(|_| AdapterError::Malformed)?,
                    details: None,
                },
            )]);
        }
        let mut unsupported = false;
        let text = match &message["content"] {
            Value::String(text) => text.clone(),
            Value::Array(parts) => {
                let mut text = String::new();
                for part in parts {
                    if part["type"] == "text"
                        && let Some(value) = part["text"].as_str()
                    {
                        text.push_str(value);
                    } else {
                        unsupported = true;
                    }
                }
                text
            }
            _ => {
                unsupported = true;
                String::new()
            }
        };
        let clean =
            |text: &str, cap| clip_display(&redact_structured_text(&neutralized(text, true)), cap);
        let payload = NormalizedPayload::ExtensionMessage {
            label: SafeText::new("extensionLabel", clean(label, 128))
                .map_err(|_| AdapterError::Malformed)?,
            text: SafeText::new("extensionText", clean(&text, 16384))
                .map_err(|_| AdapterError::Malformed)?,
            details: message
                .get("details")
                .map(|details| {
                    SafeText::new("extensionDetails", clean(&details.to_string(), 16384))
                })
                .transpose()
                .map_err(|_| AdapterError::Malformed)?,
        };
        let mut output = vec![AdapterOutput::Message(payload)];
        if unsupported {
            output.extend(self.unknown_event("message_content")?);
        }
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
        let Some(text) = bounded_text(delta, ITEM_BYTES)? else {
            return self.unknown_event("message_update_without_text_delta");
        };
        match event
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
        {
            Some("thinking_delta") => {
                self.live_turn()?.thought_active = true;
                Ok(vec![AdapterOutput::ThoughtDelta { text }])
            }
            Some("text_delta") => Ok(vec![AdapterOutput::TextDelta { text }]),
            _ => self.unknown_event("message_update_non_text_delta"),
        }
    }
}
