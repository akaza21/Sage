use tauri::{AppHandle, Manager, Webview};
use webview2_com::{
    CoTaskMemPWSTR, FaviconChangedEventHandler, GetFaviconCompletedHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2,
};
use webview2_com_sys::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, ICoreWebView2_15,
};
use windows::Win32::System::Com::IStream;
use windows_core::{Interface, PWSTR};

use super::{BrowserState, emit_snapshot};
use crate::browser::favicon::MAX_FAVICON_BYTES;

pub(super) fn register(app: AppHandle, webview: Webview, label: String, core: &ICoreWebView2) {
    let Ok(version) = core.cast::<ICoreWebView2_15>() else {
        return;
    };
    let changed = FaviconChangedEventHandler::create(Box::new(move |core, _| {
        if let Some(core) = core {
            request_favicon(&app, &webview, &label, &core);
        }
        Ok(())
    }));
    let mut token = 0;
    let _ = unsafe { version.add_FaviconChanged(&changed, &mut token) };
}

pub(super) fn capture_completed(
    app: &AppHandle,
    webview: &Webview,
    label: &str,
    core: &ICoreWebView2,
) {
    request_favicon(app, webview, label, core);
}

fn request_favicon(app: &AppHandle, webview: &Webview, label: &str, core: &ICoreWebView2) {
    let Ok(version) = core.cast::<ICoreWebView2_15>() else {
        return;
    };
    let mut source = PWSTR::null();
    if unsafe { core.Source(&mut source) }.is_err() {
        return;
    }
    let url = CoTaskMemPWSTR::from(source).to_string();
    let navigation_id = app
        .state::<BrowserState>()
        .tabs
        .lock()
        .ok()
        .and_then(|tabs| {
            let tab = tabs.entries.get(label)?;
            if tab.loading || tab.committed_url.as_deref() != Some(&url) {
                return None;
            }
            tab.navigation_id
        });
    let Some(navigation_id) = navigation_id else {
        return;
    };
    let app = app.clone();
    let webview = webview.clone();
    let label = label.to_owned();
    let completed = GetFaviconCompletedHandler::create(Box::new(move |outcome, stream| {
        if outcome.is_err() {
            return Ok(());
        }
        let bytes = match stream {
            Some(stream) => match read_bounded(&stream) {
                Some(bytes) if bytes.is_empty() => None,
                Some(bytes) => Some(bytes),
                None => return Ok(()),
            },
            None => None,
        };
        let changed = app
            .state::<BrowserState>()
            .tabs
            .lock()
            .is_ok_and(|mut tabs| {
                tabs.set_favicon_png(&label, &url, navigation_id, bytes.as_deref())
            });
        if changed {
            emit_snapshot(app.clone(), webview.clone());
        }
        Ok(())
    }));
    let _ = unsafe { version.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &completed) };
}

fn read_bounded(stream: &IStream) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let mut read = 0;
        unsafe {
            stream.Read(
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                Some(&mut read),
            )
        }
        .ok()
        .ok()?;
        if read == 0 {
            return Some(bytes);
        }
        if bytes.len() + read as usize > MAX_FAVICON_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
}
