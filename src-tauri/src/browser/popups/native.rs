//! Windows only (S1 Option C): Sage's own WebView2 `NewWindowRequested`
//! handler, so an admitted popup becomes a normal policed Sage tab that keeps
//! `window.opener`. All policy lives in `opener.rs`; this file only moves
//! WebView2 objects and checks every HRESULT; the deferral ordering is the
//! Linux-tested `settle.rs`. A popup falls back to the deny-and-enqueue intent
//! path only while WebView2 never received a child.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::mpsc,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager, Webview};
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
use super::opener::NATIVE_ACK_DEADLINE;
use super::settle::{NewWindowRequest, Settlement, settle};
use super::{INITIAL_BLANK, MAX_PENDING_POPUPS, emit_popup_intent, enqueue_intent};

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
    // BrowserState. Bounded by MAX_PENDING_POPUPS (checked before insert).
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
        return deny_and_enqueue(app, &args, opener, "", &format!("Uri failed: {error}"));
    }
    let requested = CoTaskMemPWSTR::from(uri).to_string();
    let state = app.state::<BrowserState>();
    let reserved = match (state.tabs.lock(), state.popups.lock()) {
        (Ok(tabs), Ok(mut popups)) => popups.reserve_native(&tabs, opener, &requested),
        _ => None,
    };
    let Some(intent) = reserved else {
        return deny_and_enqueue(app, &args, opener, &requested, "not admitted");
    };
    let prepared = (|| -> Result<_, String> {
        if PENDING.with_borrow(|pending| pending.len() >= MAX_PENDING_POPUPS) {
            return Err("too many deferred popups".into());
        }
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
            return deny_and_enqueue(app, &args, opener, &requested, &error);
        }
    };
    let id = intent.id;
    PENDING.with_borrow_mut(|pending| {
        pending.insert(
            id,
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
    let dispatch_app = app.clone();
    // Off the main thread `run_on_main_thread` posts to the event loop
    // instead of running inline (tauri-runtime-wry-2.11.4/src/lib.rs:235-254).
    tauri::async_runtime::spawn(async move {
        let attach_app = dispatch_app.clone();
        if let Err(error) = dispatch_app.run_on_main_thread(move || attach(&attach_app, id)) {
            // The event loop is gone, so the apartment-bound deferral cannot be
            // completed from here; release the bounded reservation at least.
            release(&dispatch_app, id, None);
            eprintln!("browser popup dispatch failed; WebView2 deferral left open: {error}");
        }
    });
}

/// Main thread: create, hide, link and hook the child, then let `settle`
/// call SetNewWindow/SetHandled and Complete the deferral exactly once.
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
    let created = lifecycle::open_popup_tab(app, &state, environment, loading);
    let label = created.as_ref().ok().cloned();
    let prepared = created.and_then(|label| prepare(app, &state, id, &opener, &label));
    let mut request = NativeRequest {
        id,
        webview: label
            .as_deref()
            .and_then(|label| state.webview_for_tab(label)),
    };
    match settle(&mut request, prepared) {
        Settlement::Attached => {
            emit_popup_intent(app, id);
            expire_unacknowledged(app);
        }
        Settlement::FallBack(reason) => {
            release(app, id, label);
            fall_back(app, &opener, &requested, &reason);
        }
        Settlement::Abandoned(reason) => {
            // WebView2 may already host the popup: close it, never duplicate it.
            release(app, id, label);
            eprintln!("browser popup closed without fallback: {reason}");
        }
    }
}

fn prepare(
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
            tabs.entries.contains_key(opener) && popups.native_opened(id, label, Instant::now())
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
    // Hooks go on before SetNewWindow so that SetNewWindow stays the last
    // step that can still fall back without a duplicate tab. `with_webview`
    // runs inline on the main thread, so `block_on` cannot wait on itself.
    tauri::async_runtime::block_on(lifecycle::register_tab_hooks(app, &webview))
}

/// The real WebView2 request; `complete` removes it, so it runs at most once.
struct NativeRequest {
    id: u64,
    webview: Option<Webview>,
}

fn pending_args(id: u64) -> Result<ICoreWebView2NewWindowRequestedEventArgs, String> {
    PENDING
        .with_borrow(|pending| pending.get(&id).map(|popup| popup.args.clone()))
        .ok_or_else(|| "popup request expired".to_owned())
}

impl NewWindowRequest for NativeRequest {
    fn set_new_window(&mut self) -> Result<(), String> {
        let webview = self.webview.as_ref().ok_or("popup tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let id = self.id;
        webview
            .with_webview(move |platform| {
                let result = (|| {
                    let core = unsafe { platform.controller().CoreWebView2() }
                        .map_err(|error| format!("CoreWebView2 failed: {error}"))?;
                    unsafe { pending_args(id)?.SetNewWindow(&core) }
                        .map_err(|error| format!("SetNewWindow failed: {error}"))
                })();
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .try_recv()
            .map_err(|_| "popup attach did not run on the main thread".to_owned())?
    }

    fn set_handled(&mut self) -> Result<(), String> {
        unsafe { pending_args(self.id)?.SetHandled(true) }.map_err(|error| error.to_string())
    }

    fn complete(&mut self) -> Result<(), String> {
        let popup = PENDING
            .with_borrow_mut(|pending| pending.remove(&self.id))
            .ok_or("deferral already completed")?;
        unsafe { popup.deferral.Complete() }.map_err(|error| error.to_string())
    }
}

/// Closes native children the chrome never acknowledged within the deadline.
fn expire_unacknowledged(app: &AppHandle) {
    let sweep_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(NATIVE_ACK_DEADLINE).await;
        let state = sweep_app.state::<BrowserState>();
        let expired = match state.popups.lock() {
            Ok(mut popups) => popups.take_expired_native(Instant::now()),
            Err(_) => Vec::new(),
        };
        for (id, tab_id) in expired {
            let closed = lifecycle::close_browser_tab(&sweep_app, &state, tab_id.clone()).await;
            let gone = state
                .tabs
                .lock()
                .is_ok_and(|tabs| !tabs.entries.contains_key(&tab_id));
            if closed.is_ok() || gone {
                if let Ok(mut popups) = state.popups.lock() {
                    popups.finish_cleanup(id, &tab_id);
                }
            } else if let Err(error) = closed {
                eprintln!("browser popup deadline close failed: {error}");
            }
        }
    });
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

/// Before any deferral: deny the popup synchronously and fall back.
fn deny_and_enqueue(
    app: &AppHandle,
    args: &ICoreWebView2NewWindowRequestedEventArgs,
    opener: &str,
    requested: &str,
    reason: &str,
) {
    if let Err(error) = unsafe { args.SetHandled(true) } {
        eprintln!("browser popup SetHandled failed: {error}");
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
