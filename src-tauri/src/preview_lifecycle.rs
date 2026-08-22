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
    #[cfg(test)]
    tests::note(new_epoch);
    stop_preview_children_before_epoch(new_epoch);
}

/// Stops preview children bound to any epoch older than `new_epoch`.
///
/// No-op on this owner stub. IDE-3 replaces the body (or routes it into
/// `app_preview`) without adding a second call site from command files.
pub(crate) fn stop_preview_children_before_epoch(new_epoch: u64) {
    let _ = new_epoch;
}

#[cfg(test)]
mod tests;
