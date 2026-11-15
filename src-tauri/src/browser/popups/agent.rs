use tauri::{AppHandle, Manager};

use super::{BrowserState, PopupRequests, PopupState};

impl PopupRequests {
    pub(super) fn ack(&mut self, id: u64, opener: &str, tab_id: &str) -> bool {
        if !self.entries.get(&id).is_some_and(|(intent, state)| {
            intent.opener_tab_id == opener
                && matches!(state, PopupState::Opened(opened) if opened == tab_id)
        }) {
            return false;
        }
        self.entries.remove(&id);
        self.native_deadlines.remove(&id);
        true
    }
}

/// The opener counts as Agent-driven when an Agent command reached it since
/// its last user navigation, the signal Agent downloads use.
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
