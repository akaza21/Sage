use serde_json::Value;

use super::{AdapterError, CodexAdapter};
use crate::acp::bindings::{characterisation::Characterisation, codex::request::OutgoingRequest};
use crate::acp::goal::{GoalAction, GoalSnapshot, GoalState, GoalStatus, valid_objective};

#[derive(Clone, PartialEq)]
pub(super) struct PendingGoalMutation {
    id: u64,
    generation: u64,
    objective: Option<String>,
    status: Option<GoalStatus>,
}

impl PendingGoalMutation {
    fn new(id: u64, generation: u64, action: &GoalAction) -> Self {
        let mut pending = Self {
            id,
            generation,
            objective: None,
            status: None,
        };
        match action {
            GoalAction::Set { objective, .. } => {
                pending.objective = Some(objective.clone());
            }
            GoalAction::ResumeExisting => pending.status = Some(GoalStatus::Active),
            GoalAction::PauseExisting => pending.status = Some(GoalStatus::Paused),
            GoalAction::ClearExisting => {}
        }
        pending
    }

    fn matches(&self, snapshot: &GoalSnapshot) -> bool {
        self.objective
            .as_ref()
            .is_none_or(|objective| *objective == snapshot.objective)
            && self
                .status
                .as_ref()
                .is_none_or(|status| *status == snapshot.status)
    }
}

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
            && match self.observed_build_characterisation {
                Some(characterisation) => {
                    characterisation.runs_tested_features()
                        && sage_acp_protocol::codex_compatibility::observed_capabilities(
                            &self.runtime_version,
                        )
                        .goals
                }
                None => {
                    self.capability_ceiling.protocol_identity
                        == sage_acp_protocol::codex_protocol::PINNED_PROTOCOL_IDENTITY
                        && self.runtime_version.split_ascii_whitespace().next_back()
                            == Some(sage_acp_protocol::codex_protocol::PINNED_CODEX_VERSION)
                }
            }
    }

    pub(crate) fn goal_state(&self) -> &GoalState {
        &self.goal_state
    }

    pub(crate) fn goal_mutation_pending(&self) -> bool {
        self.goal_mutation_fence.is_some()
    }

    pub(crate) fn active_observed_goal(&self) -> bool {
        self.goal_supported()
            && matches!(
                &self.goal_state,
                GoalState::Goal { snapshot }
                    if snapshot.status == crate::acp::goal::GoalStatus::Active
            )
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

    pub(crate) fn goal_pause(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.goal_mutation(id, &GoalAction::PauseExisting, OutgoingRequest::goal_pause)
    }

    pub(crate) fn goal_clear(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.goal_mutation(id, &GoalAction::ClearExisting, OutgoingRequest::goal_clear)
    }

    pub(crate) fn goal_set(
        &mut self,
        id: u64,
        action: &crate::acp::goal::GoalAction,
    ) -> Result<OutgoingRequest, AdapterError> {
        if let GoalAction::Set { expected_goal, .. } = action {
            let matches = match (&self.goal_state, expected_goal) {
                (GoalState::Goal { snapshot }, Some(expected)) => expected.matches(snapshot),
                (GoalState::NoGoal, None) => true,
                _ => false,
            };
            if !matches {
                return Err(AdapterError::GoalChanged);
            }
        }
        self.goal_mutation(id, action, |id, thread| {
            OutgoingRequest::goal_set(id, thread, action)
        })
    }

    fn goal_mutation(
        &mut self,
        id: u64,
        action: &GoalAction,
        build: impl FnOnce(
            u64,
            &sage_acp_protocol::contract::RequiredText<512>,
        ) -> Result<OutgoingRequest, super::super::request::RequestError>,
    ) -> Result<OutgoingRequest, AdapterError> {
        if !self.goal_supported()
            || !(matches!(self.goal_state, GoalState::Goal { .. })
                || matches!(action, GoalAction::Set { .. })
                    && matches!(self.goal_state, GoalState::NoGoal))
        {
            return Err(AdapterError::CapabilityUnavailable);
        }
        if self.goal_mutation_fence.is_some() {
            return Err(AdapterError::TurnIntentConflict);
        }
        let thread = self
            .external_thread_id
            .as_ref()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = build(id, thread)?;
        self.reserve(request.clone())?;
        self.goal_mutation_fence = Some(PendingGoalMutation::new(
            id,
            self.goal_notification_generation,
            action,
        ));
        Ok(request)
    }

    pub(crate) fn goal_mutation_failed(&mut self, id: u64) {
        if self
            .goal_mutation_fence
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.goal_mutation_fence = None;
        }
    }

    pub(crate) fn goal_mutation_delivery_unknown(&mut self, id: u64) {
        if self
            .goal_mutation_fence
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.goal_state = GoalState::Unavailable;
        }
    }

    pub(crate) fn goal_mutation_result(
        &mut self,
        id: u64,
        kind: super::super::request::RequestKind,
        result: Option<&Value>,
    ) -> bool {
        let Some(pending) = self.goal_mutation_fence.take() else {
            return false;
        };
        if pending.id != id {
            return false;
        }
        let next = match kind {
            super::super::request::RequestKind::GoalSet
            | super::super::request::RequestKind::GoalPause => result
                .and_then(|value| value.get("goal"))
                .and_then(|value| goal_snapshot(value).ok())
                .map(|snapshot| GoalState::Goal { snapshot }),
            super::super::request::RequestKind::GoalClear => match result
                .and_then(|value| value.get("cleared"))
                .and_then(Value::as_bool)
            {
                Some(true) => Some(GoalState::NoGoal),
                _ => None,
            },
            _ => None,
        };
        if let Some(next) = next {
            let applied = match &next {
                GoalState::Goal { snapshot } => pending.matches(snapshot),
                _ => true,
            };
            if pending.generation == self.goal_notification_generation {
                self.goal_state = next;
            }
            applied
        } else {
            false
        }
    }

    pub(crate) fn goal_updated(&mut self, value: &Value) {
        if !self.goal_supported() {
            return;
        }
        self.goal_notification_generation = self.goal_notification_generation.saturating_add(1);
        self.goal_state = match goal_snapshot(value) {
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
            Some(goal) => match goal_snapshot(goal) {
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

fn goal_snapshot(value: &Value) -> Result<GoalSnapshot, String> {
    let object = value.as_object().ok_or("goal object missing")?;
    let objective = object
        .get("objective")
        .and_then(Value::as_str)
        .filter(|s| valid_objective(s))
        .ok_or("goal objective invalid")?;
    let status = match object.get("status").and_then(Value::as_str) {
        Some("active") => GoalStatus::Active,
        Some("paused") => GoalStatus::Paused,
        Some("blocked") => GoalStatus::Blocked,
        Some("usageLimited") => GoalStatus::UsageLimited,
        Some("budgetLimited") => GoalStatus::BudgetLimited,
        Some("complete") => GoalStatus::Complete,
        _ => return Err("goal status invalid".into()),
    };
    let number = |key: &str| -> Result<u64, String> {
        object
            .get(key)
            .and_then(Value::as_u64)
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or_else(|| format!("goal {key} invalid"))
    };
    // The pinned ThreadGoal schema uses signed int64 timestamps. Preserve
    // their exact value as bounded decimal text in the shared projection.
    let timestamp = |key: &str| -> Result<Option<String>, String> {
        object
            .get(key)
            .and_then(Value::as_i64)
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| format!("goal {key} invalid"))
    };
    let token_budget = match object.get("tokenBudget") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .filter(|value| *value <= 9_007_199_254_740_991)
                .ok_or("goal tokenBudget invalid")?,
        ),
    };
    Ok(GoalSnapshot {
        objective: objective.to_owned(),
        status,
        token_budget,
        tokens_used: number("tokensUsed")?,
        time_used_seconds: number("timeUsedSeconds")?,
        created_at: timestamp("createdAt")?,
        updated_at: timestamp("updatedAt")?,
        progress: None,
    })
}
