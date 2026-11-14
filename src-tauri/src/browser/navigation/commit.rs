//! WebView2 fires `ContentLoading` after `SourceChanged` and before
//! `NavigationCompleted`; `outcome` relies on it to tell a committed document
//! from a navigation that never committed.

use tauri::{AppHandle, Manager};
use webview2_com::{ContentLoadingEventHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2};
use windows_core::BOOL;

use crate::browser::BrowserState;

pub(super) fn register(app: AppHandle, label: String, core: &ICoreWebView2) -> Result<i64, String> {
    let handler = ContentLoadingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut id = 0;
            let mut error_page = BOOL(0);
            unsafe {
                args.NavigationId(&mut id)?;
                args.IsErrorPage(&mut error_page)?;
            }
            if let Ok(mut tabs) = app.state::<BrowserState>().tabs.lock() {
                tabs.note_content_loading(&label, id, error_page.as_bool());
            }
            // An error page replaces the old document too.
            crate::browser::site_permissions::document_committed_native(&app, &label, id);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_ContentLoading(&handler, &mut token) }.map_err(|error| error.to_string())?;
    Ok(token)
}
