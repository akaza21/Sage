//! User messages an extension sends with `pi.sendUserMessage`. Pi echoes
//! every user message, Sage's own prompts and steers included, and the echo
//! does not say who sent it. The bridge marks each extension-sent message
//! (Pi's `input` event with source `extension`) with its text's SHA-256, so
//! an echo is shown once per marker and every other echo stays absorbed.
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::PiTranslator;
use crate::acp::bindings::pi::{
    decode::redact_structured_text,
    protocol::{AdapterError, AdapterOutput},
};
use crate::acp::redaction::text::{clip_display, neutralized};
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload};

/// Extension-sent messages awaiting their echo in one turn. A marker past
/// this drops the oldest, whose echo is then absorbed like Sage's own.
const INJECTED_USER_MESSAGES_PER_TURN: usize = 8;
/// The most of one injected message's text shown.
const INJECTED_TEXT_BYTES: usize = 65_536;

impl PiTranslator {
    /// Records one extension-sent message for the live turn; without one
    /// Sage shows no turn for it, so the marker is dropped.
    pub(crate) fn mark_injected_user_message(&mut self, digest: [u8; 32]) {
        let Ok(turn) = self.live_turn_any_phase() else {
            return;
        };
        if turn.injected_user_messages.len() >= INJECTED_USER_MESSAGES_PER_TURN {
            turn.injected_user_messages.pop_front();
        }
        turn.injected_user_messages.push_back(digest);
    }

    /// A marker whose message a later `input` handler rewrote or swallowed
    /// never meets its echo. Pi's agent loop can hold a steer or follow-up
    /// across several assistant messages, so no assistant boundary proves it
    /// stale; only `agent_end`, after both queues drain, does. Until then a
    /// Sage steer of the same text takes one such marker, so its own echo
    /// cannot be shown twice (a real injected twin then stays hidden).
    pub(in crate::acp::bindings::pi::translator) fn settle_injected(
        &mut self,
        sage_steer: Option<&str>,
    ) {
        let Some(turn) = self.active.as_mut() else {
            return;
        };
        let pending = &mut turn.injected_user_messages;
        match sage_steer {
            None => pending.clear(),
            Some(text) => {
                let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
                if let Some(index) = pending.iter().position(|marked| *marked == digest) {
                    pending.remove(index);
                }
            }
        }
    }

    pub(super) fn user_echo(
        &mut self,
        message: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        // `sendUserMessage` joins text parts with newlines; Pi echoes the
        // joined text as one part beside any images.
        let text = match &message["content"] {
            Value::String(text) => text.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter(|part| part["type"] == "text")
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => return Ok(vec![AdapterOutput::Absorbed]),
        };
        let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
        let pending = &mut self.live_turn_any_phase()?.injected_user_messages;
        let Some(index) = pending.iter().position(|marked| *marked == digest) else {
            return Ok(vec![AdapterOutput::Absorbed]);
        };
        pending.remove(index);
        let shown = clip_display(
            &redact_structured_text(&neutralized(&text, true)),
            INJECTED_TEXT_BYTES,
        );
        Ok(vec![AdapterOutput::Message(
            NormalizedPayload::UserPrompt {
                text: SafeText::new("injectedUserMessage", shown)
                    .map_err(|_| AdapterError::Malformed)?,
            },
        )])
    }
}
