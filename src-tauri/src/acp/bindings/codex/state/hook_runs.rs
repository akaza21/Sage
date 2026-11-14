//! A Turn that ends while a hook runs (an interrupt, say) never receives its
//! `hook/completed`, so these rows are settled as cancelled when the Turn
//! ends instead of staying Running.

use sage_acp_protocol::intent::session_event::{
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
