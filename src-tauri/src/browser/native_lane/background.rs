//! Observe native memory targets without evaluating the page.
use super::Lane;
use std::{sync::mpsc, time::Duration};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows_core::Interface;

impl Lane {
    pub fn hide_browser(&self, tab: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(crate::browser::page_controls::browser_hide(
            self.app.clone(),
            self.state(),
            tab.to_owned(),
            None,
        ))
    }

    pub fn browser_memory_target(&self, tab: &str) -> Result<&'static str, String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        webview
            .with_webview(move |platform| {
                let result = (|| {
                    let core = unsafe { platform.controller().CoreWebView2() }?
                        .cast::<ICoreWebView2_19>()?;
                    let mut target = Default::default();
                    unsafe { core.MemoryUsageTargetLevel(&mut target) }?;
                    Ok::<_, windows_core::Error>(match target {
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW => "low",
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL => "normal",
                        _ => "other",
                    })
                })()
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    }
}
