use serde::Serialize;
use tauri::{AppHandle, Manager, State, Webview};

use super::{BrowserState, admit_url, emit_snapshot, navigation_state};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserNavigation {
    pub(crate) tab_id: String,
    pub(crate) url: String,
    pub(crate) title: Option<String>,
    pub(crate) favicon: Option<String>,
    pub(crate) revision: String,
    pub(crate) loading: bool,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_forward: bool,
    pub(crate) zoom_percent: u16,
    pub(crate) device: Option<crate::browser::device::DeviceEmulation>,
    pub(crate) error: Option<String>,
    pub(crate) print_unavailable_reason: Option<&'static str>,
    pub(crate) annotate_unavailable_reason: Option<&'static str>,
    pub(crate) device_unavailable_reason: Option<&'static str>,
}

pub(crate) async fn browser_status(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<BrowserNavigation, String> {
    let webview: Webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .handle(&tab_id)
        .cloned()
        .ok_or("browser tab unavailable")?;
    let snapshot = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .snapshot(&tab_id)
        .ok_or("browser tab unavailable")?;
    let revision = snapshot.revision;
    let result =
        tauri::async_runtime::spawn_blocking(move || navigation_state(&webview, &snapshot))
            .await
            .map_err(|error| error.to_string())?;
    if state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .snapshot(&tab_id)
        .is_none_or(|current| current.revision != revision)
    {
        return Err("browser navigation changed; retry status".into());
    }
    result
}

pub(super) fn emit_or_retry(app: AppHandle, webview: Webview) {
    let label = webview.label().to_owned();
    let target = app
        .state::<BrowserState>()
        .tabs
        .lock()
        .ok()
        .and_then(|mut tabs| tabs.take_fallback(&label));
    if let Some(target) = target {
        tauri::async_runtime::spawn(async move {
            let result = admit_url(&target)
                .map_err(str::to_owned)
                .and_then(|url| webview.navigate(url).map_err(|error| error.to_string()));
            if result.is_err() {
                if let Ok(mut tabs) = app.state::<BrowserState>().tabs.lock() {
                    tabs.abort_fallback(&label);
                }
                emit_snapshot(app, webview);
            }
        });
    } else {
        emit_snapshot(app, webview);
    }
}
