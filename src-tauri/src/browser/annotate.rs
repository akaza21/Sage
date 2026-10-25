//! Annotate mode (M1a): the user's hover highlight over one browser tab. Rust
//! owns which tab annotates and tells the chrome over `browser:annotate`; the
//! highlight is Chromium's own inspect overlay, so nothing enters the page.
//! These are user chrome commands from the main webview, not Agent tools.
//!
//! Every native step runs off the main thread, under the mode lock, so a
//! start, a stop and the exits on navigation, hide, close and Escape never
//! interleave. Each change is announced under that lock too, so the chrome
//! receives the changes in the order they happened.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::{BrowserSnapshot, BrowserState, BrowserTabs, page_context::valid_page_context_tab_id};

/// Why the chrome shows Annotate disabled on this platform, if it does.
pub(crate) const UNAVAILABLE_REASON: Option<&str> = if cfg!(windows) {
    None
} else {
    Some("Annotate needs the WebView2 inspect overlay and is not available on this platform yet")
};

/// Why the chrome shows Annotate disabled for a tab in this state, if it does.
pub(super) fn unavailable_reason(snapshot: &BrowserSnapshot) -> Option<&'static str> {
    UNAVAILABLE_REASON.or((snapshot.loading
        || snapshot.error.is_some()
        || snapshot.committed_url.is_none())
    .then_some("Open a page to annotate"))
}

/// The control actions that turn Annotate on and off.
pub(crate) fn action(name: &str) -> Option<bool> {
    match name {
        "annotateOn" => Some(true),
        "annotateOff" => Some(false),
        _ => None,
    }
}

/// Drives the native inspect overlay of one tab.
pub(crate) trait InspectOverlay {
    fn start(&self, label: &str) -> Result<(), String>;
    fn stop(&self, label: &str) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnotateChange {
    tab_id: String,
    active: bool,
}

/// Annotate's place in `BrowserState`: the mode, and a lock-free copy of its
/// tab number (0 for none) for the main thread, which must never wait on the
/// mode lock while a native step needs that thread.
#[derive(Default)]
pub(crate) struct Annotate {
    mode: Mutex<AnnotateMode>,
    tab: AtomicU64,
}

impl Annotate {
    /// Whether `label` is annotating, without taking the mode lock.
    pub(crate) fn is_annotating(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.tab.load(Ordering::Acquire) == number)
    }

    /// Applies one change to the mode and announces what it changed before
    /// the lock is released.
    fn run(
        &self,
        announce: impl Fn(AnnotateChange),
        apply: impl FnOnce(&mut AnnotateMode) -> (Vec<AnnotateChange>, Result<(), String>),
    ) -> Result<(), String> {
        let mut mode = self.mode.lock().map_err(|_| "browser unavailable")?;
        let (changes, result) = apply(&mut mode);
        let number = mode.tab.as_deref().and_then(tab_number).unwrap_or(0);
        self.tab.store(number, Ordering::Release);
        changes.into_iter().for_each(announce);
        result
    }
}

fn tab_number(label: &str) -> Option<u64> {
    label
        .strip_prefix("browser-")?
        .parse()
        .ok()
        .filter(|number| *number > 0)
}

/// Which tab, if any, is in Annotate mode. At most one is.
#[derive(Default)]
struct AnnotateMode {
    tab: Option<String>,
}

impl AnnotateMode {
    /// Turns Annotate on for `label`, first leaving it on any other tab. The
    /// changes to announce come back even when the start fails. Starting the
    /// tab that is already on starts its overlay again and announces it again,
    /// so a remounted chrome or a page whose overlay an Agent highlight took
    /// over recovers with one click.
    fn start(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        let mut changes = Vec::new();
        if let Some(previous) = self.tab.take().filter(|previous| previous != label) {
            let _ = overlay.stop(&previous);
            changes.push(change(previous, false));
        }
        let started = overlay.start(label);
        if started.is_ok() {
            self.tab = Some(label.to_owned());
            changes.push(change(label.to_owned(), true));
        }
        (changes, started)
    }

    /// Turns Annotate off for `label` if it is on; nothing changes when it
    /// was off. The mode ends even if the overlay fails to stop, so the chrome
    /// never shows Annotate on for a page that may have lost it. `clear` is
    /// false for a closing tab, whose overlay goes with its webview.
    fn leave(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        clear: bool,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) {
            return (Vec::new(), Ok(()));
        }
        self.tab = None;
        let stopped = if clear { overlay.stop(label) } else { Ok(()) };
        (vec![change(label.to_owned(), false)], stopped)
    }
}

fn change(tab_id: String, active: bool) -> AnnotateChange {
    AnnotateChange { tab_id, active }
}

/// The tab Annotate may start on: a known tab showing a committed page.
pub(super) fn target<T: Clone>(tabs: &BrowserTabs<T>, tab_id: &str) -> Result<T, String> {
    if !valid_page_context_tab_id(tab_id) {
        return Err("invalid browser tab id".into());
    }
    tabs.openable_url(tab_id)?;
    tabs.handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable".into())
}

/// `browser_control`'s `annotateOn` and `annotateOff`.
pub(crate) async fn set(app: AppHandle, tab_id: String, on: bool) -> Result<(), String> {
    if !valid_page_context_tab_id(&tab_id) {
        return Err("invalid browser tab id".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state);
        state.annotate.run(
            |change| emit(&app, change),
            |mode| {
                if !on {
                    return mode.leave(&overlay, &tab_id, true);
                }
                let admitted = state
                    .tabs
                    .lock()
                    .map_err(|_| "browser unavailable".to_owned())
                    .and_then(|tabs| target(&tabs, &tab_id));
                match admitted {
                    Ok(_) => mode.start(&overlay, &tab_id),
                    Err(error) => (Vec::new(), Err(error)),
                }
            },
        )
    })
    .await
    .map_err(|_| "browser annotate unavailable".to_owned())?
}

/// Leaves Annotate on `label` after a navigation starts, the tab is hidden or
/// closes, or Escape reaches the page. Runs on its own thread: callers
/// include WebView2 event handlers on the main thread, which the overlay's
/// CDP calls need free.
pub(crate) fn leave(app: &AppHandle, label: &str, clear: bool) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state);
        let _ = state.annotate.run(
            |change| emit(&app, change),
            |mode| mode.leave(&overlay, &label, clear),
        );
    });
}

fn emit(app: &AppHandle, change: AnnotateChange) {
    let _ = app.emit_to("main", "browser:annotate", change);
}

struct NativeOverlay<'a>(&'a BrowserState);

impl InspectOverlay for NativeOverlay<'_> {
    #[cfg(windows)]
    fn start(&self, label: &str) -> Result<(), String> {
        let webview = self
            .0
            .webview_for_tab(label)
            .ok_or("browser tab unavailable")?;
        super::devtools::set_annotate(&webview, true)
            .map_err(|_| "browser annotate unavailable".into())
    }

    #[cfg(windows)]
    fn stop(&self, label: &str) -> Result<(), String> {
        let webview = self
            .0
            .webview_for_tab(label)
            .ok_or("browser tab unavailable")?;
        super::devtools::set_annotate(&webview, false)
            .map_err(|_| "browser annotate unavailable".into())
    }

    #[cfg(not(windows))]
    fn start(&self, _: &str) -> Result<(), String> {
        let _ = self.0;
        Err(UNAVAILABLE_REASON.unwrap_or_default().into())
    }

    #[cfg(not(windows))]
    fn stop(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
