use tauri::{AppHandle, Emitter, Manager, Webview};

use super::BrowserNavigation;
use crate::browser::{BrowserSnapshot, BrowserState};

pub(crate) fn navigation_state(
    webview: &Webview,
    snapshot: &BrowserSnapshot,
) -> Result<BrowserNavigation, String> {
    let tab_id = webview.label().to_owned();
    Ok(BrowserNavigation {
        tab_id,
        url: snapshot.committed_url.clone().unwrap_or_default(),
        title: snapshot.title.clone(),
        favicon: snapshot.favicon.clone(),
        revision: snapshot.revision.to_string(),
        loading: snapshot.loading,
        can_go_back: snapshot.can_go_back,
        can_go_forward: snapshot.can_go_forward,
        zoom_percent: snapshot.zoom_percent,
        device: snapshot.device.confirmed(),
        device_unconfirmed: snapshot.device.is_unconfirmed(),
        error: snapshot.error.map(str::to_owned),
        print_unavailable_reason: if cfg!(windows) {
            None
        } else {
            Some("system print dialog is unavailable on this platform")
        },
        annotate_unavailable_reason: crate::browser::annotate::unavailable_reason(snapshot),
        device_unavailable_reason: crate::browser::device::unavailable_reason(),
    })
}

pub(super) fn emit_snapshot(app: AppHandle, webview: Webview) {
    let snapshot = app
        .state::<BrowserState>()
        .tabs
        .lock()
        .ok()
        .and_then(|tabs| tabs.snapshot(webview.label()));
    let Some(snapshot) = snapshot else {
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(state) = navigation_state(&webview, &snapshot)
            && app.state::<BrowserState>().tabs.lock().is_ok_and(|tabs| {
                tabs.snapshot(webview.label()).is_some_and(|current| {
                    current.revision == snapshot.revision
                        && current.title == snapshot.title
                        && current.favicon == snapshot.favicon
                })
            })
        {
            let _ = app.emit_to("main", "browser:navigation", state);
        }
    });
}
