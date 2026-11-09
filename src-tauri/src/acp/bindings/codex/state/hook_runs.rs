//! Hook rows started inside the active Turn and not yet completed. A Turn
//! that ends while a hook runs (an interrupt, say) never receives its
//! `hook/completed`, so these rows are settled as cancelled when the Turn
//! ends instead of staying Running.
//!
//! Codex names a run by its handler, not by its invocation, so concurrent
//! runs of one handler share a row and one entry here.
//!
//! Cap: [`RUNNING_HOOKS_MAX`] rows, the session surface's activity cap; the
//! oldest is dropped beyond it. Each row's text is capped by its types.

use crate::acp::intent::session_event::{
    SESSION_ACTIVITIES_MAX, SessionActivity, SessionActivityState,
};

use super::CodexAdapter;

pub(crate) const RUNNING_HOOKS_MAX: usize = SESSION_ACTIVITIES_MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct RunningHooks {
    rows: Vec<SessionActivity>,
}

impl CodexAdapter {
    pub(in crate::acp::bindings::codex) fn track_running_hook(&mut self, row: &SessionActivity) {
        let rows = &mut self.running_hooks.rows;
        rows.retain(|running| running.id != row.id);
        if rows.len() == RUNNING_HOOKS_MAX {
            rows.remove(0);
        }
        rows.push(row.clone());
    }

    pub(in crate::acp::bindings::codex) fn forget_running_hook(&mut self, row: &SessionActivity) {
        self.running_hooks
            .rows
            .retain(|running| running.id != row.id);
    }

    pub(crate) fn clear_running_hooks(&mut self) {
        self.running_hooks.rows.clear();
    }

    /// Takes the rows still running, each restated as cancelled.
    pub(crate) fn cancel_running_hooks(&mut self) -> Vec<SessionActivity> {
        std::mem::take(&mut self.running_hooks.rows)
            .into_iter()
            .map(|row| SessionActivity {
                state: SessionActivityState::Cancelled,
                progress: None,
                ..row
            })
            .collect()
    }
}
