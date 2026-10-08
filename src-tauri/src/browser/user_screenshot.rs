use serde::Serialize;
use tauri::{Manager, State};

use super::{BrowserState, BrowserTabs, page_context::valid_page_context_tab_id};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserScreenshot {
    width: u32,
    height: u32,
    png_base64: String,
}

pub(super) fn capture_target<T: Clone>(
    tabs: &BrowserTabs<T>,
    tab_id: &str,
    expected_revision: &str,
) -> Result<T, String> {
    if !valid_page_context_tab_id(tab_id) {
        return Err("invalid browser tab id".into());
    }
    let snapshot = tabs.snapshot(tab_id).ok_or("browser tab unavailable")?;
    if snapshot.revision.to_string() != expected_revision {
        return Err("browser page changed; retry screenshot".into());
    }
    tabs.openable_url(tab_id)?;
    tabs.handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable".into())
}

#[tauri::command]
pub(crate) async fn browser_capture_screenshot(
    state: State<'_, BrowserState>,
    tab_id: String,
    expected_revision: String,
) -> Result<UserScreenshot, String> {
    #[cfg(windows)]
    {
        let webview = {
            let tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
            capture_target(&tabs, &tab_id, &expected_revision)?
        };
        tauri::async_runtime::spawn_blocking(move || {
            let state = webview.app_handle().state::<BrowserState>();
            let (width, height, png_base64) =
                super::devtools::capture_user_png(&state, &tab_id, &expected_revision)?;
            Ok(UserScreenshot {
                width,
                height,
                png_base64,
            })
        })
        .await
        .map_err(|_| "browser screenshot unavailable".to_owned())?
    }
    #[cfg(not(windows))]
    {
        let _ = (state, tab_id, expected_revision);
        Err("browser screenshot is only available on Windows".into())
    }
}
