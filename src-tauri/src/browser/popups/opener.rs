//! Platform-neutral rules for popups that keep `window.opener` (S1, Option C).
//!
//! The Windows `NewWindowRequested` glue in `native.rs` only asks these
//! questions; every policy decision lives here so it is unit-tested on Linux.
// Only the Windows glue calls the native-popup path; tests cover it everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use super::*;

/// The only non-HTTP(S) document a popup may start on: the opener-linked
/// empty document that `window.open('')` creates before the opener assigns
/// `location`. Matched exactly, never by prefix.
pub(crate) const INITIAL_BLANK: &str = "about:blank";

/// Admission for a popup the opener's WebView2 would host natively. It is the
/// same check as the intent path (`popup_target`: live opener, tab cap,
/// `admit_url`) plus the exact initial `about:blank`.
pub(super) fn native_popup_target<T>(
    tabs: &BrowserTabs<T>,
    opener: &str,
    requested: &str,
) -> Option<String> {
    if requested == INITIAL_BLANK {
        let open = tabs.entries.contains_key(opener) && tabs.slot_count() < MAX_BROWSER_TABS;
        return open.then(|| INITIAL_BLANK.to_owned());
    }
    let url = admit_url(requested).ok()?;
    popup_target(tabs, opener, &url)
}

impl PopupRequests {
    /// Reserves a bounded, opener-linked entry for a native popup. The entry
    /// is hidden from replay until the child tab exists.
    pub(crate) fn reserve_native<T>(
        &mut self,
        tabs: &BrowserTabs<T>,
        opener: &str,
        requested: &str,
    ) -> Option<PopupIntent> {
        if self.entries.len() >= MAX_PENDING_POPUPS {
            return None;
        }
        let url = native_popup_target(tabs, opener, requested)?;
        let id = self.next_id.checked_add(1)?;
        self.next_id = id;
        let intent = PopupIntent {
            id,
            opener_tab_id: opener.to_owned(),
            url,
        };
        self.entries
            .insert(id, (intent.clone(), PopupState::NativeOpening));
        Some(intent)
    }

    /// Records the created child as an `Opened` popup, so the tab cap,
    /// `take_for_opener` teardown and the chrome's replay/ack all cover it.
    /// Returns false when the opener closed (or the entry was dropped) first.
    pub(crate) fn native_opened(&mut self, id: u64, tab_id: &str) -> bool {
        let Some((intent, state)) = self.entries.get_mut(&id) else {
            return false;
        };
        if !matches!(state, PopupState::NativeOpening) {
            return false;
        }
        *state = PopupState::Opened(tab_id.to_owned());
        if intent.url == INITIAL_BLANK && self.initial_blank.len() < MAX_BROWSER_TABS {
            self.initial_blank.insert(tab_id.to_owned());
        }
        true
    }

    /// Drops a native popup whose creation failed before a child existed.
    pub(crate) fn native_failed(&mut self, id: u64) {
        if self
            .entries
            .get(&id)
            .is_some_and(|(_, state)| matches!(state, PopupState::NativeOpening))
        {
            self.entries.remove(&id);
        }
    }

    /// Navigation admission for a browser tab. HTTP(S) per `admit_url`; the
    /// exact `about:blank` only for a native opener-linked child that has not
    /// yet started any other navigation. Any other attempt spends the grant.
    pub(crate) fn admit_navigation(&mut self, tab_id: &str, uri: &str) -> bool {
        if uri == INITIAL_BLANK && self.initial_blank.contains(tab_id) {
            return true;
        }
        self.initial_blank.remove(tab_id);
        admit_url(uri).is_ok()
    }
}

/// `admit_navigation` against the live browser state; fails closed to plain
/// `admit_url` if the popup registry is unavailable.
pub(crate) fn navigation_admitted(app: &AppHandle, tab_id: &str, uri: &str) -> bool {
    match app.state::<BrowserState>().popups.lock() {
        Ok(mut popups) => popups.admit_navigation(tab_id, uri),
        Err(_) => admit_url(uri).is_ok(),
    }
}

/// The deny-and-enqueue intent path: the chrome opens an admitted target as
/// a fresh tab without `window.opener`. Returns false when nothing was queued.
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
