use serde_json::Map;

use super::{PENDING_COMMANDS, PendingKind, PiTranslator};
use crate::acp::bindings::pi::protocol::{AdapterError, AdapterOutput, Outbound};

#[derive(Debug)]
pub(super) struct InitializationIntent {
    pub(super) state_id: String,
    pub(super) models_id: String,
    state_done: bool,
    models_done: bool,
}

impl PiTranslator {
    pub(super) fn begin_initialize(&mut self) -> Result<Vec<Outbound>, AdapterError> {
        if self.initialization.is_some() {
            return Err(AdapterError::InitializationAlreadyActive);
        }
        if self.pending.len() > PENDING_COMMANDS - 2 {
            return Err(AdapterError::CommandCapacity);
        }
        let state = self.issue(PendingKind::State, Map::new())?;
        let models = self.issue(PendingKind::Models, Map::new())?;
        self.initialization = Some(InitializationIntent {
            state_id: state.command_id.clone(),
            models_id: models.command_id.clone(),
            state_done: false,
            models_done: false,
        });
        Ok(vec![state, models])
    }

    pub(super) fn note_initialization(
        &mut self,
        response_id: &str,
        kind: PendingKind,
        output: &mut Vec<AdapterOutput>,
    ) {
        let Some(intent) = self.initialization.as_mut() else {
            return;
        };
        match kind {
            PendingKind::State if intent.state_id == response_id => intent.state_done = true,
            PendingKind::Models if intent.models_id == response_id => intent.models_done = true,
            _ => return,
        }
        if intent.state_done && intent.models_done {
            self.initialization = None;
            output.push(AdapterOutput::Initialized);
        } else {
            output.push(AdapterOutput::InitializationProgress);
        }
    }

    pub(super) fn reset_initialization(&mut self) {
        let Some(intent) = self.initialization.take() else {
            return;
        };
        self.pending.remove(&intent.state_id);
        self.pending.remove(&intent.models_id);
        self.closed_config_requests.remove(&intent.state_id);
    }
}
