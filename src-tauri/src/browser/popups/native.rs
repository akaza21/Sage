//! Windows only (S1 Option C): Sage's own WebView2 `NewWindowRequested`
//! handler, so an admitted popup becomes a normal policed Sage tab that keeps
//! `window.opener`. All policy lives in `opener.rs`; this file only moves
//! WebView2 objects and checks every HRESULT. Any failure falls back to the
//! deny-and-enqueue intent path.
use std::{cell::RefCell, collections::BTreeMap, sync::mpsc, time::Duration};

use tauri::{AppHandle, Emitter, Manager, Webview};
use url::Url;
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2Deferral,
        ICoreWebView2NewWindowRequestedEventArgs,
    },
    NewWindowRequestedEventHandler,
};
use windows_core::{Interface, PWSTR};

use super::super::{BrowserState, downloads, lifecycle};
use super::{INITIAL_BLANK, enqueue_intent};

struct PendingPopup {
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    environment: lifecycle::TabEnvironment,
    opener: String,
    requested: String,
    url: String,
}

thread_local! {
    // WebView2 interfaces are apartment-bound and must never enter
    // BrowserState. One entry per reserved popup, so MAX_PENDING_POPUPS bounds it.
    static PENDING: RefCell<BTreeMap<u64, PendingPopup>> = const { RefCell::new(BTreeMap::new()) };
}

pub(crate) async fn register_native(app: AppHandle, webview: Webview) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let opener = webview.label().to_owned();
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let mut token = 0;
                unsafe {
                    core.add_NewWindowRequested(
                        &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                            if let Some(args) = args {
                                request(&app, &opener, sender, args);
                            }
                            Ok(())
                        })),
                        &mut token,
                    )
                }
                .map_err(|error| error.to_string())
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?
}

/// Runs inside the WebView2 callback: admit, take a deferral, and post the
/// tab creation to the event loop (creating a WebView2 here would re-enter).
fn request(
    app: &AppHandle,
    opener: &str,
    sender: Option<ICoreWebView2>,
    args: ICoreWebView2NewWindowRequestedEventArgs,
) {
    let mut uri = PWSTR::null();
    if let Err(error) = unsafe { args.Uri(&mut uri) } {
        deny_and_enqueue(
            app,
            &args,
            None,
            opener,
            "",
            &format!("Uri failed: {error}"),
        );
        return;
    }
    let requested = CoTaskMemPWSTR::from(uri).to_string();
    let state = app.state::<BrowserState>();
    let reserved = match (state.tabs.lock(), state.popups.lock()) {
        (Ok(tabs), Ok(mut popups)) => popups.reserve_native(&tabs, opener, &requested),
        _ => None,
    };
    let Some(intent) = reserved else {
        deny_and_enqueue(app, &args, None, opener, &requested, "not admitted");
        return;
    };
    let prepared = (|| -> Result<_, String> {
        let core: ICoreWebView2_2 = sender
            .ok_or("opener WebView2 unavailable")?
            .cast()
            .map_err(|error| error.to_string())?;
        let environment = unsafe { core.Environment() }.map_err(|error| error.to_string())?;
        let deferral = unsafe { args.GetDeferral() }.map_err(|error| error.to_string())?;
        Ok((environment, deferral))
    })();
    let (environment, deferral) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            release(app, intent.id, None);
            deny_and_enqueue(app, &args, None, opener, &requested, &error);
            return;
        }
    };
    PENDING.with_borrow_mut(|pending| {
        pending.insert(
            intent.id,
            PendingPopup {
                args,
                deferral,
                environment,
                opener: opener.to_owned(),
                requested,
                url: intent.url,
            },
        );
    });
    let id = intent.id;
    let dispatch_app = app.clone();
    // Off the main thread `run_on_main_thread` posts to the event loop
    // instead of running inline (tauri-runtime-wry-2.11.4/src/lib.rs:235-254).
    tauri::async_runtime::spawn(async move {
        let attach_app = dispatch_app.clone();
        if let Err(error) = dispatch_app.run_on_main_thread(move || attach(&attach_app, id)) {
            eprintln!("browser popup dispatch failed; WebView2 deferral left open: {error}");
        }
    });
}

/// Main thread: create the tab, hand it to WebView2, hook it, then complete.
fn attach(app: &AppHandle, id: u64) {
    let Some((opener, requested, loading, environment)) = PENDING.with_borrow(|pending| {
        pending.get(&id).map(|popup| {
            (
                popup.opener.clone(),
                popup.requested.clone(),
                popup.url != INITIAL_BLANK,
                popup.environment.clone(),
            )
        })
    }) else {
        return;
    };
    let state = app.state::<BrowserState>();
    let label = match lifecycle::open_popup_tab(app, &state, environment, loading) {
        Ok(label) => label,
        Err(error) => return fail(app, id, None, &opener, &requested, &error),
    };
    if let Err(error) = adopt(app, &state, id, &opener, &label) {
        return fail(app, id, Some(label), &opener, &requested, &error);
    }
    let Some(popup) = PENDING.with_borrow_mut(|pending| pending.remove(&id)) else {
        return fail(app, id, Some(label), &opener, &requested, "request expired");
    };
    if let Err(error) = unsafe { popup.deferral.Complete() } {
        release(app, id, Some(label));
        return fall_back(
            app,
            &opener,
            &requested,
            &format!("Complete failed: {error}"),
        );
    }
    let intent = state
        .popups
        .lock()
        .ok()
        .and_then(|popups| popups.entries.get(&id).map(|(intent, _)| intent.clone()));
    if let Some(intent) = intent {
        let _ = app.emit_to("main", "browser:popup-request", intent);
    }
}

fn adopt(
    app: &AppHandle,
    state: &BrowserState,
    id: u64,
    opener: &str,
    label: &str,
) -> Result<(), String> {
    let webview = state
        .webview_for_tab(label)
        .ok_or("popup tab unavailable")?;
    webview
        .hide()
        .map_err(|error| format!("popup tab hide failed: {error}"))?;
    let linked = match (state.tabs.lock(), state.popups.lock()) {
        (Ok(tabs), Ok(mut popups)) => {
            tabs.entries.contains_key(opener) && popups.native_opened(id, label)
        }
        _ => false,
    };
    if !linked {
        return Err("popup opener closed during open".into());
    }
    state
        .downloads
        .lock()
        .map_err(|error| error.to_string())?
        .set_provenance(label, downloads::NavigationProvenance::User);
    set_new_window(&webview, id)?;
    // Event subscriptions on the new contents must follow SetNewWindow and
    // precede Complete, so NavigationStarting sees the first navigation.
    // `with_webview` runs inline here (main thread), so this cannot wait on itself.
    tauri::async_runtime::block_on(lifecycle::register_tab_hooks(app, &webview))
}

fn set_new_window(webview: &Webview, id: u64) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| format!("CoreWebView2 failed: {error}"))?;
                let args = PENDING
                    .with_borrow(|pending| pending.get(&id).map(|popup| popup.args.clone()))
                    .ok_or("popup request expired")?;
                unsafe { args.SetNewWindow(&core) }
                    .map_err(|error| format!("SetNewWindow failed: {error}"))?;
                unsafe { args.SetHandled(true) }
                    .map_err(|error| format!("SetHandled failed: {error}"))
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .try_recv()
        .map_err(|_| "popup attach did not run on the main thread".to_owned())?
}

fn fail(
    app: &AppHandle,
    id: u64,
    label: Option<String>,
    opener: &str,
    requested: &str,
    error: &str,
) {
    release(app, id, label);
    if let Some(popup) = PENDING.with_borrow_mut(|pending| pending.remove(&id)) {
        if let Err(error) = unsafe { popup.args.SetNewWindow(None::<&ICoreWebView2>) } {
            eprintln!("browser popup SetNewWindow reset failed: {error}");
        }
        deny_and_enqueue(
            app,
            &popup.args,
            Some(&popup.deferral),
            opener,
            requested,
            error,
        );
    }
}

/// Drops the popup's bookkeeping and closes a tab that was already created.
fn release(app: &AppHandle, id: u64, label: Option<String>) {
    let state = app.state::<BrowserState>();
    if let Ok(mut popups) = state.popups.lock() {
        popups.native_failed(id);
        if let Some(label) = &label {
            popups.forget_child(label);
        }
    }
    if let Some(label) = label {
        let close_app = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = close_app.state::<BrowserState>();
            if let Err(error) = lifecycle::close_browser_tab(&close_app, &state, label).await {
                eprintln!("browser popup cleanup close failed: {error}");
            }
        });
    }
}

fn deny_and_enqueue(
    app: &AppHandle,
    args: &ICoreWebView2NewWindowRequestedEventArgs,
    deferral: Option<&ICoreWebView2Deferral>,
    opener: &str,
    requested: &str,
    reason: &str,
) {
    if let Err(error) = unsafe { args.SetHandled(true) } {
        eprintln!("browser popup SetHandled failed: {error}");
    }
    if let Some(Err(error)) = deferral.map(|deferral| unsafe { deferral.Complete() }) {
        eprintln!("browser popup deferral Complete failed: {error}");
    }
    fall_back(app, opener, requested, reason);
}

fn fall_back(app: &AppHandle, opener: &str, requested: &str, reason: &str) {
    let queued = Url::parse(requested).is_ok_and(|url| enqueue_intent(app, opener, &url));
    let outcome = if queued {
        "queued without opener"
    } else {
        "denied"
    };
    eprintln!("browser popup {outcome}: {reason}");
}
