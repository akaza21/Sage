//! The grant-owned session cannot set Emulation overrides (the CDP allowlist
//! denies them), so its Page.captureScreenshot uses native scale. A toolbar
//! override belongs to another session and must not replace this scale.
use std::{sync::mpsc, time::Duration};
use tauri::Webview;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Controller3,
};
use windows_core::Interface;

use crate::acp::browser::BrowserFailure;

pub(super) fn read(webview: &Webview, timeout: Duration) -> Result<f64, BrowserFailure> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = with_scale(&platform.controller(), Ok);
            let _ = sender.try_send(result);
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    receiver
        .recv_timeout(timeout)
        .map_err(|_| BrowserFailure::Unavailable)?
}

/// Read scale immediately before admission and CDP issue on the UI thread.
/// Another session's toolbar settings do not govern this capture.
pub(in crate::browser::devtools) fn with_scale<R>(
    controller: &ICoreWebView2Controller,
    issue: impl FnOnce(f64) -> Result<R, BrowserFailure>,
) -> Result<R, BrowserFailure> {
    let mut native = 0.0;
    unsafe {
        controller
            .cast::<ICoreWebView2Controller3>()
            .and_then(|controller| controller.RasterizationScale(&mut native))
    }
    .map_err(|_| BrowserFailure::Unavailable)?;
    if !native.is_finite() || native <= 0.0 {
        return Err(BrowserFailure::Unavailable);
    }
    issue(native)
}
