use crate::acp::browser::BrowserFailure;
use std::{sync::mpsc, time::Duration};
use tauri::Webview;
use webview2_com::{
    CoTaskMemPWSTR, DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2DevToolsProtocolEventReceiver,
    },
};
use windows_core::{PCWSTR, PWSTR};

pub(super) type Registration = (ICoreWebView2DevToolsProtocolEventReceiver, i64);

pub(super) fn listen(
    core: &ICoreWebView2,
    event: &str,
    on_event: impl Fn(Option<&ICoreWebView2>, &str) + 'static,
) -> Result<Registration, BrowserFailure> {
    let name = wide(event);
    let source = unsafe { core.GetDevToolsProtocolEventReceiver(PCWSTR(name.as_ptr())) }
        .map_err(|_| BrowserFailure::Unavailable)?;
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |core, args| {
        if let Some(args) = args {
            let mut raw = PWSTR::null();
            unsafe { args.ParameterObjectAsJson(&mut raw)? };
            on_event(core.as_ref(), &CoTaskMemPWSTR::from(raw).to_string());
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { source.add_DevToolsProtocolEventReceived(&handler, &mut token) }
        .map_err(|_| BrowserFailure::Unavailable)?;
    Ok((source, token))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub(super) fn unregister((source, token): Registration) {
    let _ = unsafe { source.remove_DevToolsProtocolEventReceived(token) };
}

/// Runs `step` on the webview's thread and waits for it.
pub(super) fn on_main(
    webview: &Webview,
    step: impl FnOnce(&ICoreWebView2, &ICoreWebView2Controller) -> Result<(), BrowserFailure>
    + Send
    + 'static,
) -> Result<(), BrowserFailure> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let controller = platform.controller();
            let result = unsafe { controller.CoreWebView2() }
                .map_err(|_| BrowserFailure::Unavailable)
                .and_then(|core| step(&core, &controller));
            let _ = sender.send(result);
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| BrowserFailure::Unavailable)?
}
