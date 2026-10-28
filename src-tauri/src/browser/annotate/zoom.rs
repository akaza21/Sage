//! Annotate across a page zoom and a toolbar popover (BP-H2b). As in Codex,
//! neither ends the mode. A popover such as the "…" menu or the downloads
//! list covers the page, which is a native window above Sage's DOM, so the
//! page hides while it is open; nothing can be picked meanwhile, and hovering
//! resumes once the page is placed again. A zoom moves every element, so the
//! outline drawn before it goes, and an element waiting for a comment is
//! dropped, as its frozen frame and outline no longer match the page.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};

use super::{
    Annotate, AnnotateChange, AnnotateMode, BrowserState, Exit, InspectOverlay, NativeOverlay,
    emit, tab_number,
};

impl Annotate {
    /// Hides `label`'s page: a covering popover keeps Annotate on and is
    /// remembered until the page is placed again; any other hide answers how
    /// Annotate leaves.
    pub(crate) fn hiding(&self, label: &str, cover: bool) -> Option<Exit> {
        if cover {
            self.covered
                .store(tab_number(label).unwrap_or(0), Ordering::Release);
            None
        } else {
            Some(Exit::Hidden)
        }
    }

    /// `label`'s page was placed again, so no popover covers it. Lock-free,
    /// as placing runs on the main thread.
    pub(crate) fn uncover(&self, label: &str) {
        if let Some(number) = tab_number(label) {
            let _ = self
                .covered
                .compare_exchange(number, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    /// Whether a popover covers `label`'s page.
    pub(crate) fn is_covered(&self, label: &str) -> bool {
        tab_number(label).is_some_and(|number| self.covered.load(Ordering::Acquire) == number)
    }

    /// `label`'s page zoom changed.
    pub(super) fn zoomed(
        &self,
        overlay: &impl InspectOverlay,
        label: &str,
        announce: impl Fn(AnnotateChange),
    ) -> Result<(), String> {
        let covered = self.is_covered(label);
        self.run(announce, |mode| mode.zoomed(overlay, label, covered))
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
/// zoom event arrives on the main thread that the overlay needs.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn zoomed(app: &AppHandle, label: &str) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state
            .annotate
            .zoomed(&overlay, &label, |change| emit(&app, change));
    });
}
