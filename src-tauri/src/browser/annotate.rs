//! Every native step runs off the main thread under the mode lock, and each
//! change is announced under that lock, so steps never interleave and the
//! chrome receives the changes in the order they happened.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::{BrowserSnapshot, BrowserState, BrowserTabs, page_context::valid_page_context_tab_id};

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod element;
#[cfg(windows)]
mod native;
pub(crate) mod selection;
pub(crate) mod zoom;

use selection::{Selection, SelectionView};

pub(crate) const UNAVAILABLE_REASON: Option<&str> = if cfg!(windows) {
    None
} else {
    Some("Annotate needs the WebView2 inspect overlay and is not available on this platform yet")
};

pub(super) fn unavailable_reason(snapshot: &BrowserSnapshot) -> Option<&'static str> {
    UNAVAILABLE_REASON.or((snapshot.loading
        || snapshot.error.is_some()
        || snapshot.committed_url.is_none())
    .then_some("Open a page to annotate"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    On,
    Off,
    Cancel,
    Confirm,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    Reveal,
    Hidden,
    Closed,
}

pub(crate) trait InspectOverlay {
    fn start(&self, label: &str) -> Result<(), String>;
    /// Skips the overlay when `closing`: its webview may already be gone.
    fn stop(&self, label: &str, closing: bool) -> Result<(), String>;
    /// Calls `freeze` before the hide, so a resize never shows the page.
    fn pick(
        &self,
        label: &str,
        backend_node_id: u64,
        freeze: &dyn Fn(),
    ) -> Result<Selection, String>;
    fn release(&self, label: &str, reveal: bool, resume: bool) -> Result<(), String>;
    /// Focuses the page when `page`, else Sage's webview, whose comment box
    /// takes the typing.
    fn focus(&self, label: &str, page: bool) -> Result<(), String>;
    fn clear_outline(&self, label: &str) -> Result<(), String>;
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

/// The atomics are lock-free copies (tab numbers, 0 for none) for the main
/// thread, which must never wait on the mode lock while a native step needs
/// that thread.
#[derive(Default)]
pub(crate) struct Annotate {
    mode: Mutex<AnnotateMode>,
    tab: AtomicU64,
    frozen: AtomicU64,
    covered: AtomicU64,
    deferred: AtomicU64,
    zoom_queued: AtomicBool,
}

impl Annotate {
    pub(crate) fn is_annotating(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.tab.load(Ordering::Acquire) == number)
    }

    pub(crate) fn is_frozen(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.frozen.load(Ordering::Acquire) == number)
    }

    /// Holds the mode lock so a start cannot run its enables between the check
    /// and `disable`; a busy lock skips `disable`. Returns whether `disable`
    /// ran. Never call it on the main thread.
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

    fn freeze(&self, label: &str) {
        self.frozen
            .store(tab_number(label).unwrap_or(0), Ordering::Release);
    }

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

#[derive(Default)]
struct AnnotateMode {
    tab: Option<String>,
    pending: Option<Selection>,
}

impl AnnotateMode {
    /// Starting the tab that is already on restarts its overlay, so a remounted
    /// chrome or a page whose overlay an Agent highlight took over recovers.
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

    /// The mode ends even if the overlay fails to stop, so the chrome never shows
    /// Annotate on for a page that may have lost it.
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

pub(super) fn target<T: Clone>(tabs: &BrowserTabs<T>, tab_id: &str) -> Result<T, String> {
    if !valid_page_context_tab_id(tab_id) {
        return Err("invalid browser tab id".into());
    }
    tabs.openable_url(tab_id)?;
    tabs.handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable".into())
}

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
            Action::Cancel => {
                let result = state.annotate.run(announce, |mode| {
                    mode.cancel(&overlay, &tab_id, state.annotate.is_covered(&tab_id))
                });
                zoom::show_deferred(&app, &tab_id);
                result
            }
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

/// Runs on its own thread: the report arrives on the main thread, which the
/// pick's CDP calls need free.
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

/// Runs on its own thread: callers include WebView2 event handlers on the
/// main thread, which the overlay's CDP calls need free.
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

    fn clear_outline(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
