//! Every committed epoch bump must call
//! [`on_committed_project_transition_epoch`].

pub(crate) fn on_committed_project_transition_epoch(new_epoch: u64) {
    stop_preview_children_before_epoch(new_epoch);
}

pub(crate) fn stop_preview_children_before_epoch(new_epoch: u64) {
    crate::app_preview_commands::stop_preview_children_before_epoch(new_epoch);
}

pub(crate) fn cleanup_previews_on_exit() {
    stop_preview_children_before_epoch(u64::MAX);
}

#[cfg(test)]
mod tests;
