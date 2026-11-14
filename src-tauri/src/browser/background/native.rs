use tauri::{Manager, Webview};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows_core::Interface;

use super::{MemoryTarget, MemoryTargetPort};
use crate::browser::BrowserState;

/// Requested on creation, hide, reveal and minimize/restore. The update runs on
/// the WebView's thread and re-reads placement there, so a queued update
/// cannot restore a page hidden since to NORMAL. No per-WebView cache: the
/// current level is read back and only changed when it differs.
impl MemoryTargetPort for Webview {
    fn update_memory_target(
        &self,
        policy: impl FnOnce(Option<bool>, bool) -> MemoryTarget + Send + 'static,
    ) {
        let app = self.app_handle().clone();
        let label = self.label().to_owned();
        let window = self.window();
        if let Err(error) = self.with_webview(move |platform| {
            let result = (|| {
                let minimized = window.is_minimized().map_err(|e| e.to_string())?;
                let placement = app
                    .try_state::<BrowserState>()
                    .and_then(|browser| browser.page_visibility_for_tab(&label))
                    .map(|visibility| visibility.is_placed());
                let core =
                    unsafe { platform.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
                let Ok(memory) = core.cast::<ICoreWebView2_19>() else {
                    tracing::debug!(%label, "WebView2 runtime lacks memory targets");
                    return Ok(());
                };
                let target = match policy(placement, minimized) {
                    MemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                    MemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                };
                let mut current = target;
                if unsafe { memory.MemoryUsageTargetLevel(&mut current) }.is_ok()
                    && current == target
                {
                    return Ok(());
                }
                unsafe { memory.SetMemoryUsageTargetLevel(target) }.map_err(|e| e.to_string())
            })();
            if let Err(error) = result {
                tracing::warn!(%label, %error, "WebView2 memory target update failed");
            }
        }) {
            tracing::warn!(webview = self.label(), %error, "WebView2 memory target dispatch failed");
        }
    }
}
