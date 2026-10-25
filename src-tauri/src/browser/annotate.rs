//! Annotate mode (M1a): the user's hover highlight over one browser tab. Rust
//! owns which tab annotates and tells the chrome over `browser:annotate`; the
//! highlight is Chromium's own inspect overlay, so nothing enters the page.
//! These are user chrome commands from the main webview, not Agent tools.
//!
//! Every native step runs off the main thread, under the mode lock, so a
//! start, a stop and the exits on navigation, hide and close never interleave.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::{BrowserState, BrowserTabs, page_context::valid_page_context_tab_id};

/// Why the chrome shows Annotate disabled on this platform, if it does.
pub(crate) const UNAVAILABLE_REASON: Option<&str> = if cfg!(windows) {
    None
} else {
    Some("Annotate needs the WebView2 inspect overlay and is not available on this platform yet")
};

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

/// Which tab, if any, is in Annotate mode. At most one is.
#[derive(Default)]
pub(crate) struct AnnotateMode {
    tab: Option<String>,
}

impl AnnotateMode {
    /// Turns Annotate on for `label`, first leaving it on any other tab. The
    /// changes to announce come back even when the start fails.
    fn start(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        let mut changes = Vec::new();
        if self.tab.as_deref() == Some(label) {
            return (changes, Ok(()));
        }
        if let Some(previous) = self.tab.take() {
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

    /// Turns Annotate off for `label` if it is on; `None` when it was off.
    /// The mode ends even if the overlay fails to stop, so the chrome never
    /// shows Annotate on for a page that may have lost it. `clear` is false
    /// for a closing tab, whose overlay goes with its webview.
    fn leave(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        clear: bool,
    ) -> Option<Result<(), String>> {
        if self.tab.as_deref() != Some(label) {
            return None;
        }
        self.tab = None;
        Some(if clear { overlay.stop(label) } else { Ok(()) })
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
        let mut mode = state.annotate.lock().map_err(|_| "browser unavailable")?;
        if on {
            target(&*state.tabs.lock().map_err(|_| "browser unavailable")?, &tab_id)?;
            let (changes, started) = mode.start(&overlay, &tab_id);
            drop(mode);
            changes.into_iter().for_each(|change| emit(&app, change));
            return started;
        }
        let left = mode.leave(&overlay, &tab_id, true);
        drop(mode);
        let Some(stopped) = left else {
            return Ok(());
        };
        emit(&app, change(tab_id, false));
        stopped
    })
    .await
    .map_err(|_| "browser annotate unavailable".to_owned())?
}

/// Leaves Annotate on `label` after a navigation starts, the tab is hidden or
/// it closes. Runs on its own thread: callers include WebView2 event handlers
/// on the main thread, which the overlay's CDP calls need free.
pub(crate) fn leave(app: &AppHandle, label: &str, clear: bool) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let Ok(mut mode) = state.annotate.lock() else {
            return;
        };
        let left = mode.leave(&NativeOverlay(&state), &label, clear);
        drop(mode);
        if left.is_some() {
            emit(&app, change(label, false));
        }
    });
}

fn emit(app: &AppHandle, change: AnnotateChange) {
    let _ = app.emit_to("main", "browser:annotate", change);
}

struct NativeOverlay<'a>(&'a BrowserState);

impl InspectOverlay for NativeOverlay<'_> {
    #[cfg(windows)]
    fn start(&self, label: &str) -> Result<(), String> {
        let webview = self.0.webview_for_tab(label).ok_or("browser tab unavailable")?;
        super::devtools::set_annotate(&webview, true)
            .map_err(|_| "browser annotate unavailable".into())
    }

    #[cfg(windows)]
    fn stop(&self, label: &str) -> Result<(), String> {
        let webview = self.0.webview_for_tab(label).ok_or("browser tab unavailable")?;
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
