use tauri::{Manager, State, Webview};

use super::DeviceEmulation;
use crate::browser::BrowserState;

/// Applies or resets the tab's emulation. Updates apply one at a time, and
/// one that a later request overtook while it waited does nothing, so the
/// page and what the tab records always end on the latest request.
pub(super) async fn set(
    state: &State<'_, BrowserState>,
    tab_id: String,
    device: Option<DeviceEmulation>,
) -> Result<(), String> {
    let (webview, request) = {
        let mut tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        let webview = tabs
            .handle(&tab_id)
            .cloned()
            .ok_or("browser tab unavailable")?;
        let request = tabs
            .request_device(&tab_id)
            .ok_or("browser tab unavailable")?;
        (webview, request)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let state = webview.app_handle().state::<BrowserState>();
        let _turn = state
            .device_updates
            .lock()
            .map_err(|_| "device toolbar unavailable")?;
        let latest = |state: &BrowserState| {
            state
                .tabs
                .lock()
                .is_ok_and(|tabs| tabs.latest_device(&tab_id, request))
        };
        if !latest(&state) {
            return Ok(());
        }
        let applied = apply(&webview, device.as_ref());
        // A failed apply has reset the tab, so it holds no emulation.
        let held = if applied.is_ok() { device } else { None };
        state
            .tabs
            .lock()
            .map_err(|_| "browser unavailable")?
            .settle_device(&tab_id, request, held);
        applied
    })
    .await
    .map_err(|_| "device toolbar unavailable".to_owned())?
}

/// Sends the calls over WebView2's own CDP session, outside any Agent
/// permit. Runs off the main thread: each call waits for the WebView.
fn apply(webview: &Webview, emulation: Option<&DeviceEmulation>) -> Result<(), String> {
    super::apply_with(emulation, |method, params| {
        crate::browser::devtools::device_call(webview, method, params)
            .map_err(|_| "device toolbar could not update the page".to_owned())
    })
}
