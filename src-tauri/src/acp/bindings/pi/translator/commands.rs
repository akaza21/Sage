use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{
    ActiveTurn, PENDING_COMMANDS, PendingKind, PendingRequest, PiTranslator, TurnLifecycle,
    TurnPhase, validate_id,
};
use crate::acp::bindings::pi::{
    decode::PROMPT_BYTES,
    protocol::{AdapterError, Outbound},
};

impl PiTranslator {
    pub(crate) fn mark_extension_command(&mut self) -> Result<(), AdapterError> {
        let active = self.active.as_mut().ok_or(AdapterError::StaleIntent)?;
        active.lifecycle.mark_extension()
    }

    pub(crate) fn mark_prompt_written(&mut self, command_id: &str) -> Result<(), AdapterError> {
        let active = self.active.as_mut().ok_or(AdapterError::Correlation)?;
        if active.prompt_command_id != command_id {
            return Err(AdapterError::Correlation);
        }
        active.lifecycle.written()
    }

    pub(crate) fn reject_before_write(&mut self, command_id: &str) -> Result<(), AdapterError> {
        let request = *self
            .pending
            .get(command_id)
            .ok_or(AdapterError::Correlation)?;
        let kind = request.kind;
        if kind == PendingKind::Prompt {
            let active = self.active.as_ref().ok_or(AdapterError::Correlation)?;
            if active.prompt_command_id != command_id
                || active.lifecycle.phase != TurnPhase::Reserved
            {
                return Err(AdapterError::Correlation);
            }
        }
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
                self.active = None;
            }
            PendingKind::Abort => {
                if let Some(active) = self.active.as_mut()
                    && request.owner == Some(active.lifecycle.owner)
                    && active.lifecycle.terminal().is_none()
                {
                    active.lifecycle.resolve_abort(false);
                }
            }
            PendingKind::State
            | PendingKind::Models
            | PendingKind::SetModel
            | PendingKind::Commands
            | PendingKind::ThinkingLevels
            | PendingKind::SetThinkingLevel
            | PendingKind::NewSession
            | PendingKind::Resume
            | PendingKind::Steer
            | PendingKind::ClearQueue
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
        validate_message(&message)?;
        if self
            .pending
            .values()
            .any(|request| matches!(request.kind, PendingKind::NewSession | PendingKind::Resume))
        {
            return Err(AdapterError::SessionTransitionBusy);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.lifecycle.terminal().is_none())
        {
            return Err(AdapterError::PromptAlreadyActive);
        }
        let mut fields = Map::new();
        fields.insert("message".into(), Value::String(message));
        let outbound = self.issue(PendingKind::Prompt, fields)?;
        self.active = Some(ActiveTurn {
            turn_key,
            prompt_command_id: outbound.command_id.clone(),
            lifecycle: TurnLifecycle::new(self.next_command),
            authority_denied: false,
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

    /// Queues `message` into the running model loop; Pi delivers it after
    /// the current tool calls finish. Once the loop has ended Pi would keep it
    /// for the next prompt, so a steer is refused outside `ModelRunning`.
    pub(super) fn begin_steer(&mut self, message: String) -> Result<Vec<Outbound>, AdapterError> {
        validate_message(&message)?;
        if self.active.as_ref().is_none_or(|turn| {
            turn.authority_denied
                || turn.lifecycle.terminal().is_some()
                || turn.lifecycle.phase != TurnPhase::ModelRunning
        }) {
            return Err(AdapterError::StaleIntent);
        }
        let mut fields = Map::new();
        fields.insert("message".into(), Value::String(message));
        self.one(PendingKind::Steer, fields)
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
        let owner = match kind {
            PendingKind::Prompt => Some(self.next_command),
            PendingKind::Abort | PendingKind::Steer => {
                self.active.as_ref().map(|turn| turn.lifecycle.owner)
            }
            _ => None,
        };
        self.pending
            .insert(id.clone(), PendingRequest { kind, owner });
        Ok(Outbound {
            command_id: id,
            line,
        })
    }
}

fn validate_message(message: &str) -> Result<(), AdapterError> {
    if message.is_empty() || message.len() > PROMPT_BYTES || message.contains('\0') {
        return Err(AdapterError::Limit("prompt bytes"));
    }
    Ok(())
}
