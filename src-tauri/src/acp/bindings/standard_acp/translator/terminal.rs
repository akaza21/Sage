use serde_json::Value;

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::NormalizedPayload,
    lifecycle::{ItemLifecycle, TurnState},
};

use super::super::{
    sanitize::normalized_text, surface::HARNESS_RPC_ERROR_CODE, wire::decode_frame,
};
use super::{
    AdapterEvent, MAX_EVENTS_PER_FRAME, StandardAcpTranslator, TranslationError,
    decode::{rpc_id, usage_event, usage_object},
};

const MAX_TERMINAL_REQUESTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptTerminal {
    Completed,
    Cancelled,
    Failed,
}

impl PromptTerminal {
    pub(crate) fn turn_state(self) -> TurnState {
        match self {
            Self::Completed => TurnState::Completed,
            Self::Cancelled => TurnState::Cancelled,
            Self::Failed => TurnState::Failed,
        }
    }
}

impl StandardAcpTranslator {
    pub(crate) fn translate_prompt_result(
        &mut self,
        raw: &[u8],
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        self.translate_prompt_result_value(frame)
    }

    pub(crate) fn translate_prompt_result_value(
        &mut self,
        frame: Value,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let request_id = rpc_id(frame.get("id"))?;
        let error = frame.get("error").and_then(Value::as_object);
        let error_message = error
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str);
        let error_code = error
            .and_then(|error| error.get("data"))
            .and_then(|data| data.get("errorKind"))
            .and_then(Value::as_str)
            .unwrap_or(HARNESS_RPC_ERROR_CODE);
        let terminal = self.prompt_terminal(&frame)?;
        if let Some(previous) = self.terminal_ledger.get(request_id.as_str()) {
            return if *previous == terminal {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::TerminalConflict)
            };
        }
        if let Some(current) = self.terminal {
            return if self.terminal_request_id.as_deref() == Some(request_id.as_str())
                && current == terminal
            {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::TerminalConflict)
            };
        }
        if self.pending_prompt.as_deref() != Some(request_id.as_str()) {
            return Err(TranslationError::ForeignId);
        }
        let running = self.turn_running();
        self.pending_prompt = None;
        self.record_terminal(request_id.as_str(), terminal);
        let usage = frame
            .get("result")
            .and_then(Value::as_object)
            .and_then(|result| usage_event(usage_object(result), self.context_window));
        let mut events = Vec::new();
        events.extend(running);
        if let Some(message) = error_message {
            events.push(AdapterEvent::Item {
                external_key: RequiredText::new(
                    "externalItemId",
                    format!("error:{}", request_id.as_str()),
                )?,
                lifecycle: ItemLifecycle::Failed,
                tool_state: None,
                payload: NormalizedPayload::Error {
                    code: SafeText::new("errorCode", error_code)?,
                    message: normalized_text("errorMessage", message)?,
                    retryable: false,
                },
            });
        }
        if error_message.is_none()
            && frame
                .get("result")
                .and_then(|result| result.get("stopReason"))
                .and_then(Value::as_str)
                == Some("refusal")
        {
            let harness = match self.profile.harness_id {
                "claude-code" => "Claude Code",
                "gemini-cli" => "Gemini CLI",
                "cursor-agent" => "Cursor",
                "opencode" => "OpenCode",
                _ => "the harness",
            };
            events.push(AdapterEvent::Item {
                external_key: RequiredText::new("externalItemId", "harness-refusal")?,
                lifecycle: ItemLifecycle::Failed,
                tool_state: None,
                payload: NormalizedPayload::Error {
                    code: SafeText::new("errorCode", "harnessRefusal")?,
                    message: SafeText::new("errorMessage", format!("Declined by {harness}"))?,
                    retryable: false,
                },
            });
        }
        events.extend(usage);
        self.drain_diffs(
            terminal == PromptTerminal::Completed,
            MAX_EVENTS_PER_FRAME.saturating_sub(1),
            &mut events,
        )?;
        events.push(AdapterEvent::TurnTerminal(terminal));
        self.clear_turn_items();
        if events.len() > MAX_EVENTS_PER_FRAME {
            return Err(TranslationError::EventLimit);
        }
        Ok(events)
    }

    pub(crate) fn reject_prompt(&mut self, request_id: &str) -> Result<(), TranslationError> {
        if self.pending_prompt.as_deref() != Some(request_id) || self.terminal.is_some() {
            return Err(TranslationError::ForeignId);
        }
        self.pending_prompt = None;
        self.clear_turn_items();
        Ok(())
    }

    fn prompt_terminal(&self, frame: &Value) -> Result<PromptTerminal, TranslationError> {
        if frame.get("error").is_some() {
            return Ok(PromptTerminal::Failed);
        }
        match frame
            .get("result")
            .and_then(|result| result.get("stopReason"))
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?
        {
            "end_turn" => Ok(PromptTerminal::Completed),
            "cancelled" => Ok(PromptTerminal::Cancelled),
            "refusal" | "max_tokens" | "max_turn_requests" => Ok(PromptTerminal::Failed),
            _ => Err(TranslationError::UnknownTerminal),
        }
    }

    fn record_terminal(&mut self, request_id: &str, terminal: PromptTerminal) {
        self.terminal_request_id = Some(request_id.to_owned());
        self.terminal = Some(terminal);
        if self.terminal_ledger.len() >= MAX_TERMINAL_REQUESTS
            && let Some(oldest) = self.terminal_ledger.keys().next().cloned()
        {
            self.terminal_ledger.remove(&oldest);
        }
        self.terminal_ledger.insert(request_id.to_owned(), terminal);
    }
}
