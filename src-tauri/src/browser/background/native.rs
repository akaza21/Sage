use tauri::{Manager, Webview};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows_core::Interface;

use super::{MemoryState, MemoryTarget};
use crate::browser::BrowserState;

/// Called on the main thread for creation, hide, reveal and window resize.
/// Re-read placement inside the callback so queued work cannot restore a
/// hidden page to NORMAL. No tasks, listeners or per-window target cache.
pub(crate) fn refresh(webview: &Webview) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let window = webview.window();
    if let Err(error) = webview.with_webview(move |platform| {
        let result = (|| {
            let minimized = window.is_minimized().map_err(|e| e.to_string())?;
            let mut memory_state = MemoryState::for_visibility(true, minimized);
            if let Some(browser) = app.try_state::<BrowserState>() {
                let mut tabs = browser.tabs.lock().map_err(|_| "browser unavailable")?;
                if let Some(tab) = tabs.entries.get_mut(&label) {
                    tab.background =
                        MemoryState::for_visibility(tab.visibility.is_placed(), minimized);
                    memory_state = tab.background;
                }
            }
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
