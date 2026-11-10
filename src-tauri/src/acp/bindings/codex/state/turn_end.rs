//! What the active Turn's end needs: whether Sage asked Codex to stop it, and
//! the latest plan, so the plan item closes with the Turn.

use crate::acp::{contract::BoundedVec, intent::NormalizedPlanEntry, lifecycle::TurnState};

use super::CodexAdapter;

pub(crate) type PlanEntries = BoundedVec<NormalizedPlanEntry, 128>;

impl CodexAdapter {
    /// Codex ends every stopped Turn `interrupted`; it is a user stop when
    /// Sage wrote `turn/interrupt` for that Turn.
    pub(crate) fn stopped_by_user(&self, turn: &str) -> bool {
        match &self.previous_turn {
            Some(previous) if previous.id.as_str() == turn => {
                previous.state == TurnState::Cancelled
            }
            _ => self.active_turn_user_stop,
        }
    }

    pub(crate) fn remember_turn_plan(&mut self, entries: PlanEntries) {
        self.active_turn_plan = Some(entries);
    }

    pub(crate) fn take_turn_plan(&mut self, turn: &str) -> Option<PlanEntries> {
        self.active_turn_id
            .as_ref()
            .filter(|active| active.as_str() == turn)?;
        self.active_turn_plan.take()
    }
}
