use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{ActiveTurn, EventPhase, PENDING_COMMANDS, PendingKind, PiTranslator, validate_id};
use crate::acp::bindings::pi::{
    decode::PROMPT_BYTES,
    protocol::{AdapterError, Outbound},
};

impl PiTranslator {
    pub(crate) fn reject_before_write(&mut self, command_id: &str) -> Result<(), AdapterError> {
        let kind = *self
            .pending
            .get(command_id)
            .ok_or(AdapterError::Correlation)?;
        if self
            .initialization
            .as_ref()
            .is_some_and(|intent| intent.state_id == command_id || intent.models_id == command_id)
        {
            self.reset_initialization();
            return Ok(());
        }
        self.pending.remove(command_id);
        self.closed_config_requests.remove(command_id);
        if kind == PendingKind::SetThinkingLevel {
            self.pending_thinking = None;
        }
        match kind {
            PendingKind::Prompt => {
                let active = self.active.as_ref().ok_or(AdapterError::Correlation)?;
                if active.prompt_command_id != command_id
                    || active.event_phase != EventPhase::PromptIssued
                {
                    return Err(AdapterError::Correlation);
                }
                self.active = None;
            }
            PendingKind::Abort => {
                self.active
                    .as_mut()
                    .ok_or(AdapterError::Correlation)?
                    .abort_requested = false;
            }
            PendingKind::State
            | PendingKind::Models
            | PendingKind::SetModel
            | PendingKind::ThinkingLevels
            | PendingKind::SetThinkingLevel
            | PendingKind::NewSession
            | PendingKind::Messages
            | PendingKind::Entries => {}
        }
        Ok(())
    }

    pub(super) fn begin_prompt(
        &mut self,
        turn_key: String,
        message: String,
    ) -> Result<Vec<Outbound>, AdapterError> {
        if self.launch_admission.is_none() {
            return Err(AdapterError::LaunchNotAdmitted);
        }
        validate_id("Turn key", &turn_key)?;
        if message.is_empty() || message.len() > PROMPT_BYTES || message.contains('\0') {
            return Err(AdapterError::Limit("prompt bytes"));
        }
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_none())
        {
            return Err(AdapterError::PromptAlreadyActive);
        }
        let mut fields = Map::new();
        fields.insert("message".into(), Value::String(message));
        let outbound = self.issue(PendingKind::Prompt, fields)?;
        self.active = Some(ActiveTurn {
            turn_key,
            prompt_command_id: outbound.command_id.clone(),
            event_phase: EventPhase::PromptIssued,
            abort_requested: false,
            authority_denied: false,
            terminal: None,
            tools: BTreeMap::new(),
            compaction_active: false,
            thought_active: false,
            settlement_outcome: super::TerminalOutcome::Completed,
            settlement_message: None,
            retry_attempt: None,
            retry_notices: 0,
            retry_failed: false,
            retry_final_error: None,
            unknown_kinds: Default::default(),
            unknown_suppression_emitted: false,
        });
        Ok(vec![outbound])
    }

    pub(super) fn one(
        &mut self,
        kind: PendingKind,
        fields: Map<String, Value>,
    ) -> Result<Vec<Outbound>, AdapterError> {
        Ok(vec![self.issue(kind, fields)?])
    }

    pub(super) fn issue(
        &mut self,
        kind: PendingKind,
        mut fields: Map<String, Value>,
    ) -> Result<Outbound, AdapterError> {
        if self.pending.len() >= PENDING_COMMANDS {
            return Err(AdapterError::CommandCapacity);
        }
        self.next_command = self
            .next_command
            .checked_add(1)
            .ok_or(AdapterError::CommandCapacity)?;
        let id = format!("pi-{}-{}", self.epoch, self.next_command);
        fields.insert("type".into(), Value::String(kind.command().into()));
        fields.insert("id".into(), Value::String(id.clone()));
        let line =
            serde_json::to_string(&Value::Object(fields)).map_err(|_| AdapterError::Malformed)?;
        if line
            .len()
            .checked_add(1)
            .is_none_or(|wire_bytes| wire_bytes > super::super::decode::FRAME_BYTES)
        {
            return Err(AdapterError::Limit("outbound frame bytes"));
        }
        self.pending.insert(id.clone(), kind);
        Ok(Outbound {
            command_id: id,
            line,
        })
    }
}
