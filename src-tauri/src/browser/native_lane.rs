//! Test-only seam for the WebView2 native lane (`--features native-lane`,
//! Windows debug builds only; see `tests/native_lane`). It boots Sage's real
//! browser state, invoke guard and capabilities behind a hidden `main` window
//! in a disposable profile, and hands the lane a blocking driver that runs on
//! its own thread while Tauri owns the main thread. The driver adds no
//! behaviour: each call is the production function a command, the chrome or
//! the Agent reaches.

mod annotate;
mod diagnostics;

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

use super::{BrowserState, devtools, lifecycle, page_controls, popups};
use crate::{
    acp::browser::{
        BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult, BrowserScope, dispatch,
    },
    launch_profile::{PROFILE_MARKER_BYTES, PROFILE_MARKER_NAME, resolve_launch_profile},
    persistence::Database,
    settings_store::SettingsStore,
    state::AppState,
};

/// The lane's blocking driver. Never call it from Tauri's main thread.
pub struct Lane {
    app: AppHandle,
}

/// What Sage's own tab state says about one browser tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStatus {
    pub loading: bool,
    pub committed_url: Option<String>,
    pub error: Option<String>,
}

/// A popup that Sage hosted natively (S1), linked to its opener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePopup {
    pub id: u64,
    pub opener: String,
    pub child: String,
    pub url: String,
}

/// The production Agent command layer, `DevtoolsCapability<NativePort>`, for
/// one tab, driven through `acp::browser::dispatch`.
pub struct AgentTab {
    capability: devtools::NativeDevtools,
    scope: BrowserScope,
}

impl AgentTab {
    pub fn perform(&mut self, command: BrowserCommand) -> Result<BrowserResult, BrowserFailure> {
        let request = BrowserRequest {
            scope: self.scope.clone(),
            command,
        };
        dispatch(&mut self.capability, request).map(|response| response.result)
    }
}

/// The hidden main window's WebView2 user data folder.
struct MainWebviewDir(PathBuf);

/// Boots Sage on `root`, an empty directory the caller owns and removes, runs
/// the preflight and then `lane` on a worker thread once the event loop is
/// ready, closes every browser tab and the window, and exits. Returns the
/// lane's exit code, or 2 when the preflight fails and no case runs.
pub fn run(root: &Path, lane: impl FnOnce(&Lane) -> i32 + Send + 'static) -> Result<i32, String> {
    std::fs::write(root.join(PROFILE_MARKER_NAME), PROFILE_MARKER_BYTES)
        .map_err(|error| format!("profile marker: {error}"))?;
    diagnostics::install_logger();
    let root = root.to_owned();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| boot(app, &root))
        .invoke_handler(crate::ipc_commands::handle_main_invoke)
        .build(tauri::generate_context!())
        .map_err(|error| format!("Tauri build: {error}"))?;
    let code = Arc::new(AtomicI32::new(1));
    let finished = code.clone();
    let mut lane = Some(lane);
    app.run_return(move |handle, event| {
        if matches!(event, RunEvent::Ready)
            && let Some(lane) = lane.take()
        {
            let handle = handle.clone();
            let finished = finished.clone();
            std::thread::spawn(move || {
                let driver = Lane {
                    app: handle.clone(),
                };
                let mut result = match driver.preflight() {
                    Ok(()) => {
                        catch_unwind(AssertUnwindSafe(|| lane(&driver))).unwrap_or_else(|_| {
                            eprintln!("native-lane: the lane panicked");
                            101
                        })
                    }
                    Err(failure) => {
                        println!("[native-lane] PREFLIGHT FAIL: {failure}");
                        println!("[native-lane] RESULT: FAIL (preflight; no case ran)");
                        2
                    }
                };
                if let Err(error) = driver.close_all() {
                    eprintln!("native-lane: tabs left open at exit: {error}");
                    result = result.max(1);
                }
                finished.store(result, Ordering::SeqCst);
                // Tauri keeps its runtime alive after `run_return`, so the
                // window is destroyed explicitly to release WebView2 and its
                // processes before the caller removes the profile.
                if let Some(main) = handle.get_window("main")
                    && let Err(error) = main.destroy()
                {
                    eprintln!("native-lane: main window destroy failed: {error}");
                }
                handle.exit(result);
            });
        }
        crate::app_run_events::handle_run_event(handle, event);
    });
    Ok(code.load(Ordering::SeqCst))
}

/// The production setup, minus harness discovery and the workbench UI: a
/// disposable profile, the real Browser/App state and a hidden `main`. The
/// stores stay in memory, because Tauri keeps managed state alive until the
/// process exits and Windows cannot delete an open database.
fn boot(app: &mut tauri::App, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let profile = resolve_launch_profile(
        Some(root.as_os_str()),
        None,
        app.path().app_data_dir()?,
        app.path().app_local_data_dir()?,
    )?;
    let database = Database::open(Path::new(":memory:"))?;
    let browser = BrowserState::new(&profile, app.path().app_local_data_dir()?, &[], 1)
        .map_err(std::io::Error::other)?;
    let settings = SettingsStore::open(Path::new(":memory:"))?;
    browser
        .attach_site_settings(settings.clone())
        .map_err(std::io::Error::other)?;
    app.manage(browser);
    app.manage(AppState::try_new_desktop(
        database,
        settings,
        app.handle().clone(),
    )?);
    app.manage(crate::voice_input::VoiceInputState::default());
    let webview_dir = profile
        .webview_dir()
        .ok_or("the disposable profile has no webview directory")?
        .to_owned();
    WebviewWindowBuilder::new(app, "main", WebviewUrl::External("about:blank".parse()?))
        .visible(false)
        .data_directory(webview_dir.clone())
        .build()?;
    app.manage(MainWebviewDir(webview_dir));
    Ok(())
}

impl Lane {
    fn state(&self) -> State<'_, BrowserState> {
        self.app.state::<BrowserState>()
    }

    /// Runtime, session, desktop and main-webview evidence; `Err` when no
    /// case could run (locked workstation, no WebView2).
    fn preflight(&self) -> Result<(), String> {
        let main = self.app.state::<MainWebviewDir>();
        let state = self.state();
        diagnostics::preflight(
            &self.app,
            &[
                ("main window", &main.0),
                ("browser tabs", &state.profile_dir),
            ],
        )
    }

    /// `browser_open` as the chrome reaches it, or (`agent`) as the Agent's
    /// OpenTab does. A failure also says which webviews Tauri still holds
    /// and whether the runtime still runs closures on each, so a dropped
    /// child is attributable.
    pub fn open_tab(&self, url: Option<&str>, agent: bool) -> Result<String, String> {
        tauri::async_runtime::block_on(lifecycle::open_browser_tab(
            self.app.clone(),
            &self.state(),
            url.map(str::to_owned),
            agent,
        ))
        .map_err(|error| {
            format!(
                "{error}; Sage tabs {:?}; {}",
                self.tabs(),
                diagnostics::webviews(&self.app)
            )
        })
    }

    /// The chrome's `browser_navigate`.
    pub fn navigate(&self, tab: &str, url: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(page_controls::browser_navigate(
            self.state(),
            tab.to_owned(),
            url.to_owned(),
        ))
    }

    /// The chrome's `browser_close`.
    pub fn close_tab(&self, tab: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(lifecycle::close_browser_tab(
            &self.app,
            &self.state(),
            tab.to_owned(),
        ))
    }

    /// Closes every live tab, openers first so their popups go with them.
    pub fn close_all(&self) -> Result<(), String> {
        for _ in 0..2 * super::MAX_BROWSER_TABS {
            let Some(tab) = self.tabs().into_iter().next() else {
                return Ok(());
            };
            self.close_tab(&tab)
                .map_err(|error| format!("{tab}: {error}"))?;
        }
        Err(format!("still open: {:?}", self.tabs()))
    }

    /// Live tab labels, as `browser_live_tabs` reports them.
    pub fn tabs(&self) -> Vec<String> {
        self.state()
            .tabs
            .lock()
            .map(|tabs| tabs.entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn status(&self, tab: &str) -> Option<TabStatus> {
        let snapshot = self.state().tabs.lock().ok()?.snapshot(tab)?;
        Some(TabStatus {
            loading: snapshot.loading,
            committed_url: snapshot.committed_url,
            error: snapshot.error.map(str::to_owned),
        })
    }

    /// The tab's bounded native navigation trace (ids, flags, WebView2
    /// statuses; no URLs), for the lane's evidence.
    pub fn navigation_trace(&self, tab: &str) -> Vec<String> {
        self.state()
            .tabs
            .lock()
            .map(|tabs| tabs.navigation_trace(tab))
            .unwrap_or_default()
    }

    /// The chrome's Stop (`CoreWebView2::Stop`).
    pub fn stop(&self, tab: &str) -> Result<(), String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        tauri::async_runtime::block_on(super::navigation::control::native_stop(webview))
    }

    /// The URL the tab has settled on, as the Agent's attachment reads it.
    pub fn settled_url(&self, tab: &str) -> Option<String> {
        self.state().settled_tab(tab).ok().map(|(url, _)| url)
    }

    /// A raw CDP call on the tab: the lane's own observation, outside any
    /// Agent permit.
    pub fn cdp(&self, tab: &str, method: &str, params: &Value) -> Result<Value, String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let raw = devtools::lane_call(
            &webview,
            method,
            &params.to_string(),
            Duration::from_secs(10),
        )
        .map_err(|failure| format!("{method}: {failure:?}"))?;
        serde_json::from_str(&raw).map_err(|error| format!("{method}: {error}"))
    }

    /// Evaluates `expression` in the page's main world, as the page itself
    /// would; `user_gesture` gives it the activation a click would.
    pub fn evaluate(
        &self,
        tab: &str,
        expression: &str,
        user_gesture: bool,
    ) -> Result<Value, String> {
        let params = json!({
            "expression": expression,
            "returnByValue": true,
            "userGesture": user_gesture,
        });
        let response = self.cdp(tab, "Runtime.evaluate", &params)?;
        if let Some(details) = response.get("exceptionDetails") {
            return Err(format!("page exception: {details}"));
        }
        Ok(response["result"]["value"].clone())
    }

    /// The Agent's capability for `tab`, attached as the Agent path attaches it.
    pub fn agent(&self, tab: &str) -> Result<AgentTab, String> {
        let tab_id = tab
            .strip_prefix("browser-")
            .and_then(|number| number.parse().ok())
            .ok_or("not a browser tab label")?;
        let scope = BrowserScope {
            session_id: "native-lane".into(),
            tab_id,
        };
        let capability = devtools::NativeDevtools::for_tab(&self.state(), scope.clone())
            .map_err(|failure| format!("attach: {failure:?}"))?;
        Ok(AgentTab { capability, scope })
    }

    /// The user's "add page to chat" context read, at the tab's current revision.
    pub fn page_context(&self, tab: &str) -> Result<Value, String> {
        let revision = self
            .state()
            .tabs
            .lock()
            .map_err(|error| error.to_string())?
            .snapshot(tab)
            .ok_or("browser tab unavailable")?
            .revision;
        let context = devtools::capture_page_context(&self.state(), tab, &revision.to_string())?;
        serde_json::to_value(&context).map_err(|error| error.to_string())
    }

    /// Popups Sage hosts natively and the chrome has not yet acknowledged.
    pub fn native_popups(&self) -> Vec<NativePopup> {
        popups::browser_popup_pending(self.state())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|intent| {
                Some(NativePopup {
                    id: intent.id,
                    opener: intent.opener_tab_id,
                    child: intent.native_tab_id?,
                    url: intent.url,
                })
            })
            .collect()
    }

    /// How many navigations Sage has answered with
    /// `NavigationVerdict::Abandon` (a blank popup child closed because its
    /// first real navigation was refused).
    pub fn abandoned_popups(&self) -> u64 {
        popups::ABANDONED.load(Ordering::SeqCst)
    }

    /// Adopts a native popup as the chrome does (`browser_popup_ack`).
    pub fn acknowledge_popup(&self, popup: &NativePopup) -> Result<(), String> {
        popups::browser_popup_ack(
            self.app.clone(),
            self.state(),
            popup.id,
            popup.opener.clone(),
            popup.child.clone(),
        )
    }
}
