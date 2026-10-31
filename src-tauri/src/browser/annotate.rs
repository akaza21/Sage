//! Annotate mode: the user's hover highlight over one browser tab (M1a) and
//! picking an element to comment on (M1b). Rust owns which tab annotates and
//! which element waits for a comment, and tells the chrome over
//! `browser:annotate`; the highlight is Chromium's own inspect overlay and the
//! element is read in the isolated Agent world, so nothing enters the page's
//! main world. These are user chrome commands from the main webview, not
//! Agent tools.
//!
//! Every native step runs off the main thread, under the mode lock, so a
//! start, a pick, a comment and the exits on navigation, hide, close and
//! Escape never interleave. Each change is announced under that lock too, so
//! the chrome receives the changes in the order they happened.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::{BrowserSnapshot, BrowserState, BrowserTabs, page_context::valid_page_context_tab_id};

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod element;
#[cfg(windows)]
mod native;
pub(crate) mod selection;

use selection::{Selection, SelectionView};

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

/// The `browser_control` actions Annotate answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    On,
    Off,
    /// Drops the element waiting for a comment.
    Cancel,
    /// Numbers and comments the waiting element and hands the item out.
    Confirm,
    /// The frozen page the user comments on.
    Frame,
}

pub(crate) fn action(name: &str) -> Option<Action> {
    match name {
        "annotateOn" => Some(Action::On),
        "annotateOff" => Some(Action::Off),
        "annotateCancel" => Some(Action::Cancel),
        "annotateConfirm" => Some(Action::Confirm),
        "annotateFrame" => Some(Action::Frame),
        _ => None,
    }
}

/// How a tab leaves Annotate, which decides whether a page frozen for a
/// comment is shown again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The button, Escape or a navigation: stop and show the page.
    Reveal,
    /// The tab was hidden: stop and leave the page hidden.
    Hidden,
    /// The tab is closing: its overlay goes with its webview.
    Closed,
}

/// Drives the native inspect overlay of one tab.
pub(crate) trait InspectOverlay {
    fn start(&self, label: &str) -> Result<(), String>;
    /// Stops listening for clicks, and turns the overlay off unless the tab
    /// is `closing`, when its webview may already be gone.
    fn stop(&self, label: &str, closing: bool) -> Result<(), String>;
    /// Reads the clicked element, pauses hover highlighting, captures the
    /// page and hides it so the chrome can draw over the frozen frame. It
    /// calls `freeze` before the hide, so a resize never shows the page.
    fn pick(
        &self,
        label: &str,
        backend_node_id: u64,
        freeze: &dyn Fn(),
    ) -> Result<Selection, String>;
    /// Shows the frozen page again when `reveal`, and resumes hover
    /// highlighting when `resume`.
    fn release(&self, label: &str, reveal: bool, resume: bool) -> Result<(), String>;
    /// Moves native keyboard focus to `label`'s page when `page`, else to
    /// Sage's own webview, whose comment box takes the typing.
    fn focus(&self, label: &str, page: bool) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnotateChange {
    tab_id: String,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<SelectionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Annotate's place in `BrowserState`: the mode, and lock-free copies of its
/// tab number and of the tab whose page is frozen (0 for none) for the main
/// thread, which must never wait on the mode lock while a native step needs
/// that thread.
#[derive(Default)]
pub(crate) struct Annotate {
    mode: Mutex<AnnotateMode>,
    tab: AtomicU64,
    frozen: AtomicU64,
}

impl Annotate {
    /// Whether `label` is annotating, without taking the mode lock.
    pub(crate) fn is_annotating(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.tab.load(Ordering::Acquire) == number)
    }

    /// Whether `label`'s page is hidden behind a frozen frame, without taking
    /// the mode lock; placing the tab must not show it.
    pub(crate) fn is_frozen(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.frozen.load(Ordering::Acquire) == number)
    }

    /// Runs `disable` only while `label` does not annotate and no Annotate
    /// change is under way, holding the mode lock so a start cannot run its
    /// enables between the check and `disable`. It never waits: a busy lock
    /// means Annotate is changing, and then `disable` is skipped. Returns
    /// whether `disable` ran. Never call it on the main thread.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn disable_unless_annotating(&self, label: &str, disable: impl FnOnce()) -> bool {
        let Ok(mode) = self.mode.try_lock() else {
            return false;
        };
        if mode.tab.as_deref() == Some(label) {
            return false;
        }
        disable();
        true
    }

    /// Marks `label`'s page frozen ahead of the hide that freezes it; the
    /// next change settles the copy again.
    fn freeze(&self, label: &str) {
        self.frozen
            .store(tab_number(label).unwrap_or(0), Ordering::Release);
    }

    /// Picks the element Chromium reported for a click on `label`.
    fn pick(
        &self,
        overlay: &impl InspectOverlay,
        label: &str,
        backend_node_id: u64,
        announce: impl Fn(AnnotateChange),
    ) -> Result<(), String> {
        self.run(announce, |mode| {
            mode.pick(overlay, label, backend_node_id, &|| self.freeze(label))
        })
    }

    /// Applies one change to the mode and announces what it changed before
    /// the lock is released.
    fn run<R>(
        &self,
        announce: impl Fn(AnnotateChange),
        apply: impl FnOnce(&mut AnnotateMode) -> (Vec<AnnotateChange>, Result<R, String>),
    ) -> Result<R, String> {
        let mut mode = self.mode.lock().map_err(|_| "browser unavailable")?;
        let (changes, result) = apply(&mut mode);
        let number = |tab: Option<&str>| tab.and_then(tab_number).unwrap_or(0);
        self.tab
            .store(number(mode.tab.as_deref()), Ordering::Release);
        let frozen = mode
            .pending
            .is_some()
            .then_some(mode.tab.as_deref())
            .flatten();
        self.frozen.store(number(frozen), Ordering::Release);
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

/// Which tab, if any, is in Annotate mode, and the element it picked, if it
/// waits for a comment. At most one tab annotates.
#[derive(Default)]
struct AnnotateMode {
    tab: Option<String>,
    pending: Option<Selection>,
}

impl AnnotateMode {
    /// Turns Annotate on for `label`, first leaving it on any other tab. The
    /// changes to announce come back even when the start fails. Starting the
    /// tab that is already on starts its overlay again and announces it again,
    /// so a remounted chrome or a page whose overlay an Agent highlight took
    /// over recovers with one click; a waiting element is dropped.
    fn start(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        let mut changes = Vec::new();
        if let Some(previous) = self.tab.take() {
            if self.pending.take().is_some() {
                let _ = overlay.release(&previous, true, false);
            }
            if previous != label {
                let _ = overlay.stop(&previous, false);
                changes.push(change(previous, false));
            }
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
    /// never shows Annotate on for a page that may have lost it.
    fn leave(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        exit: Exit,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) {
            return (Vec::new(), Ok(()));
        }
        self.tab = None;
        let released = match self.pending.take() {
            Some(_) if exit != Exit::Closed => overlay.release(label, exit == Exit::Reveal, false),
            _ => Ok(()),
        };
        let stopped = overlay.stop(label, exit == Exit::Closed);
        (vec![change(label.to_owned(), false)], stopped.and(released))
    }
}

fn change(tab_id: String, active: bool) -> AnnotateChange {
    AnnotateChange {
        tab_id,
        active,
        selection: None,
        error: None,
    }
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

/// `browser_control`'s Annotate actions. Confirm answers with the item and
/// Frame with the frozen page; the others answer nothing.
pub(crate) async fn set(
    app: AppHandle,
    tab_id: String,
    action: Action,
    comment: Option<String>,
    ordinal: Option<u8>,
) -> Result<Option<serde_json::Value>, String> {
    if !valid_page_context_tab_id(&tab_id) {
        return Err("invalid browser tab id".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let announce = |change| emit(&app, change);
        match action {
            Action::On => state.annotate.run(announce, |mode| {
                let admitted = state
                    .tabs
                    .lock()
                    .map_err(|_| "browser unavailable".to_owned())
                    .and_then(|tabs| target(&tabs, &tab_id));
                match admitted {
                    Ok(_) => mode.start(&overlay, &tab_id),
                    Err(error) => (Vec::new(), Err(error)),
                }
            }),
            Action::Off => state
                .annotate
                .run(announce, |mode| mode.leave(&overlay, &tab_id, Exit::Reveal)),
            Action::Cancel => state
                .annotate
                .run(announce, |mode| mode.cancel(&overlay, &tab_id)),
            Action::Confirm => {
                let comment = comment.unwrap_or_default();
                let ordinal = ordinal.ok_or("browser element ordinal rejected")?;
                return value(
                    state
                        .annotate
                        .run(announce, |mode| mode.confirm(&tab_id, ordinal, &comment)),
                )
                .map(Some);
            }
            Action::Frame => {
                return value(
                    state
                        .annotate
                        .run(announce, |mode| (Vec::new(), mode.frame(&tab_id))),
                )
                .map(Some);
            }
        }
        .map(|()| None)
    })
    .await
    .map_err(|_| "browser annotate unavailable".to_owned())?
}

/// Picks the element Chromium reported for a click in inspect mode. Runs on
/// its own thread: the report arrives on the main thread, which the pick's
/// CDP calls need free.
pub(crate) fn pick(app: &AppHandle, label: &str, backend_node_id: u64) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state
            .annotate
            .pick(&overlay, &label, backend_node_id, |change| {
                emit(&app, change)
            });
    });
}

/// Leaves Annotate on `label` after a navigation starts, the tab is hidden or
/// closes, or Escape reaches the page. Runs on its own thread: callers
/// include WebView2 event handlers on the main thread, which the overlay's
/// CDP calls need free.
pub(crate) fn leave(app: &AppHandle, label: &str, exit: Exit) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state.annotate.run(
            |change| emit(&app, change),
            |mode| mode.leave(&overlay, &label, exit),
        );
    });
}

fn value(result: Result<impl Serialize, String>) -> Result<serde_json::Value, String> {
    result.and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
}

fn emit(app: &AppHandle, change: AnnotateChange) {
    let _ = app.emit_to("main", "browser:annotate", change);
}

struct NativeOverlay<'a>(&'a BrowserState, &'a AppHandle);

#[cfg(not(windows))]
impl InspectOverlay for NativeOverlay<'_> {
    fn start(&self, _: &str) -> Result<(), String> {
        let _ = (self.0, self.1);
        Err(UNAVAILABLE_REASON.unwrap_or_default().into())
    }

    fn stop(&self, _: &str, _: bool) -> Result<(), String> {
        Ok(())
    }

    fn pick(&self, _: &str, _: u64, _: &dyn Fn()) -> Result<Selection, String> {
        Err(UNAVAILABLE_REASON.unwrap_or_default().into())
    }

    fn release(&self, _: &str, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }

    fn focus(&self, _: &str, _: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
