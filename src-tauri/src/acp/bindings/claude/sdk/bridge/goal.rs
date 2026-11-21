use serde_json::Value;

use super::SdkBridge;
use crate::acp::goal::{GoalProgress, GoalSnapshot, GoalState, GoalStatus, valid_objective};
use sage_acp_protocol::intent::GoalCapabilities;

/// This command is offered by the CLI only for accounts with native goals enabled.
const GOAL_COMMAND: &str = "goal";
const MAX_REASON_CHARS: usize = 512;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

impl SdkBridge {
    pub(in crate::acp::bindings::claude::sdk) fn goal_capabilities(&self) -> GoalCapabilities {
        let supported = self.goals_enabled;
        GoalCapabilities {
            set: supported,
            clear: supported,
            ..Default::default()
        }
    }

    pub(super) fn refresh_goal_capability(&mut self) {
        self.goals_enabled = self.commands.as_array().is_some_and(|commands| {
            commands
                .iter()
                .any(|command| command["name"] == GOAL_COMMAND)
        });
        if self.goals_enabled && matches!(self.goal, GoalState::Unsupported) {
            self.goal = GoalState::NoGoal;
        }
    }

    pub(in crate::acp::bindings::claude::sdk) fn goal_command_name(&self) -> &'static str {
        GOAL_COMMAND
    }

    pub(in crate::acp::bindings::claude::sdk) fn goal_state(&self) -> &GoalState {
        &self.goal
    }

    pub(super) fn active_goal(&mut self, sdk: &Value) {
        if !self.goals_enabled {
            return;
        }
        if sdk.get("session_id").and_then(Value::as_str) != Some(self.session_id.as_str()) {
            return;
        }
        self.goal = match sdk.get("value") {
            Some(Value::Null) => GoalState::NoGoal,
            Some(value) => snapshot(value).map_or(GoalState::Unavailable, |snapshot| {
                GoalState::Goal { snapshot }
            }),
            None => GoalState::Unavailable,
        };
    }
}

fn snapshot(value: &Value) -> Option<GoalSnapshot> {
    let objective = value.get("condition")?.as_str()?;
    if !valid_objective(objective) {
        return None;
    }
    let counter = |key: &str| value.get(key)?.as_u64().filter(|n| *n <= MAX_SAFE_INTEGER);
    let last_reason = match value.get("last_reason") {
        None => None,
        Some(reason) => {
            let reason = reason.as_str()?;
            if reason.chars().count() > MAX_REASON_CHARS || reason.contains('\0') {
                return None;
            }
            Some(reason.to_owned())
        }
    };
    Some(GoalSnapshot {
        objective: objective.to_owned(),
        status: GoalStatus::Active,
        token_budget: None,
        tokens_used: 0,
        time_used_seconds: 0,
        created_at: None,
        updated_at: None,
        progress: Some(GoalProgress {
            iterations: counter("iterations")?,
            set_at: counter("set_at")?,
            tokens_at_start: counter("tokens_at_start")?,
            last_reason,
        }),
    })
}
