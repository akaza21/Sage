use crate::acp::lifecycle::TurnState;

use super::CodexAdapter;

impl CodexAdapter {
    /// True only when `thread`, `turn` and `state` repeat the terminal this
    /// adapter last applied, with no turn active or provisional since, so
    /// ingesting it again is a duplicate terminal that changes nothing.
    pub(crate) fn repeats_previous_terminal(
        &self,
        thread: &str,
        turn: &str,
        state: TurnState,
    ) -> bool {
        self.active_turn_id.is_none()
            && self.provisional_turn.is_none()
            && self.expect_thread(thread).is_ok()
            && self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id.as_str() == turn && previous.state == state)
    }
}
