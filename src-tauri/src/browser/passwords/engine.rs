//! WebView2's own password manager is turned off on every browser tab, so
//! Sage's vault is the only place a password is saved or filled from.

use std::{sync::mpsc, time::Duration};

use tauri::Webview;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
use windows_core::Interface;

pub(in crate::browser) fn disable_engine_password_manager(webview: &Webview) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| {
                // SAFETY: WebView2 calls on the webview's own thread.
                let settings = unsafe { platform.controller().CoreWebView2()?.Settings()? }
                    .cast::<ICoreWebView2Settings4>()?;
                unsafe {
                    settings.SetIsPasswordAutosaveEnabled(false)?;
                    settings.SetIsGeneralAutofillEnabled(false)
                }
            })()
            .map_err(|error: windows_core::Error| error.to_string());
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?
}
