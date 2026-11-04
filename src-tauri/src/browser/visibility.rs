//! Per-webview placement intent, separate from Annotate's temporary freeze.
//! Updated and checked on the main thread so a queued release cannot undo a
//! tab switch or cover. Each tab owns one flag; no paths or history accumulate.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::BrowserState;

impl BrowserState {
    pub(super) fn page_visibility_for_tab(&self, label: &str) -> Option<Arc<PageVisibility>> {
        Some(
            self.tabs
                .lock()
                .ok()?
                .entries
                .get(label)?
                .visibility
                .clone(),
        )
    }
}

#[derive(Default)]
pub(super) struct PageVisibility(AtomicBool);

impl PageVisibility {
    pub(super) fn is_placed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(super) fn place(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(super) fn hide(&self) {
        self.0.store(false, Ordering::Release);
    }

    /// The caller runs this check and the native show/focus on the main thread.
    pub(super) fn reveal(&self, show: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if self.is_placed() { show() } else { Ok(()) }
    }

    /// Called on the main thread; wake finishes before the first visible frame.
    pub(super) fn reveal_page(
        &self,
        webview: &tauri::Webview,
        show: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        self.reveal(|| {
            let _awake = super::background::ensure_awake(webview)?;
            show()
        })
    }
}

#[cfg(test)]
#[path = "visibility_tests.rs"]
mod tests;
