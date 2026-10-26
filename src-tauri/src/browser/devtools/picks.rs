//! Annotate's one CDP event, `Overlay.inspectNodeRequested`: Chromium raises
//! it when the user clicks an element in inspect mode, and the click never
//! reaches the page. It is registered only while a tab annotates.

use std::{cell::RefCell, collections::HashMap, sync::mpsc, time::Duration};

use tauri::Webview;
use webview2_com::{
    CoTaskMemPWSTR, DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2DevToolsProtocolEventReceiver,
};
use windows_core::{PCWSTR, PWSTR};

use crate::acp::browser::BrowserFailure;

use super::cdp_port::select::inspected_node;

const EVENT: &str = "Overlay.inspectNodeRequested";

thread_local! {
    static PICKS: RefCell<HashMap<String, (ICoreWebView2DevToolsProtocolEventReceiver, i64)>> =
        RefCell::new(HashMap::new());
}

/// Calls `on_pick` on the main thread with the backend node id of each
/// clicked element, replacing any earlier listener of this tab.
pub(crate) fn watch_picks(
    webview: &Webview,
    on_pick: impl Fn(u64) + Send + 'static,
) -> Result<(), BrowserFailure> {
    let label = webview.label().to_owned();
    on_main(webview, move |core| {
        let name = EVENT.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let source = unsafe { core.GetDevToolsProtocolEventReceiver(PCWSTR(name.as_ptr())) }
            .map_err(|_| BrowserFailure::Unavailable)?;
        let handler =
            DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_core, args| {
                if let Some(args) = args {
                    let mut raw = PWSTR::null();
                    unsafe { args.ParameterObjectAsJson(&mut raw)? };
                    if let Some(node) = inspected_node(&CoTaskMemPWSTR::from(raw).to_string()) {
                        on_pick(node);
                    }
                }
                Ok(())
            }));
        let mut token = 0;
        unsafe { source.add_DevToolsProtocolEventReceived(&handler, &mut token) }
            .map_err(|_| BrowserFailure::Unavailable)?;
        let previous = PICKS.with(|picks| picks.borrow_mut().insert(label, (source, token)));
        unregister(previous);
        Ok(())
    })
}

/// Stops listening for this tab's clicks.
pub(crate) fn unwatch_picks(webview: &Webview) -> Result<(), BrowserFailure> {
    let label = webview.label().to_owned();
    on_main(webview, move |_| {
        unregister(PICKS.with(|picks| picks.borrow_mut().remove(&label)));
        Ok(())
    })
}

fn unregister(registration: Option<(ICoreWebView2DevToolsProtocolEventReceiver, i64)>) {
    if let Some((source, token)) = registration {
        let _ = unsafe { source.remove_DevToolsProtocolEventReceived(token) };
    }
}

/// Runs `step` on the webview's thread and waits for it.
fn on_main(
    webview: &Webview,
    step: impl FnOnce(
        &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    ) -> Result<(), BrowserFailure>
    + Send
    + 'static,
) -> Result<(), BrowserFailure> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = unsafe { platform.controller().CoreWebView2() }
                .map_err(|_| BrowserFailure::Unavailable)
                .and_then(|core| step(&core));
            let _ = sender.send(result);
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| BrowserFailure::Unavailable)?
}
