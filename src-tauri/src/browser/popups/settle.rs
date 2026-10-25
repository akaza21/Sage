//! The WebView2 `NewWindowRequested` deferral state machine, behind a seam so
//! its ordering is unit-tested on Linux. `native.rs` implements the seam with
//! the real args and deferral.
#![cfg_attr(not(windows), allow(dead_code))]

/// One deferred popup request. `complete` must be called exactly once.
pub(crate) trait NewWindowRequest {
    /// `SetNewWindow` with the child tab's `CoreWebView2`.
    fn set_new_window(&mut self) -> Result<(), String>;
    fn set_handled(&mut self) -> Result<(), String>;
    fn complete(&mut self) -> Result<(), String>;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Settlement {
    /// The child tab hosts the popup and keeps `window.opener`.
    Attached,
    /// WebView2 never received a child: the popup was denied and the caller
    /// closes any created tab and falls back to the intent path.
    FallBack(String),
    /// `Complete` failed after `SetNewWindow`: WebView2 may already host the
    /// popup, so the caller closes the child and must not queue a second,
    /// opener-less tab.
    Abandoned(String),
}

/// Settles a deferred request after the child tab was prepared (created,
/// hidden, linked and hooked). `SetNewWindow` is the last fallible step that
/// can still fall back; the deferral is completed exactly once on every path.
pub(crate) fn settle(
    request: &mut impl NewWindowRequest,
    prepared: Result<(), String>,
) -> Settlement {
    let attached = prepared.and_then(|()| request.set_new_window());
    if let Err(error) = attached {
        let mut reason = error;
        if let Err(error) = request.set_handled() {
            reason = format!("{reason}; SetHandled failed: {error}");
        }
        if let Err(error) = request.complete() {
            reason = format!("{reason}; Complete failed: {error}");
        }
        return Settlement::FallBack(reason);
    }
    // With NewWindow set, WebView2 uses the child even if this report fails
    // (wry's own handler has already marked the request handled).
    if let Err(error) = request.set_handled() {
        eprintln!("browser popup SetHandled failed after SetNewWindow: {error}");
    }
    match request.complete() {
        Ok(()) => Settlement::Attached,
        Err(error) => Settlement::Abandoned(format!("Complete failed: {error}")),
    }
}

#[cfg(test)]
#[path = "settle_tests.rs"]
mod tests;
