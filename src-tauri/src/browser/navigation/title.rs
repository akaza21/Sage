use tauri::{AppHandle, Manager, Webview};
use webview2_com::{
    CoTaskMemPWSTR, DocumentTitleChangedEventHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2,
};
use windows_core::PWSTR;

use super::{BrowserState, emit_snapshot};

pub(super) fn capture_completed_title(
    app: &AppHandle,
    label: &str,
    url: &str,
    core: &ICoreWebView2,
) {
    let mut raw = PWSTR::null();
    if unsafe { core.DocumentTitle(&mut raw) }.is_err() {
        return;
    }
    let title = CoTaskMemPWSTR::from(raw).to_string();
    if let Ok(mut tabs) = app.state::<BrowserState>().tabs.lock() {
        tabs.set_document_title(label, url, &title);
    }
}

pub(super) fn register(
    app: AppHandle,
    webview: Webview,
    label: String,
    core: &ICoreWebView2,
) -> Result<(), String> {
    let changed = DocumentTitleChangedEventHandler::create(Box::new(move |core, _| {
        if let Some(core) = core {
            let mut source = PWSTR::null();
            let mut raw = PWSTR::null();
            unsafe {
                core.Source(&mut source)?;
                core.DocumentTitle(&mut raw)?;
            }
            let source = CoTaskMemPWSTR::from(source).to_string();
            let title = CoTaskMemPWSTR::from(raw).to_string();
            if app
                .state::<BrowserState>()
                .tabs
                .lock()
                .is_ok_and(|mut tabs| tabs.set_document_title(&label, &source, &title))
            {
                emit_snapshot(app.clone(), webview.clone());
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_DocumentTitleChanged(&changed, &mut token) }
        .map_err(|error| error.to_string())
}
