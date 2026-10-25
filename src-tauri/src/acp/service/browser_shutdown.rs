#![cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]

use std::time::Instant;

#[cfg(windows)]
use tauri::Manager;

pub(super) fn close_agent_tabs_once_for_shutdown<I, Claim, Close, Guard>(
    labels: I,
    deadline: Instant,
    mut claim: Claim,
    mut close: Close,
) where
    I: IntoIterator,
    I::Item: AsRef<str>,
    Claim: FnMut(&str) -> Option<Guard>,
    Close: FnMut(&str),
{
    for label in labels {
        if Instant::now() >= deadline {
            break;
        }
        let label = label.as_ref();
        if let Some(_claim) = claim(label) {
            close(label);
        }
    }
}

pub(super) fn complete_shutdown_close(
    close: impl FnOnce() -> Result<(), String>,
    quarantine: impl FnOnce(),
    revoke: impl FnOnce(),
) {
    if close().is_ok() {
        revoke();
    } else {
        quarantine();
    }
}

impl super::AcpService {
    pub(super) fn close_agent_tabs_for_shutdown(&self, deadline: Instant) {
        #[cfg(windows)]
        if let Some(app) = self.browser_app.lock().ok().and_then(|slot| slot.clone()) {
            close_agent_tabs_once_for_shutdown(
                self.browser_attachments.agent_all_labels(),
                deadline,
                |label| {
                    let claim = self.browser_attachments.agent_claim_sweep(label);
                    if claim.is_some() {
                        self.browser_attachments.agent_retire_label(label);
                    }
                    claim
                },
                |label| {
                    let state = app.state::<crate::browser::BrowserState>();
                    complete_shutdown_close(
                        || state.close_for_shutdown(label),
                        || state.quarantine_failed_close(label),
                        || self.revoke_browser_tab(label),
                    );
                },
            );
            return;
        }
        let _ = deadline;
        for (project, chat) in self.browser_attachments.agent_chats() {
            self.browser_attachments.revoke_agent_chat(&project, &chat);
        }
    }
}
