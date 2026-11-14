// Only the Windows glue calls the native-popup path; tests cover it everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use std::time::Instant;

use super::*;

/// The only non-HTTP(S) document a popup may start on: the opener-linked
/// empty document that `window.open('')` creates before the opener assigns
/// `location`. Matched exactly, never by prefix.
pub(crate) const INITIAL_BLANK: &str = "about:blank";

/// A native child the chrome never acknowledged is closed after this, so a
/// hidden live page cannot hold a tab-cap slot indefinitely.
pub(crate) const NATIVE_ACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Admission for a popup the opener's WebView2 would host natively. It is the
/// same check as the intent path (`popup_target`: live opener, tab cap,
/// `admit_url`) plus the exact initial `about:blank`.
pub(super) fn native_popup_target<T>(
    tabs: &BrowserTabs<T>,
    opener: &str,
    requested: &str,
) -> Option<String> {
    if requested == INITIAL_BLANK {
        return opener_has_room(tabs, opener).then(|| INITIAL_BLANK.to_owned());
    }
    let url = admit_url(requested).ok()?;
    popup_target(tabs, opener, &url)
}

impl PopupRequests {
    /// The entry is hidden from replay until the child tab exists.
    pub(crate) fn reserve_native<T>(
        &mut self,
        tabs: &BrowserTabs<T>,
        opener: &str,
        requested: &str,
    ) -> Option<PopupIntent> {
        let url = native_popup_target(tabs, opener, requested);
        self.insert(opener, url, PopupState::NativeOpening)
    }

    pub(crate) fn native_opened(&mut self, id: u64, tab_id: &str, now: Instant) -> bool {
        let Some((intent, state)) = self.entries.get_mut(&id) else {
            return false;
        };
        if !matches!(state, PopupState::NativeOpening) {
            return false;
        }
        *state = PopupState::Opened(tab_id.to_owned());
        intent.native_tab_id = Some(tab_id.to_owned());
        if intent.url == INITIAL_BLANK && self.initial_blank.len() < MAX_BROWSER_TABS {
            self.initial_blank.insert(tab_id.to_owned());
        }
        let entries = &self.entries;
        self.native_deadlines
            .retain(|id, _| entries.contains_key(id));
        self.native_deadlines.insert(id, now + NATIVE_ACK_DEADLINE);
        true
    }

    /// Each is quarantined like `take_for_opener`; the caller closes the tab and
    /// then calls `finish_cleanup`.
    pub(crate) fn take_expired_native(&mut self, now: Instant) -> Vec<(u64, String)> {
        let mut expired = Vec::new();
        let deadlines = std::mem::take(&mut self.native_deadlines);
        for (id, deadline) in deadlines {
            let Some((_, state)) = self.entries.get_mut(&id) else {
                continue;
            };
            let PopupState::Opened(tab_id) = state else {
                continue;
            };
            if deadline <= now {
                let tab_id = tab_id.clone();
                *state = PopupState::Quarantined(Some(tab_id.clone()));
                expired.push((id, tab_id));
            } else {
                self.native_deadlines.insert(id, deadline);
            }
        }
        expired
    }

    pub(crate) fn native_failed(&mut self, id: u64) {
        if self
            .entries
            .get(&id)
            .is_some_and(|(_, state)| matches!(state, PopupState::NativeOpening))
        {
            self.entries.remove(&id);
        }
    }

    /// Every navigation except the granted initial `about:blank` spends the
    /// blank grant, HTTP(S) included. On WebView2 a `javascript:` popup is not
    /// abandoned: it arrives as an `about:blank` request and no navigation fires
    /// for its `javascript:` URL, so its script runs in the blank child with the
    /// opener's origin, as in Chrome. That gains nothing the opener lacks (it
    /// could `window.open('')` and script the child itself), and the child stays
    /// a policed Sage tab.
    pub(crate) fn admit_navigation(&mut self, tab_id: &str, uri: &str) -> NavigationVerdict {
        if uri == INITIAL_BLANK && self.initial_blank.contains(tab_id) {
            return NavigationVerdict::Admit;
        }
        let held_blank = self.initial_blank.remove(tab_id);
        match (admit_url(uri).is_ok(), held_blank) {
            (true, _) => NavigationVerdict::Admit,
            (false, true) => NavigationVerdict::Abandon,
            (false, false) => NavigationVerdict::Deny,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationVerdict {
    Admit,
    Deny,
    Abandon,
}

#[cfg(feature = "native-lane")]
pub(crate) static ABANDONED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fails closed to plain `admit_url` if the popup registry is unavailable. An
/// abandoned child is closed off the WebView2 callback.
pub(crate) fn navigation_admitted(app: &AppHandle, tab_id: &str, uri: &str) -> bool {
    let verdict = match app.state::<BrowserState>().popups.lock() {
        Ok(mut popups) => popups.admit_navigation(tab_id, uri),
        Err(_) if admit_url(uri).is_ok() => NavigationVerdict::Admit,
        Err(_) => NavigationVerdict::Deny,
    };
    if verdict == NavigationVerdict::Abandon {
        #[cfg(feature = "native-lane")]
        ABANDONED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let close_app = app.clone();
        let tab_id = tab_id.to_owned();
        tauri::async_runtime::spawn(async move {
            let state = close_app.state::<BrowserState>();
            if let Err(error) = super::cleanup::abandon_child(&close_app, &state, &tab_id).await {
                eprintln!("browser abandoned popup close failed: {error}");
            }
        });
    }
    verdict == NavigationVerdict::Admit
}

pub(crate) fn opener_has_room<T>(tabs: &BrowserTabs<T>, opener: &str) -> bool {
    tabs.entries.contains_key(opener) && tabs.slot_count() < MAX_BROWSER_TABS
}

pub(crate) fn emit_popup_intent(app: &AppHandle, id: u64) {
    let intent = app
        .state::<BrowserState>()
        .popups
        .lock()
        .ok()
        .and_then(|popups| popups.entries.get(&id).map(|(intent, _)| intent.clone()));
    if let Some(intent) = intent {
        let _ = app.emit_to("main", "browser:popup-request", intent);
    }
}

pub(crate) fn enqueue_intent(app: &AppHandle, opener: &str, requested: &Url) -> bool {
    let browser = app.state::<BrowserState>();
    let intent = browser.tabs.lock().ok().and_then(|tabs| {
        browser
            .popups
            .lock()
            .ok()
            .and_then(|mut popups| popups.enqueue(&tabs, opener, requested))
    });
    match intent {
        Some(intent) => {
            let _ = app.emit_to("main", "browser:popup-request", intent);
            true
        }
        None => false,
    }
}

#[cfg(test)]
#[path = "opener_tests.rs"]
mod tests;
