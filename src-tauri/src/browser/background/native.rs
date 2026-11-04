use tauri::{Manager, Webview};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows_core::Interface;

use super::{MemoryState, MemoryTarget};
use crate::browser::BrowserState;

/// Requested on creation, hide, reveal and window resize. The update runs on
/// the WebView's thread and re-reads placement there, so a queued update
/// cannot restore a page hidden since to NORMAL. No per-window cache: the
/// current level is read back and only changed when it differs.
pub(crate) fn refresh(webview: &Webview) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let window = webview.window();
    if let Err(error) = webview.with_webview(move |platform| {
        let result = (|| {
            let minimized = window.is_minimized().map_err(|e| e.to_string())?;
            // Only browser tabs have a placement; every other webview counts as placed.
            let placed = app
                .try_state::<BrowserState>()
                .and_then(|browser| browser.page_visibility_for_tab(&label))
                .is_none_or(|visibility| visibility.is_placed());
            let memory_state = MemoryState::for_visibility(placed, minimized);
            let core =
                unsafe { platform.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
            let Ok(memory) = core.cast::<ICoreWebView2_19>() else {
                tracing::debug!(%label, "WebView2 runtime lacks memory targets");
                return Ok(());
            };
            let target = match memory_state.target() {
                MemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                MemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            let mut current = target;
            if unsafe { memory.MemoryUsageTargetLevel(&mut current) }.is_ok() && current == target {
                return Ok(());
            }
            unsafe { memory.SetMemoryUsageTargetLevel(target) }.map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            tracing::warn!(%label, %error, "WebView2 memory target update failed");
        }
    }) {
        tracing::warn!(webview = webview.label(), %error, "WebView2 memory target dispatch failed");
    }
}
