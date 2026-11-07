//! Annotate across a page zoom and a toolbar popover (BP-H2b). As in Codex,
//! neither ends the mode. A popover such as the "…" menu or the downloads
//! list covers the page, which is a native window above Sage's DOM, so the
//! page hides while it is open; nothing can be picked meanwhile, and hovering
//! resumes once the page is placed again. A zoom moves every element, so the
//! outline drawn before it goes, and an element waiting for a comment is
//! dropped, as its frozen frame and outline no longer match the page.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};

use super::{
    Annotate, AnnotateChange, AnnotateMode, BrowserState, Exit, InspectOverlay, NativeOverlay,
    emit, tab_number,
};

/// Clears `copy` if it names `label`'s tab.
fn clear(copy: &AtomicU64, label: &str) {
    if let Some(number) = tab_number(label) {
        let _ = copy.compare_exchange(number, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

impl Annotate {
    /// Hides `label`'s page: a covering popover keeps Annotate on and is
    /// remembered until the page is placed again; any other hide forgets the
    /// cover and answers how Annotate leaves. Either way a show that a frozen
    /// page put off no longer applies.
    pub(crate) fn hiding(&self, label: &str, cover: bool) -> Option<Exit> {
        clear(&self.deferred, label);
        if cover {
            self.covered
                .store(tab_number(label).unwrap_or(0), Ordering::Release);
            None
        } else {
            clear(&self.covered, label);
            Some(Exit::Hidden)
        }
    }

    /// `label`'s page was placed again, so no popover covers it. Answers
    /// whether to show it: a frozen page stays hidden, and the show is put
    /// off until a zoom unfreezes it. Lock-free, as placing runs on the main
    /// thread.
    pub(crate) fn placed(&self, label: &str) -> bool {
        clear(&self.covered, label);
        if self.is_frozen(label) {
            self.deferred
                .store(tab_number(label).unwrap_or(0), Ordering::Release);
            false
        } else {
            clear(&self.deferred, label);
            true
        }
    }

    /// Whether a popover covers `label`'s page.
    pub(crate) fn is_covered(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.covered.load(Ordering::Acquire) == number)
    }

    /// Takes the show a frozen page put off, once it is no longer frozen.
    /// Runs on the main thread, after any placement or hide queued before it.
    pub(super) fn take_deferred_show(&self, label: &str) -> bool {
        let Some(number) = tab_number(label) else {
            return false;
        };
        !self.is_frozen(label)
            && self
                .deferred
                .compare_exchange(number, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    /// Queues a zoom step; false when one already waits, which will see this
    /// zoom too. Ctrl+wheel raises a zoom per tick.
    pub(super) fn queue_zoom(&self) -> bool {
        !self.zoom_queued.swap(true, Ordering::AcqRel)
    }

    /// `label`'s page zoom changed; release any waiting selection.
    pub(super) fn zoomed(
        &self,
        overlay: &impl InspectOverlay,
        label: &str,
        announce: impl Fn(AnnotateChange),
    ) -> Result<(), String> {
        self.run(announce, |mode| {
            // Under the lock: a zoom after this point queues the next step.
            self.zoom_queued.store(false, Ordering::Release);
            let covered = self.is_covered(label);
            mode.zoomed(overlay, label, covered)
        })
    }
}

impl AnnotateMode {
    /// Keeps Annotate on through a zoom. A waiting element is dropped and
    /// hovering resumes; the page shows again unless a popover covers it, in
    /// which case placing it shows it. Otherwise the last outline goes, and
    /// the next hover draws one at the new zoom.
    pub(super) fn zoomed(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        covered: bool,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) {
            return (Vec::new(), Ok(()));
        }
        if self.pending.take().is_none() {
            return (Vec::new(), overlay.clear_outline(label));
        }
        let released = overlay.release(label, !covered, true);
        if !covered {
            let _ = overlay.focus(label, true);
        }
        (vec![self.resumed(label, &released)], released)
    }
}

/// Keeps Annotate on `label` through a page zoom from any source: the "…"
/// menu, a shortcut, Ctrl+wheel or a reset. Runs on its own thread, as the
/// zoom event arrives on the main thread that the overlay needs; at most one
/// such thread runs and one waits.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn zoomed(app: &AppHandle, label: &str) {
    if !app.state::<BrowserState>().annotate.queue_zoom() {
        return;
    }
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state
            .annotate
            .zoomed(&overlay, &label, |change| emit(&app, change));
        // A cover may have opened AND closed after the sample. Drain any
        // placement that found the page still frozen, regardless of that sample.
        show_deferred(&app, &label);
    });
}

/// A placement may have found the page frozen while zoom or cancellation
/// released its selection. Finish that placement after the mode unfreezes.
pub(super) fn show_deferred(app: &AppHandle, label: &str) {
    let state = app.state::<BrowserState>();
    let Some(webview) = state.webview_for_tab(label) else {
        return;
    };
    let Some(visibility) = state.page_visibility_for_tab(label) else {
        return;
    };
    let main = app.clone();
    let label = label.to_owned();
    let _ = app.run_on_main_thread(move || {
        if main
            .state::<BrowserState>()
            .annotate
            .take_deferred_show(&label)
        {
            let _ = visibility.reveal_page(&webview, || {
                webview.show().map_err(|error| error.to_string())
            });
        }
    });
}
