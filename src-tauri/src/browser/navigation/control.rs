//! Back, forward, reload and stop for a browser tab, and the Annotate
//! actions, which answer with a value.

#[cfg(windows)]
use std::{sync::mpsc, time::Duration};

use tauri::{AppHandle, State, Webview};
#[cfg(windows)]
use windows_core::BOOL;

use super::super::{BrowserState, admit_url};

#[tauri::command]
pub(crate) async fn browser_control(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    action: String,
    comment: Option<String>,
    ordinal: Option<u8>,
) -> Result<Option<serde_json::Value>, String> {
    if let Some(action) = super::super::annotate::action(&action) {
        return super::super::annotate::set(app, tab_id, action, comment, ordinal).await;
    }
    history_control(state, tab_id, action).await.map(|()| None)
}

async fn history_control(
    state: State<'_, BrowserState>,
    tab_id: String,
    action: String,
) -> Result<(), String> {
    if !matches!(action.as_str(), "back" | "forward" | "reload" | "stop") {
        return Err("unknown browser action".into());
    }
    let (webview, target, native_aligned) = {
        let mut tabs = state.tabs.lock().map_err(|error| error.to_string())?;
        let webview = tabs
            .handle(&tab_id)
            .cloned()
            .ok_or("browser tab unavailable")?;
        let target = match action.as_str() {
            "back" => Some(tabs.traverse(&tab_id, -1)?),
            "forward" => Some(tabs.traverse(&tab_id, 1)?),
            _ => None,
        };
        let native_aligned = tabs.native_history_aligned(&tab_id);
        (webview, target, native_aligned)
    };
    if let Some(target) = target {
        if native_aligned {
            match native_history_action(webview.clone(), action.clone()).await {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    state
                        .tabs
                        .lock()
                        .map_err(|error| error.to_string())?
                        .mark_native_history_diverged(&tab_id);
                }
                Err(error) => {
                    cancel_traversal(state.inner(), &tab_id)?;
                    return Err(error);
                }
            }
        } else {
            state
                .tabs
                .lock()
                .map_err(|error| error.to_string())?
                .mark_native_history_diverged(&tab_id);
        }
        let result = admit_url(&target)
            .map_err(str::to_owned)
            .and_then(|url| webview.navigate(url).map_err(|error| error.to_string()));
        if result.is_err() {
            cancel_traversal(state.inner(), &tab_id)?;
        }
        return result;
    }
    if action == "reload" {
        return webview.reload().map_err(|error| error.to_string());
    }
    cancel_traversal(state.inner(), &tab_id)?;
    native_stop(webview).await
}

fn cancel_traversal(state: &BrowserState, tab_id: &str) -> Result<(), String> {
    let mut tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    tabs.cancel_traversal(tab_id);
    Ok(())
}

#[cfg(windows)]
async fn native_history_action(webview: Webview, action: String) -> Result<bool, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result: Result<bool, String> = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let mut available = BOOL(0);
                unsafe {
                    if action == "back" {
                        core.CanGoBack(&mut available)
                    } else {
                        core.CanGoForward(&mut available)
                    }
                }
                .map_err(|error| error.to_string())?;
                if available.as_bool() {
                    unsafe {
                        if action == "back" {
                            core.GoBack()
                        } else {
                            core.GoForward()
                        }
                    }
                    .map_err(|error| error.to_string())?;
                }
                Ok(available.as_bool())
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(windows))]
async fn native_history_action(_webview: Webview, _action: String) -> Result<bool, String> {
    Err("native browser navigation is unavailable on this platform".into())
}

#[cfg(windows)]
pub(in crate::browser) async fn native_stop(webview: Webview) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = unsafe {
                platform
                    .controller()
                    .CoreWebView2()
                    .and_then(|core| core.Stop())
            };
            let _ = sender.send(result.map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(windows))]
async fn native_stop(_webview: Webview) -> Result<(), String> {
    Err("native browser navigation is unavailable on this platform".into())
}
