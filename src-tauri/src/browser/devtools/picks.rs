//! Annotate's two CDP events, registered only while a tab annotates.
//! `Overlay.inspectNodeRequested`: the user clicked an element in inspect
//! mode, and the click never reached the page. `Overlay.nodeHighlightRequested`:
//! the pointer reached another element, which Sage outlines in the overlay
//! (`cdp_port/inspect.rs`) without blocking the main thread.

use std::{cell::RefCell, collections::HashMap, sync::mpsc, time::Duration};

use tauri::{AppHandle, Webview};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR,
    DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
        ICoreWebView2Controller, ICoreWebView2DevToolsProtocolEventReceiver,
    },
};
use windows_core::{PCWSTR, PWSTR};

use crate::acp::browser::BrowserFailure;

use super::cdp_port::{
    inspect::{HIDE_OUTLINE, box_model_params, hovered_node, outline_params},
    select::inspected_node,
};

const PICK: &str = "Overlay.inspectNodeRequested";
const HOVER: &str = "Overlay.nodeHighlightRequested";

type Registration = (ICoreWebView2DevToolsProtocolEventReceiver, i64);

thread_local! {
    static PICKS: RefCell<HashMap<String, [Registration; 2]>> = RefCell::new(HashMap::new());
}

/// Calls `on_pick` on the main thread with the backend node id of each
/// clicked element and outlines each hovered one, replacing any earlier
/// listeners of this tab.
pub(crate) fn watch_picks(
    webview: &Webview,
    on_pick: impl Fn(u64) + Send + 'static,
) -> Result<(), BrowserFailure> {
    let label = webview.label().to_owned();
    on_main(webview, move |core, controller| {
        let pick = listen(core, PICK, move |_, payload| {
            if let Some(node) = inspected_node(payload) {
                on_pick(node);
            }
        })?;
        let controller = controller.clone();
        let hover = listen(core, HOVER, move |core, payload| {
            outline(core, payload, page_zoom(&controller));
        })
        .inspect_err(|_| unregister(pick.clone()))?;
        let previous = PICKS.with(|picks| picks.borrow_mut().insert(label, [pick, hover]));
        previous.into_iter().flatten().for_each(unregister);
        Ok(())
    })
}

/// Stops listening for this tab's clicks and hovers. It needs only the app,
/// so a tab whose webview is already gone still lets go of its listeners.
pub(crate) fn unwatch_picks(app: &AppHandle, label: &str) -> Result<(), BrowserFailure> {
    let label = label.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let removed = PICKS.with(|picks| picks.borrow_mut().remove(&label));
        removed.into_iter().flatten().for_each(unregister);
        let _ = sender.send(());
    })
    .map_err(|_| BrowserFailure::Unavailable)?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| BrowserFailure::Unavailable)
}

/// Hands each `event` payload to `on_event` with the webview that raised it.
fn listen(
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

/// Reads the hovered element's box, then draws its outline from the
/// completion; an element without a box removes the outline. Leaving or
/// pausing inspect mode removes it one round trip after hovering ends, so
/// this outline always lands first.
fn outline(core: Option<&ICoreWebView2>, payload: &str, zoom: f64) {
    let (Some(core), Some(node)) = (core, hovered_node(payload)) else {
        return;
    };
    let drawer = core.clone();
    let measured =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |outcome, model| {
            let hinge = outcome.ok().and_then(|()| outline_params(&model, zoom));
            let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
            call(
                &drawer,
                HIDE_OUTLINE.0,
                hinge.as_deref().unwrap_or(HIDE_OUTLINE.1),
                &done,
            );
            Ok(())
        }));
    call(core, "DOM.getBoxModel", &box_model_params(node), &measured);
}

/// The page zoom WebView2 applies (1.0 at 100%), read when the pointer
/// reaches an element so Sage's zoom controls and Ctrl+wheel both count.
fn page_zoom(controller: &ICoreWebView2Controller) -> f64 {
    let mut zoom = 1.0;
    match unsafe { controller.ZoomFactor(&mut zoom) } {
        Ok(()) => zoom,
        Err(_) => f64::NAN,
    }
}

/// Issues one CDP call; `done` hears how it went.
fn call(
    core: &ICoreWebView2,
    method: &str,
    params: &str,
    done: &ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
) {
    let (method, params) = (wide(method), wide(params));
    let _ = unsafe {
        core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), done)
    };
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn unregister((source, token): Registration) {
    let _ = unsafe { source.remove_DevToolsProtocolEventReceived(token) };
}

/// Runs `step` on the webview's thread and waits for it.
fn on_main(
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
