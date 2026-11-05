use std::{cell::RefCell, collections::BTreeMap, sync::mpsc, time::Duration};

use tauri::{AppHandle, Emitter, Manager, Webview};
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ, COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS,
        COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_PERMISSION_STATE_DENY,
        ICoreWebView2Deferral, ICoreWebView2PermissionRequestedEventArgs,
        ICoreWebView2PermissionRequestedEventArgs2, ICoreWebView2PermissionRequestedEventArgs3,
    },
    PermissionRequestedEventHandler,
};
use windows_core::{Interface, PWSTR};

use super::{
    Decision, PermissionKind, RequestOutcome,
    commands::{emit_site_settings, snapshot},
};
use crate::browser::{BrowserState, admit_url};

struct PendingPermission {
    args: ICoreWebView2PermissionRequestedEventArgs3,
    deferral: ICoreWebView2Deferral,
}

thread_local! {
    static PENDING: RefCell<BTreeMap<u64, Vec<PendingPermission>>> = const { RefCell::new(BTreeMap::new()) };
}

fn kind_from_native(kind: COREWEBVIEW2_PERMISSION_KIND) -> Option<PermissionKind> {
    match kind {
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => Some(PermissionKind::Camera),
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => Some(PermissionKind::Microphone),
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => Some(PermissionKind::Geolocation),
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => Some(PermissionKind::Notifications),
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => Some(PermissionKind::ClipboardRead),
        _ => None,
    }
}

fn deny(args: &ICoreWebView2PermissionRequestedEventArgs) {
    if let Ok(version) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
        let _ = unsafe { version.SetSavesInProfile(false) };
        let _ = unsafe { version.SetHandled(true) };
    } else if let Ok(version) = args.cast::<ICoreWebView2PermissionRequestedEventArgs2>() {
        let _ = unsafe { version.SetHandled(true) };
    }
    let _ = unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY) };
}

fn emit_snapshot(app: &AppHandle, tab_id: &str) {
    if let Ok(snapshot) = snapshot(&app.state::<BrowserState>(), tab_id) {
        let _ = app.emit_to("main", "browser:permissions", snapshot);
    }
}

pub(crate) async fn register_native(app: AppHandle, webview: Webview) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let tab_id = webview.label().to_owned();
    let callback_webview = webview.clone();
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let mut token = 0;
                unsafe {
                    core.add_PermissionRequested(
                        &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                            if let Some(args) = args {
                                handle_request(&app, &callback_webview, &tab_id, args);
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

fn handle_request(
    app: &AppHandle,
    webview: &Webview,
    tab_id: &str,
    args: ICoreWebView2PermissionRequestedEventArgs,
) {
    let prepared = (|| {
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = CoTaskMemPWSTR::from(uri).to_string();
        let mut native_kind = COREWEBVIEW2_PERMISSION_KIND::default();
        unsafe { args.PermissionKind(&mut native_kind)? };
        Ok::<_, windows_core::Error>((uri, native_kind))
    })();
    let Ok((uri, native_kind)) = prepared else {
        deny(&args);
        return;
    };
    let Some(kind) = kind_from_native(native_kind) else {
        deny(&args);
        return;
    };
    let Ok(version) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() else {
        deny(&args);
        return;
    };
    if unsafe { version.SetSavesInProfile(false) }.is_err()
        || unsafe { version.SetHandled(true) }.is_err()
    {
        deny(&args);
        return;
    }
    let agent = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .map(|downloads| downloads.is_agent(tab_id))
        .unwrap_or(true);
    let outcome = app
        .state::<BrowserState>()
        .permissions
        .lock()
        .map(|mut permissions| permissions.request(tab_id, &uri, kind, agent))
        .unwrap_or(RequestOutcome::Deny);
    match outcome {
        RequestOutcome::Allow => {
            if unsafe { version.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW) }.is_err() {
                deny(&args);
            }
        }
        RequestOutcome::Deny => deny(&args),
        RequestOutcome::Prompt(prompt) => {
            let Ok(deferral) = (unsafe { version.GetDeferral() }) else {
                if let Ok(mut permissions) = app.state::<BrowserState>().permissions.lock() {
                    permissions.remove_waiter(prompt.id);
                }
                deny(&args);
                return;
            };
            let _ = unsafe { version.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY) };
            let first = PENDING.with_borrow_mut(|pending| {
                let attached = pending.entry(prompt.id).or_default();
                attached.push(PendingPermission {
                    args: version,
                    deferral,
                });
                attached.len() == 1
            });
            if first {
                emit_snapshot(app, tab_id);
                let timeout_app = app.clone();
                let timeout_webview = webview.clone();
                let timeout_tab = tab_id.to_owned();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let _ = timeout_webview.with_webview(move |_| {
                        finish(&timeout_app, &timeout_tab, prompt.id, Decision::Deny, false);
                    });
                });
            }
        }
    }
}

fn finish(app: &AppHandle, tab_id: &str, id: u64, decision: Decision, save: bool) -> bool {
    let pending = PENDING.with_borrow_mut(|pending| pending.remove(&id));
    let Some(pending) = pending else {
        return false;
    };
    let origin_matches = !save || committed_origin_matches(app, tab_id, id);
    let state = match if origin_matches {
        decision
    } else {
        Decision::Deny
    } {
        Decision::Allow => COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        Decision::Deny => COREWEBVIEW2_PERMISSION_STATE_DENY,
    };
    let configured = pending.iter().all(|entry| {
        unsafe { entry.args.SetSavesInProfile(false) }
            .and_then(|()| unsafe { entry.args.SetHandled(true) })
            .and_then(|()| unsafe { entry.args.SetState(state) })
            .is_ok()
    });
    let mut settled = Vec::new();
    let recorded = app
        .state::<BrowserState>()
        .permissions
        .lock()
        .is_ok_and(|mut permissions| {
            if save && origin_matches && configured {
                if let Some(others) = permissions.resolve_site(id, tab_id, decision) {
                    settled = others;
                    true
                } else {
                    permissions.cancel(id);
                    false
                }
            } else {
                permissions.cancel(id)
            }
        });
    if !configured || !recorded {
        for entry in &pending {
            deny(&entry.args);
        }
    }
    for entry in pending {
        let _ = unsafe { entry.deferral.Complete() };
    }
    if save && configured && recorded && origin_matches {
        // The saved answer settles the same site's prompts in other tabs.
        for (other, other_tab) in settled {
            finish(app, &other_tab, other, decision, false);
        }
        // A saved answer applies to the whole site, so every tab's drawer
        // and Settings > Browser refresh.
        emit_site_settings(app, &app.state::<BrowserState>());
    } else {
        emit_snapshot(app, tab_id);
    }
    configured && recorded && origin_matches
}

fn committed_origin_matches(app: &AppHandle, tab_id: &str, id: u64) -> bool {
    let expected = app
        .state::<BrowserState>()
        .permissions
        .lock()
        .ok()
        .and_then(|permissions| {
            permissions
                .pending(tab_id)
                .into_iter()
                .find(|prompt| prompt.id == id)
                .and_then(|prompt| {
                    (!permissions
                        .active_origin
                        .get(tab_id)
                        .is_some_and(|origin| origin != &prompt.origin))
                    .then_some(prompt.origin)
                })
        });
    let committed = app
        .state::<BrowserState>()
        .tabs
        .lock()
        .ok()
        .and_then(|tabs| {
            tabs.entries
                .get(tab_id)
                .and_then(|tab| tab.committed_url.as_deref().map(str::to_owned))
        })
        .and_then(|url| admit_url(&url).ok())
        .map(|url| url.origin().ascii_serialization());
    expected.is_some() && expected == committed
}

fn deny_stale(app: &AppHandle, tab_id: &str, ids: Vec<u64>) {
    for id in ids {
        if !finish(app, tab_id, id, Decision::Deny, false)
            && let Ok(mut permissions) = app.state::<BrowserState>().permissions.lock()
        {
            permissions.cancel(id);
        }
    }
    emit_snapshot(app, tab_id);
}

pub(crate) fn deny_navigation_native(app: &AppHandle, tab_id: &str, ids: Vec<u64>) {
    deny_stale(app, tab_id, ids);
}

pub(crate) fn navigation_settled_native(app: &AppHandle, tab_id: &str, source: Option<&str>) {
    let ids = app
        .state::<BrowserState>()
        .permissions
        .lock()
        .map(|mut permissions| permissions.navigation_settled(tab_id, source))
        .unwrap_or_default();
    deny_stale(app, tab_id, ids);
}

/// `ContentLoading` for `navigation_id`: a document or an error page
/// committed. Reports the tab's cleared stale mark, if this commit proves it.
pub(crate) fn document_committed_native(app: &AppHandle, tab_id: &str, navigation_id: u64) {
    let state = app.state::<BrowserState>();
    let replaced = state
        .permissions
        .lock()
        .is_ok_and(|mut permissions| permissions.document_committed(tab_id, navigation_id));
    if replaced {
        emit_site_settings(app, &state);
    }
}

pub(crate) async fn resolve_native(
    app: AppHandle,
    webview: Webview,
    tab_id: String,
    id: u64,
    decision: Decision,
) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |_| {
            let _ = sender.send(finish(&app, &tab_id, id, decision, true));
        })
        .map_err(|error| error.to_string())?;
    match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(true) => Ok(()),
        _ => Err("browser permission request unavailable".into()),
    }
}

pub(crate) async fn cancel_tab_native(webview: &Webview, ids: Vec<u64>) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |_| {
            let pending = PENDING.with_borrow_mut(|entries| {
                ids.iter()
                    .filter_map(|id| entries.remove(id))
                    .collect::<Vec<_>>()
            });
            for entries in pending {
                for entry in entries {
                    deny(&entry.args);
                    let _ = unsafe { entry.deferral.Complete() };
                }
            }
            let _ = sender.send(());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())
}
