use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;
#[cfg(windows)]
use tauri::Emitter;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, State, WebviewBuilder, WebviewUrl};
use url::Url;

use super::{
    BrowserState,
    lifecycle::{close_browser_tab, close_webview_on_main_thread},
};

#[cfg(windows)]
mod native;
mod start;

use start::{ClearStart, start_clear};

#[cfg(windows)]
fn wait_for_pending_clear(
    receiver: std::sync::mpsc::Receiver<Result<(), String>>,
    timeout: std::time::Duration,
) -> Result<(), String> {
    receiver
        .recv_timeout(timeout)
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => "browser data clear timed out".to_owned(),
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                "browser data clear callback disconnected".to_owned()
            }
        })?
}

static NEXT_CLEAR_LABEL: AtomicU64 = AtomicU64::new(1);

struct ClearGate<'a> {
    flag: &'a AtomicBool,
    armed: bool,
}

impl ClearGate<'_> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ClearGate<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(false, Ordering::Release);
        }
    }
}

pub(super) fn begin_clear<T>(
    tabs: &std::sync::Mutex<super::BrowserTabs<T>>,
    clearing_data: &AtomicBool,
) -> Result<Vec<String>, String> {
    let tabs = tabs.lock().map_err(|error| error.to_string())?;
    if !tabs.opening.is_empty() || !tabs.closing.is_empty() {
        return Err("browser tabs are still opening or closing".into());
    }
    if let Some(message) = tabs.teardown.blocked_message() {
        return Err(message.into());
    }
    if clearing_data.swap(true, Ordering::AcqRel) {
        return Err("browser data clearing is already in progress".into());
    }
    Ok(tabs.entries.keys().cloned().collect())
}

#[cfg(windows)]
fn finish_pending_clear(
    flag: &AtomicBool,
    result: Result<(), String>,
    publish: impl FnOnce(&ClearBrowsingDataCompletion),
) -> Result<(), String> {
    publish(&ClearBrowsingDataCompletion {
        cleared: result.is_ok(),
        error: result.as_ref().err().cloned(),
    });
    flag.store(false, Ordering::Release);
    result
}

#[cfg(windows)]
async fn complete_pending_clear<F: std::future::Future<Output = Result<(), String>>>(
    flag: &AtomicBool,
    result: Result<(), String>,
    close: F,
    publish: impl FnOnce(&ClearBrowsingDataCompletion),
) -> Result<(), String> {
    let close_result = close.await;
    let outcome = match close_result {
        Ok(()) => result,
        Err(error) => Err(error),
    };
    finish_pending_clear(flag, outcome, publish)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClearBrowsingDataCompletion {
    cleared: bool,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClearBrowsingDataResult {
    closed_tab_ids: Vec<String>,
    cleared: bool,
    pending: bool,
    error: Option<String>,
}

#[tauri::command]
pub(crate) async fn browser_clear_data(
    app: AppHandle,
    state: State<'_, BrowserState>,
) -> Result<ClearBrowsingDataResult, String> {
    let tab_ids = match start_clear(&state.tabs, &state.clearing_data)? {
        ClearStart::Refused(result) => return Ok(result),
        ClearStart::Begun(tab_ids) => tab_ids,
    };
    #[cfg_attr(not(windows), allow(unused_mut, unused_variables))]
    let mut gate = ClearGate {
        flag: &state.clearing_data,
        armed: true,
    };
    for tab_id in &tab_ids {
        app.state::<crate::state::AppState>()
            .acp
            .revoke_browser_tab(tab_id);
    }
    let mut result = ClearBrowsingDataResult {
        closed_tab_ids: Vec::new(),
        cleared: false,
        pending: false,
        error: None,
    };
    let Some(window) = app.get_window("main") else {
        result.error = Some("main window unavailable".into());
        return Ok(result);
    };
    let label = format!(
        "browser-data-clear-{}",
        NEXT_CLEAR_LABEL.fetch_add(1, Ordering::Relaxed)
    );
    let inert_url = Url::parse("about:blank").map_err(|error| error.to_string())?;
    let builder = WebviewBuilder::new(label, WebviewUrl::External(inert_url))
        .data_directory(state.profile_dir.clone())
        .on_navigation(|_| false);
    let temporary = match window.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(1.0, 1.0),
    ) {
        Ok(webview) => webview,
        Err(error) => {
            result.error = Some(error.to_string());
            return Ok(result);
        }
    };
    if let Err(error) = temporary.hide() {
        result.error = Some(
            match close_webview_on_main_thread(&app, &state.tabs, temporary, false).await {
                Ok(()) => error.to_string(),
                Err(cause) => format!("browser child hide failed: {error}; {cause}"),
            },
        );
        return Ok(result);
    }
    for tab_id in tab_ids {
        match close_browser_tab(&app, &state, tab_id.clone()).await {
            Ok(()) => result.closed_tab_ids.push(tab_id),
            Err(error) => result.error = Some(error),
        }
    }
    if result.error.is_none() {
        #[cfg(windows)]
        let clear_webview = temporary.clone();
        #[cfg(windows)]
        let native_clear =
            tauri::async_runtime::spawn_blocking(move || native::clear_profile(&clear_webview))
                .await
                .map_err(|error| error.to_string())
                .and_then(|outcome| outcome);
        #[cfg(windows)]
        match native_clear {
            Ok(native::ClearProfileOutcome::Completed(Ok(()))) => result.cleared = true,
            Ok(native::ClearProfileOutcome::Completed(Err(error))) | Err(error) => {
                result.error = Some(error)
            }
            Ok(native::ClearProfileOutcome::Pending(receiver)) => {
                result.pending = true;
                result.error = Some(
                    "Browser data is still clearing; new browser tabs are blocked until it finishes."
                        .into(),
                );
                let completion_app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let completion_state = completion_app.state::<BrowserState>();
                    let completion = tauri::async_runtime::spawn_blocking(move || {
                        wait_for_pending_clear(receiver, std::time::Duration::from_secs(60))
                    })
                    .await;
                    let result = completion
                        .map_err(|error| error.to_string())
                        .and_then(|callback| callback);
                    if let Err(error) = complete_pending_clear(
                        &completion_state.clearing_data,
                        result,
                        close_webview_on_main_thread(
                            &completion_app,
                            &completion_state.tabs,
                            temporary,
                            false,
                        ),
                        |final_result| {
                            if let Err(error) = completion_app.emit_to(
                                "main",
                                "browser:clear-data-complete",
                                final_result,
                            ) {
                                eprintln!("browser data clear completion event failed: {error}");
                            }
                        },
                    )
                    .await
                    {
                        eprintln!("browser data clear failed: {error}");
                    }
                });
                gate.disarm();
                return Ok(result);
            }
        }
        #[cfg(not(windows))]
        {
            // Unreached: `start_clear` refuses first on these platforms.
            result.error = Some(start::UNSUPPORTED_PLATFORM.into());
        }
    }
    if let Err(error) = close_webview_on_main_thread(&app, &state.tabs, temporary, false).await {
        result.error = Some(error);
        result.cleared = false;
    }
    Ok(result)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn pending_callback_has_a_deadline() {
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _ = sender.send(Ok(()));
        });
        let result = wait_for_pending_clear(receiver, std::time::Duration::ZERO);
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[tokio::test]
    async fn callback_timeout_closes_temporary_view_and_publishes_failure() {
        let (_sender, receiver) = mpsc::sync_channel(1);
        let timeout = wait_for_pending_clear(receiver, std::time::Duration::ZERO);
        let clearing_data = AtomicBool::new(true);
        let mut closed = false;
        let mut published = None;
        let result = complete_pending_clear(
            &clearing_data,
            timeout,
            async {
                closed = true;
                Ok(())
            },
            |completion| published = Some((completion.cleared, completion.error.clone())),
        )
        .await;
        assert!(closed);
        assert_eq!(result, Err("browser data clear timed out".into()));
        assert_eq!(
            published,
            Some((false, Some("browser data clear timed out".into())))
        );
        assert!(!clearing_data.load(Ordering::Acquire));
    }

    #[test]
    fn disconnected_callback_fails_clear_and_releases_gate() {
        let (sender, receiver) = mpsc::sync_channel::<Result<(), String>>(1);
        drop(sender);
        let clearing_data = AtomicBool::new(true);
        let result = finish_pending_clear(
            &clearing_data,
            receiver
                .recv()
                .map_err(|error| error.to_string())
                .and_then(|result| result),
            |_| {},
        );
        assert!(result.is_err());
        assert!(!clearing_data.load(Ordering::Acquire));
    }

    #[test]
    fn waiter_failure_releases_clear_gate() {
        let clearing_data = AtomicBool::new(true);
        let result = finish_pending_clear(&clearing_data, Err("waiter failed".into()), |_| {});
        assert_eq!(result, Err("waiter failed".into()));
        assert!(!clearing_data.load(Ordering::Acquire));
    }

    #[test]
    fn pending_clear_publishes_final_success_and_failure() {
        for outcome in [Ok(()), Err("WebView2 clearing failed".into())] {
            let clearing_data = AtomicBool::new(true);
            let mut published = None;
            let actual = finish_pending_clear(&clearing_data, outcome.clone(), |completion| {
                published = Some((completion.cleared, completion.error.clone()));
            });
            assert_eq!(actual, outcome);
            let completion = published.expect("completion published");
            assert_eq!(completion.0, outcome.is_ok());
            assert_eq!(completion.1, outcome.err());
            assert!(!clearing_data.load(Ordering::Acquire));
        }
    }

    #[test]
    fn pending_clear_keeps_reopen_blocked_until_final_outcome_is_published() {
        let clearing_data = AtomicBool::new(true);
        finish_pending_clear(&clearing_data, Ok(()), |_| {
            assert!(clearing_data.load(Ordering::Acquire));
        })
        .unwrap();
        assert!(!clearing_data.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn clear_waits_for_native_tab_teardown_acknowledgment() {
        let tabs = std::sync::Mutex::new(super::super::BrowserTabs::<usize>::new());
        tabs.lock().unwrap().insert("browser-1".into(), 1, false);
        let clearing_data = AtomicBool::new(false);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let close = super::super::lifecycle::close_with_unlocked(&tabs, "browser-1", |_| async {
            receiver.await.map_err(|error| error.to_string())
        });
        tokio::pin!(close);
        assert!(
            tokio::time::timeout(std::time::Duration::ZERO, &mut close)
                .await
                .is_err()
        );
        assert!(begin_clear(&tabs, &clearing_data).is_err());
        sender.send(()).unwrap();
        close.await.unwrap();
        assert!(begin_clear(&tabs, &clearing_data).is_ok());
    }
}
