//! A page out of view asks WebView2 to keep less memory, and asks for the
//! usual amount again when it is shown. The page keeps running, unlike a
//! suspended one, so an agent driving a hidden tab over CDP is unaffected.

/// Best effort: a WebView2 runtime too old for the setting keeps its memory.
#[cfg(windows)]
pub(super) fn keep_less(webview: &tauri::Webview, hidden: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows_core::Interface;

    let level = if hidden {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    let _ = webview.with_webview(move |platform| {
        let _ = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| core.SetMemoryUsageTargetLevel(level))
        };
    });
}

#[cfg(not(windows))]
pub(super) fn keep_less(_webview: &tauri::Webview, _hidden: bool) {}
