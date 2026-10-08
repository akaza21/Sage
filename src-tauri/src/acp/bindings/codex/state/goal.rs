use serde_json::Value;

use super::{AdapterError, CodexAdapter};
use crate::acp::bindings::{characterisation::Characterisation, codex::request::OutgoingRequest};
use crate::acp::goal::{GoalSnapshot, GoalState};

impl CodexAdapter {
    pub(crate) fn goal_unavailable(&mut self) {
        if self.goal_supported()
            && matches!(self.goal_state, GoalState::Unsupported | GoalState::Loading)
        {
            self.goal_state = GoalState::Unavailable;
        }
    }

    pub(crate) fn goal_supported(&self) -> bool {
        self.negotiated.is_some()
            && self.observed_build_characterisation == Some(Characterisation::Characterised)
            && self.runtime_version.split_ascii_whitespace().next_back() == Some("0.146.0")
    }

    pub(crate) fn goal_state(&self) -> &GoalState {
        &self.goal_state
    }

    pub(crate) fn goal_get(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if !self.goal_supported() {
            return Err(AdapterError::CapabilityUnavailable);
        }
        let thread = self
            .external_thread_id
            .as_ref()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = OutgoingRequest::goal_get(id, thread)?;
        self.reserve(request.clone())?;
        self.goal_get_fence = Some((id, self.goal_notification_generation));
        self.goal_state = GoalState::Loading;
        Ok(request)
    }

    pub(crate) fn goal_updated(&mut self, value: &Value) {
        if !self.goal_supported() {
            return;
        }
        self.goal_notification_generation = self.goal_notification_generation.saturating_add(1);
        self.goal_state = match GoalSnapshot::from_wire(value) {
            Ok(snapshot) => GoalState::Goal { snapshot },
            Err(_) => GoalState::Unavailable,
        };
    }

    pub(crate) fn goal_cleared(&mut self) {
        if self.goal_supported() {
            self.goal_notification_generation = self.goal_notification_generation.saturating_add(1);
            self.goal_state = GoalState::NoGoal;
        }
    }

    pub(crate) fn goal_get_result(&mut self, id: u64, value: Option<&Value>) -> Result<(), String> {
        let Some((pending_id, generation)) = self.goal_get_fence.take() else {
            return Err("goal get was not pending".into());
        };
        if pending_id != id {
            return Err("foreign goal get response".into());
        }
        if generation != self.goal_notification_generation {
            return Ok(());
        }
        self.goal_state = match value {
            None => GoalState::Unavailable,
            Some(Value::Null) => GoalState::NoGoal,
            Some(goal) => match GoalSnapshot::from_wire(goal) {
                Ok(snapshot) => GoalState::Goal { snapshot },
                Err(_) => GoalState::Unavailable,
            },
        };
        Ok(())
    }

    pub(crate) fn goal_get_failed(&mut self, id: u64) {
        if self
            .goal_get_fence
            .is_some_and(|(pending_id, _)| pending_id == id)
        {
            self.goal_get_fence = None;
            if matches!(self.goal_state, GoalState::Loading) {
                self.goal_state = GoalState::Unavailable;
            }
        }
    }

    pub(crate) fn goal_get_delivery_unknown(&mut self, id: u64) {
        if self
            .goal_get_fence
            .is_some_and(|(pending_id, _)| pending_id == id)
            && matches!(self.goal_state, GoalState::Loading)
        {
            self.goal_state = GoalState::Unavailable;
        }
    }
}
