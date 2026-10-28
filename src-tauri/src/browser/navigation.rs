#[cfg(windows)]
use std::{sync::mpsc, time::Duration};

#[cfg(windows)]
use tauri::Manager;
use tauri::{AppHandle, State, Webview};
#[cfg(windows)]
use windows_core::BOOL;

use super::{BrowserState, admit_url};

pub(crate) mod control;
mod emit;
#[cfg(windows)]
mod favicon;
mod start;
mod status;
#[cfg(windows)]
mod title;
use emit::emit_snapshot;
pub(super) use emit::navigation_state;
pub(crate) use status::BrowserNavigation;
#[cfg(windows)]
use status::emit_or_retry;

#[tauri::command]
pub(crate) async fn browser_status(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<BrowserNavigation, String> {
    status::browser_status(state, tab_id).await
}

#[cfg(windows)]
pub(crate) async fn register_native(app: AppHandle, webview: Webview) -> Result<(), String> {
    use webview2_com::{
        NavigationCompletedEventHandler, NavigationStartingEventHandler, SourceChangedEventHandler,
    };
    use windows_core::PWSTR;

    use super::popups::{INITIAL_BLANK, navigation_admitted};

    let label = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    let event_webview = webview.clone();
    webview
        .with_webview(move |platform| {
            let result: Result<(), String> = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let start_app = app.clone();
                let start_label = label.clone();
                let start_webview = event_webview.clone();
                let starting =
                    NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                        if let Some(args) = args {
                            let mut id = 0;
                            let mut uri = PWSTR::null();
                            unsafe {
                                args.NavigationId(&mut id)?;
                                args.Uri(&mut uri)?;
                            }
                            let uri = webview2_com::CoTaskMemPWSTR::from(uri).to_string();
                            if !navigation_admitted(&start_app, &start_label, &uri) {
                                unsafe {
                                    args.SetCancel(true)?;
                                }
                                return Ok(());
                            }
                            if uri == INITIAL_BLANK {
                                return Ok(()); // opener-linked initial document; not a page load
                            }
                            let browser = start_app.state::<BrowserState>();
                            let (started, denied) =
                                match (browser.tabs.lock(), browser.permissions.lock()) {
                                    (Ok(mut tabs), Ok(mut permissions)) => start::begin(
                                        &mut tabs,
                                        &mut permissions,
                                        &start_label,
                                        id,
                                        &uri,
                                    ),
                                    _ => (false, Vec::new()),
                                };
                            super::site_permissions::deny_navigation_native(
                                &start_app,
                                &start_label,
                                denied,
                            );
                            if started {
                                start_app.state::<BrowserState>().user_navigation_started(
                                    &start_label,
                                    id,
                                    &uri,
                                );
                                start_app
                                    .state::<crate::state::AppState>()
                                    .acp
                                    .browser_navigation_started(&start_label);
                                let clear_webview = start_webview.clone();
                                std::thread::spawn(move || {
                                    let _ = super::devtools::clear_agent_highlight(&clear_webview);
                                });
                                super::annotate::leave(
                                    &start_app,
                                    &start_label,
                                    super::annotate::Exit::Reveal,
                                );
                                emit_snapshot(start_app.clone(), start_webview.clone());
                            }
                        }
                        Ok(())
                    }));
                let completed_app = app.clone();
                let completed_label = label.clone();
                let completed_webview = event_webview.clone();
                let completed =
                    NavigationCompletedEventHandler::create(Box::new(move |core, args| {
                        if let (Some(core), Some(args)) = (core, args) {
                            let mut id = 0;
                            let mut success = BOOL(0);
                            unsafe {
                                args.NavigationId(&mut id)?;
                                args.IsSuccess(&mut success)?;
                            }
                            let url = if success.as_bool() {
                                let mut source = PWSTR::null();
                                unsafe {
                                    core.Source(&mut source)?;
                                }
                                let source = webview2_com::CoTaskMemPWSTR::from(source);
                                Some(source.to_string())
                            } else {
                                None
                            };
                            let settled =
                                completed_app.state::<BrowserState>().tabs.lock().is_ok_and(
                                    |mut tabs| {
                                        tabs.finish_navigation(&completed_label, id, url.as_deref())
                                    },
                                );
                            completed_app
                                .state::<BrowserState>()
                                .user_navigation_finished(
                                    &completed_label,
                                    id,
                                    settled && success.as_bool(),
                                );
                            if settled {
                                let committed_url = completed_app
                                    .state::<BrowserState>()
                                    .tabs
                                    .lock()
                                    .ok()
                                    .and_then(|tabs| {
                                        tabs.entries
                                            .get(&completed_label)
                                            .and_then(|tab| tab.committed_url.clone())
                                    });
                                super::site_permissions::navigation_settled_native(
                                    &completed_app,
                                    &completed_label,
                                    committed_url.as_deref(),
                                );
                                if let Some(url) = url.as_deref() {
                                    title::capture_completed_title(
                                        &completed_app,
                                        &completed_label,
                                        url,
                                        &core,
                                    );
                                    favicon::capture_completed(
                                        &completed_app,
                                        &completed_webview,
                                        &completed_label,
                                        &core,
                                    );
                                }
                                if let Ok((url, revision)) = completed_app
                                    .state::<BrowserState>()
                                    .settled_tab(&completed_label)
                                {
                                    completed_app
                                        .state::<crate::state::AppState>()
                                        .acp
                                        .browser_navigation_settled(
                                            &completed_label,
                                            &url,
                                            revision,
                                        );
                                }
                                emit_or_retry(completed_app.clone(), completed_webview.clone());
                            }
                        }
                        Ok(())
                    }));
                let source_app = app.clone();
                let source_label = label.clone();
                let source_webview = event_webview.clone();
                let source_changed =
                    SourceChangedEventHandler::create(Box::new(move |core, args| {
                        if let (Some(core), Some(args)) = (core, args) {
                            let mut new_document = BOOL(0);
                            unsafe {
                                args.IsNewDocument(&mut new_document)?;
                            }
                            if !new_document.as_bool() {
                                let mut source = PWSTR::null();
                                unsafe {
                                    core.Source(&mut source)?;
                                }
                                let source = webview2_com::CoTaskMemPWSTR::from(source);
                                if source_app.state::<BrowserState>().tabs.lock().is_ok_and(
                                    |mut tabs| {
                                        tabs.source_changed(&source_label, &source.to_string())
                                    },
                                ) {
                                    if let Ok((url, revision)) = source_app
                                        .state::<BrowserState>()
                                        .settled_tab(&source_label)
                                    {
                                        source_app
                                            .state::<crate::state::AppState>()
                                            .acp
                                            .browser_navigation_settled(
                                                &source_label,
                                                &url,
                                                revision,
                                            );
                                    }
                                    emit_or_retry(source_app.clone(), source_webview.clone());
                                }
                            }
                        }
                        Ok(())
                    }));
                let mut start_token = 0;
                unsafe { core.add_NavigationStarting(&starting, &mut start_token) }
                    .map_err(|error| error.to_string())?;
                let mut complete_token = 0;
                if let Err(error) =
                    unsafe { core.add_NavigationCompleted(&completed, &mut complete_token) }
                {
                    let _ = unsafe { core.remove_NavigationStarting(start_token) };
                    return Err(error.to_string());
                }
                let mut source_token = 0;
                if let Err(error) =
                    unsafe { core.add_SourceChanged(&source_changed, &mut source_token) }
                {
                    let _ = unsafe { core.remove_NavigationCompleted(complete_token) };
                    let _ = unsafe { core.remove_NavigationStarting(start_token) };
                    return Err(error.to_string());
                }
                if let Err(error) =
                    title::register(app.clone(), event_webview.clone(), label.clone(), &core)
                {
                    let _ = unsafe { core.remove_SourceChanged(source_token) };
                    let _ = unsafe { core.remove_NavigationCompleted(complete_token) };
                    let _ = unsafe { core.remove_NavigationStarting(start_token) };
                    return Err(error.to_string());
                }
                favicon::register(app.clone(), event_webview.clone(), label.clone(), &core);
                Ok(())
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
pub(crate) async fn register_native(_app: AppHandle, _webview: Webview) -> Result<(), String> {
    Err("native browser navigation is unavailable on this platform".into())
}
