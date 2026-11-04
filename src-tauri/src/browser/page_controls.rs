use tauri::{AppHandle, State, Webview};

use tauri::Manager;

#[cfg(windows)]
use std::{sync::mpsc, time::Duration};

use super::{BrowserState, BrowserTabs, admit_url};

fn dispatch_user_print<T, R>(
    tabs: &BrowserTabs<T>,
    tab_id: &str,
    show_dialog: impl FnOnce(&T) -> Result<R, String>,
) -> Result<R, String> {
    tabs.openable_url(tab_id)?;
    let handle = tabs.handle(tab_id).ok_or("browser tab unavailable")?;
    show_dialog(handle)
}

#[tauri::command]
pub(crate) async fn browser_print(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<(), String> {
    let (webview, revision) = {
        let tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        dispatch_user_print(&tabs, &tab_id, |webview| {
            let revision = tabs
                .snapshot(&tab_id)
                .ok_or("browser tab unavailable")?
                .revision;
            Ok((webview.clone(), revision))
        })?
    };
    native_print(webview, revision).await
}

#[cfg(windows)]
async fn native_print(webview: Webview, expected_revision: u64) -> Result<(), String> {
    use webview2_com_sys::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM, ICoreWebView2_16,
    };
    use windows_core::Interface;

    let (sender, receiver) = mpsc::sync_channel(1);
    let app = webview.app_handle().clone();
    let tab_id = webview.label().to_owned();
    webview
        .with_webview(move |platform| {
            let result: Result<(), String> = (|| {
                {
                    let state = app.state::<BrowserState>();
                    let tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
                    if tabs
                        .snapshot(&tab_id)
                        .is_none_or(|snapshot| snapshot.revision != expected_revision)
                        || tabs.openable_url(&tab_id).is_err()
                    {
                        return Err("browser page changed; retry print".into());
                    }
                }
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let printer = core
                    .cast::<ICoreWebView2_16>()
                    .map_err(|_| "system print dialog is unavailable in this WebView".to_owned())?;
                unsafe { printer.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM) }
                    .map_err(|error| error.to_string())
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "system print dialog did not open".to_owned())?
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(windows))]
async fn native_print(_webview: Webview, _expected_revision: u64) -> Result<(), String> {
    Err("system print dialog is unavailable on this platform".into())
}

fn active_webview(state: &State<'_, BrowserState>, tab_id: &str) -> Result<Webview, String> {
    state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .handle(tab_id)
        .cloned()
        .ok_or_else(|| "browser tab unavailable".into())
}

#[tauri::command]
pub(crate) async fn browser_hide(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    cover: Option<bool>,
) -> Result<(), String> {
    let webview = active_webview(&state, &tab_id)?;
    let visibility = state
        .page_visibility_for_tab(&tab_id)
        .ok_or("browser tab unavailable")?;
    // A toolbar popover only covers the page for a moment, so Annotate stays
    // on; any other hide ends it.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.clone()
        .run_on_main_thread(move || {
            visibility.hide();
            if cover != Some(true) {
                visibility.sleep();
                app.state::<crate::state::AppState>().acp.suspend_browser_tab(&tab_id);
            }
            let state = app.state::<BrowserState>();
            if let Some(exit) = state.annotate.hiding(&tab_id, cover == Some(true)) {
                super::annotate::leave(&app, &tab_id, exit);
            }
            let _ = sender.send(webview.hide().map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "browser hide unavailable".to_owned())?
}

#[tauri::command]
pub(crate) async fn browser_navigate(
    state: State<'_, BrowserState>,
    tab_id: String,
    url: String,
) -> Result<(), String> {
    let url = admit_url(&url).map_err(str::to_owned)?;
    let webview = active_webview(&state, &tab_id)?;
    state
        .downloads
        .lock()
        .map_err(|error| error.to_string())?
        .request_user_navigation(&tab_id, url.as_str());
    if let Err(error) = webview.navigate(url) {
        if let Ok(mut downloads) = state.downloads.lock() {
            downloads.cancel_user_navigation(&tab_id);
        }
        return Err(error.to_string());
    }
    Ok(())
}

fn admitted_zoom(percent: u16) -> Result<f64, String> {
    if !(50..=300).contains(&percent) || !percent.is_multiple_of(25) {
        return Err("browser zoom must be 50–300% in 25% steps".into());
    }
    Ok(f64::from(percent) / 100.0)
}

#[tauri::command]
pub(crate) async fn browser_zoom(
    state: State<'_, BrowserState>,
    tab_id: String,
    percent: u16,
) -> Result<(), String> {
    let scale = admitted_zoom(percent)?;
    active_webview(&state, &tab_id)?
        .set_zoom(scale)
        .map_err(|error| error.to_string())?;
    let updated = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .set_zoom_percent(&tab_id, percent);
    if updated {
        Ok(())
    } else {
        Err("browser tab unavailable".into())
    }
}

#[tauri::command]
pub(crate) async fn browser_open_external(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<(), String> {
    let url = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .openable_url(&tab_id)?;
    let url = admit_url(&url).map_err(str::to_owned)?;
    open_external(url.as_str())
}

#[tauri::command]
pub(crate) async fn browser_find(
    state: State<'_, BrowserState>,
    tab_id: String,
    action: String,
    query: Option<String>,
) -> Result<(), String> {
    validate_find(&action, query.as_deref())?;
    native_find(active_webview(&state, &tab_id)?, action, query).await
}

fn validate_find(action: &str, query: Option<&str>) -> Result<(), String> {
    if !matches!(action, "start" | "next" | "previous" | "stop") {
        return Err("unknown browser find action".into());
    }
    if action == "start" {
        let text = query.ok_or("find text is required")?;
        if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
            return Err("invalid browser find text".into());
        }
    } else if query.is_some() {
        return Err("find text is only accepted when starting a search".into());
    }
    Ok(())
}

#[cfg(windows)]
async fn native_find(
    webview: Webview,
    action: String,
    query: Option<String>,
) -> Result<(), String> {
    use webview2_com_sys::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_2, ICoreWebView2_28, ICoreWebView2Environment15,
    };
    use windows_core::{Interface, PCWSTR};

    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result: Result<(), String> = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let find = core
                    .cast::<ICoreWebView2_28>()
                    .and_then(|core| unsafe { core.Find() })
                    .map_err(|error| error.to_string())?;
                let result = match action.as_str() {
                    "next" => unsafe { find.FindNext() },
                    "previous" => unsafe { find.FindPrevious() },
                    "stop" => unsafe { find.Stop() },
                    _ => {
                        let environment = core
                            .cast::<ICoreWebView2_2>()
                            .and_then(|core| unsafe { core.Environment() })
                            .and_then(|environment| {
                                environment.cast::<ICoreWebView2Environment15>()
                            })
                            .map_err(|error| error.to_string())?;
                        let options = unsafe { environment.CreateFindOptions() }
                            .map_err(|error| error.to_string())?;
                        let text = query.expect("validated start query");
                        let text = format!("{text}\0").encode_utf16().collect::<Vec<_>>();
                        unsafe {
                            options
                                .SetFindTerm(PCWSTR(text.as_ptr()))
                                .map_err(|error| error.to_string())?;
                            options
                                .SetShouldHighlightAllMatches(true)
                                .map_err(|error| error.to_string())?;
                            find.Start(&options, None)
                        }
                    }
                };
                result.map_err(|error| error.to_string())
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
async fn native_find(
    _webview: Webview,
    _action: String,
    _query: Option<String>,
) -> Result<(), String> {
    Err("native browser find is unavailable on this platform".into())
}

#[cfg(windows)]
fn open_external(url: &str) -> Result<(), String> {
    use windows::{Win32::UI::Shell::ShellExecuteW, core::PCWSTR};

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = format!("{url}\0").encode_utf16().collect::<Vec<_>>();
    // SAFETY: both buffers are NUL-terminated and live through the synchronous call.
    // The admitted URL is a ShellExecute target, never a shell command or argument.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err("could not open browser URL externally".into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_external(_url: &str) -> Result<(), String> {
    Err("external browser open is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::{admitted_zoom, validate_find};
    use crate::browser::BrowserTabs;

    #[test]
    fn print_only_dispatches_for_a_committed_loaded_page() {
        let mut tabs = BrowserTabs::<usize>::new();
        tabs.insert("browser-1".into(), 1, false);
        let mut shown = 0;
        assert!(
            super::dispatch_user_print(&tabs, "browser-1", |_| {
                shown += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(shown, 0);
        assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
        assert!(
            super::dispatch_user_print(&tabs, "browser-1", |_| {
                shown += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(shown, 0);
        assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
        super::dispatch_user_print(&tabs, "browser-1", |_| {
            shown += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(shown, 1);
    }

    #[test]
    fn zoom_is_bounded_to_supported_steps() {
        assert_eq!(admitted_zoom(50).unwrap(), 0.5);
        assert_eq!(admitted_zoom(100).unwrap(), 1.0);
        assert_eq!(admitted_zoom(200).unwrap(), 2.0);
        assert_eq!(admitted_zoom(300).unwrap(), 3.0);
        for invalid in [0, 49, 51, 101, 301, u16::MAX] {
            assert!(admitted_zoom(invalid).is_err());
        }
    }

    #[test]
    fn find_requires_one_bounded_term_only_for_start() {
        assert!(validate_find("start", Some("example")).is_ok());
        assert!(validate_find("next", None).is_ok());
        assert!(validate_find("previous", None).is_ok());
        assert!(validate_find("stop", None).is_ok());
        for (action, query) in [
            ("start", None),
            ("start", Some("")),
            ("start", Some("one\ntwo")),
            (
                "start",
                Some(
                    "long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated long term repeated",
                ),
            ),
            ("next", Some("term")),
            ("unknown", None),
        ] {
            assert!(validate_find(action, query).is_err());
        }
    }
}
