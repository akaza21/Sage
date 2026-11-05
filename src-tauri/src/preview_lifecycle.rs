//! Owner-landed Project-transition hook for bounded App Preview children.
//!
//! Every committed epoch bump must call
//! [`on_committed_project_transition_epoch`]. Preview start/stop lives in
//! IDE-3; this module only guarantees the cleanup call site.

/// Invoked after a committed Project-transition epoch bump.
///
/// Production increment sites today: create/add/switch
/// (`project_commands::execute_committed_transition`), Chat transitions
/// (`chat_commands::execute_chat_transition`, including archive/restore),
/// file rename, and trash.
pub(crate) fn on_committed_project_transition_epoch(new_epoch: u64) {
    stop_preview_children_before_epoch(new_epoch);
}

/// Stops preview children bound to any epoch older than `new_epoch`.
///
/// No-op on this owner stub. IDE-3 replaces the body (or routes it into
/// `app_preview`) without adding a second call site from command files.
pub(crate) fn stop_preview_children_before_epoch(new_epoch: u64) {
    crate::app_preview_commands::stop_preview_children_before_epoch(new_epoch);
}

/// Process-exit cleanup. Owner calls this from `lib.rs` on ExitRequested.
pub(crate) fn cleanup_previews_on_exit() {
    stop_preview_children_before_epoch(u64::MAX);
}

#[cfg(test)]
mod tests;
