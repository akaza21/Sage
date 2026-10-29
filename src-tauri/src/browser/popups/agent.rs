//! The chrome's acknowledgement of a popup child, after which a child of a
//! tab the Agent drives joins the opener's Agent attachment.

use tauri::{AppHandle, Manager};

use super::{BrowserState, PopupRequests, PopupState};

impl PopupRequests {
    /// Settles the entry for `tab_id` if Rust recorded it as `opener`'s child.
    pub(super) fn ack(&mut self, id: u64, opener: &str, tab_id: &str) -> bool {
        if !self.entries.get(&id).is_some_and(|(intent, state)| {
            intent.opener_tab_id == opener
                && matches!(state, PopupState::Opened(opened) if opened == tab_id)
        }) {
            return false;
        }
        self.entries.remove(&id);
        true
    }
}

/// Called once Rust has matched `child` to `opener` (`browser_popup_ack`).
/// The opener counts as Agent-driven when an Agent command reached it since
/// its last user navigation, the signal Agent downloads use; the child of any
/// other tab stays the user's.
pub(super) fn join_opener_attachment(
    app: &AppHandle,
    state: &BrowserState,
    opener: &str,
    child: &str,
) {
    let agent_driven = state
        .downloads
        .lock()
        .is_ok_and(|downloads| downloads.is_agent(opener));
    if !agent_driven {
        return;
    }
    let settled = state.settled_tab(child).ok();
    app.state::<crate::state::AppState>()
        .acp
        .adopt_browser_popup(
            opener,
            child,
            settled
                .as_ref()
                .map(|(url, revision)| (url.as_str(), *revision)),
        );
}
