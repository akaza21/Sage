pub(super) use super::teardown::TeardownQuarantine;
use super::teardown::UNCERTAIN_CLOSE_PREFIX;
use super::*;
use std::future::Future;

mod close_wait;
use close_wait::{uncertain_close, wait_for_native_close};
mod close_tab;
pub(super) use close_tab::close_browser_tab;
mod builder;
pub(super) use builder::register_tab_hooks;
#[cfg(windows)]
pub(super) use builder::{TabEnvironment, open_popup_tab};

fn finish_browser_tab_close(
    downloads: &Mutex<downloads::DownloadRegistry>,
    permissions: &Mutex<site_permissions::SitePermissions>,
    label: &str,
    revoke: impl FnOnce(&str),
) {
    downloads
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .forget_tab(label);
    permissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish_close(label);
    revoke(label);
}

fn complete_native_close<T>(
    tabs: &Mutex<BrowserTabs<T>>,
    quarantine: &TeardownQuarantine,
    label: &str,
    result: Result<(), String>,
    sender: tokio::sync::oneshot::Sender<Result<(), String>>,
    publish: impl FnOnce(&str),
) {
    if result.is_ok()
        && let Ok(mut tabs) = tabs.lock()
    {
        tabs.entries.remove(label);
    }
    quarantine.acknowledge(label, &result);
    let succeeded = result.is_ok();
    if succeeded {
        publish(label);
    }
    let _ = sender.send(result);
}

#[cfg(any(not(windows), test))]
fn allowed_browser_navigation(next: &Url) -> bool {
    admit_url(next.as_str()).is_ok()
}

struct OpenReservation<'a, T> {
    tabs: &'a Mutex<BrowserTabs<T>>,
    label: String,
}

impl<T> Drop for OpenReservation<'_, T> {
    fn drop(&mut self) {
        let mut tabs = self
            .tabs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tabs.opening.remove(&self.label);
    }
}

fn complete_browser_open(
    label: String,
    target: Option<Url>,
    navigate: impl FnOnce(Url) -> Result<(), String>,
) -> Result<String, String> {
    if let Some(target) = target {
        navigate(target)?;
    }
    Ok(label)
}

fn open_with<'a, T>(
    tabs: &'a Mutex<BrowserTabs<T>>,
    clearing_data: &AtomicBool,
    loading: bool,
    create: impl FnOnce(&str) -> Result<T, String>,
) -> Result<(String, OpenReservation<'a, T>), String> {
    let label = {
        let mut tabs = tabs.lock().map_err(|error| error.to_string())?;
        if clearing_data.load(Ordering::Acquire) {
            return Err("browser data clearing is in progress".into());
        }
        if tabs.slot_count() >= MAX_BROWSER_TABS {
            return Err("browser tab limit reached".into());
        }
        if !tabs.closing.is_empty() {
            return Err("Browser is still closing, try again".into());
        }
        if let Some(message) = tabs.teardown.blocked_message() {
            return Err(message.into());
        }
        tabs.reserve_open_label()?
    };
    let reservation = OpenReservation {
        tabs,
        label: label.clone(),
    };
    let created = create(&label);
    let mut tabs = tabs.lock().map_err(|error| error.to_string())?;
    let handle = created?;
    tabs.insert(label.clone(), handle, loading);
    Ok((label, reservation))
}

pub(super) async fn close_with_unlocked<T: Clone, F: Future<Output = Result<(), String>>>(
    tabs: &Mutex<BrowserTabs<T>>,
    label: &str,
    close: impl FnOnce(T) -> F,
) -> Result<(), String> {
    let tab = {
        let mut tabs = tabs.lock().map_err(|error| error.to_string())?;
        let tab = tabs
            .entries
            .remove(label)
            .ok_or("browser tab unavailable")?;
        tabs.closing.insert(label.to_owned());
        tab
    };
    let result = close(tab.handle.clone()).await;
    let mut tabs = tabs.lock().map_err(|error| error.to_string())?;
    tabs.closing.remove(label);
    if result
        .as_ref()
        .err()
        .is_some_and(|error| !error.starts_with(UNCERTAIN_CLOSE_PREFIX))
    {
        tabs.entries.insert(label.to_owned(), tab);
    }
    result
}

async fn hide_or_close<T: Clone, F: Future<Output = Result<(), String>>>(
    tabs: &Mutex<BrowserTabs<T>>,
    label: &str,
    hide: impl FnOnce(&T) -> Result<(), String>,
    close: impl FnOnce(T) -> F,
) -> Result<(), String> {
    let handle = tabs
        .lock()
        .map_err(|error| error.to_string())?
        .handle(label)
        .cloned()
        .ok_or("browser tab unavailable")?;
    if let Err(error) = hide(&handle) {
        let cleanup = close_with_unlocked(tabs, label, close).await;
        return Err(match cleanup {
            Ok(()) => format!("browser child hide failed: {error}"),
            Err(cause) => format!("browser child hide failed: {error}; cleanup failed: {cause}"),
        });
    }
    Ok(())
}

pub(super) async fn close_webview_on_main_thread(
    app: &AppHandle,
    tabs: &Mutex<BrowserTabs<Webview>>,
    webview: Webview,
    notify_tab_closed: bool,
) -> Result<(), String> {
    let quarantine = tabs
        .lock()
        .map_err(|error| error.to_string())?
        .teardown
        .clone();
    let label = webview.label().to_owned();
    quarantine.begin(&label)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let completion_app = app.clone();
    let completion_quarantine = quarantine.clone();
    let completion_label = label.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let result = webview.close().map_err(|error| error.to_string());
        let state = completion_app.state::<BrowserState>();
        complete_native_close(
            &state.tabs,
            &completion_quarantine,
            &completion_label,
            result,
            sender,
            |closed_label| {
                if let Ok(mut popups) = state.popups.lock() {
                    popups.forget_child(closed_label);
                }
                if notify_tab_closed {
                    finish_browser_tab_close(
                        &state.downloads,
                        &state.permissions,
                        closed_label,
                        |id| {
                            completion_app
                                .state::<crate::state::AppState>()
                                .acp
                                .revoke_browser_tab(id);
                        },
                    );
                    let _ = completion_app.emit_to("main", "browser:tab-closed", closed_label);
                }
            },
        );
    }) {
        quarantine.mark_failed(&label);
        return Err(uncertain_close(error));
    }
    let result = wait_for_native_close(receiver, std::time::Duration::from_secs(10)).await;
    if let Err(error) = &result {
        quarantine.mark_failed(&label);
        eprintln!("browser native close failed: {error}");
    }
    result
}

#[tauri::command]
pub(crate) async fn browser_open(
    app: AppHandle,
    state: State<'_, BrowserState>,
    url: Option<String>,
) -> Result<String, String> {
    open_browser_tab(app, &state, url, false).await
}

pub(crate) async fn open_agent_browser_tab(
    app: AppHandle,
    state: State<'_, BrowserState>,
    url: String,
) -> Result<String, String> {
    open_browser_tab(app, &state, Some(url), true).await
}

pub(super) async fn open_browser_tab(
    app: AppHandle,
    state: &BrowserState,
    url: Option<String>,
    agent: bool,
) -> Result<String, String> {
    open_browser_tab_tracked(app, state, url, agent, |_| {}).await
}

pub(super) async fn open_browser_tab_tracked(
    app: AppHandle,
    state: &BrowserState,
    url: Option<String>,
    agent: bool,
    created: impl FnOnce(&str),
) -> Result<String, String> {
    let url = url
        .as_deref()
        .map(admit_url)
        .transpose()
        .map_err(str::to_owned)?;
    let window = app.get_window("main").ok_or("main window unavailable")?;
    let (label, _opening) = open_with(&state.tabs, &state.clearing_data, url.is_some(), |label| {
        builder::create_tab_webview(&app, &window, &state.profile_dir, label, None)
    })?;
    created(&label);
    hide_or_close(
        &state.tabs,
        &label,
        |webview| webview.hide().map_err(|error| error.to_string()),
        |webview| close_webview_on_main_thread(&app, &state.tabs, webview, false),
    )
    .await?;
    let webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .handle(&label)
        .cloned()
        .ok_or("browser tab unavailable")?;
    state
        .downloads
        .lock()
        .map_err(|error| error.to_string())?
        .set_provenance(
            &label,
            if agent {
                downloads::NavigationProvenance::Agent
            } else {
                downloads::NavigationProvenance::User
            },
        );
    let registration = register_tab_hooks(&app, &webview).await;
    let opened = registration.and_then(|()| {
        complete_browser_open(label.clone(), url, |target| {
            webview
                .navigate(target)
                .map_err(|error| format!("browser initial navigation failed: {error}"))
        })
    });
    match opened {
        Ok(label) => Ok(label),
        Err(error) => {
            if let Ok(mut downloads) = state.downloads.lock() {
                downloads.forget_tab(&label);
            }
            let _ = close_with_unlocked(&state.tabs, &label, |webview| {
                close_webview_on_main_thread(&app, &state.tabs, webview, false)
            })
            .await;
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn browser_close(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<(), String> {
    close_browser_tab(&app, &state, tab_id).await
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod open_reply_tests;
