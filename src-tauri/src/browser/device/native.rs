use tauri::{Manager, State, Webview};

use super::{DeviceEmulation, DeviceFailure, Left, held_after};
use crate::browser::BrowserState;

/// Applies or resets the tab's emulation. Updates apply one at a time, and
/// one that a later request overtook while it waited does nothing, so the
/// page and what the tab records always end on the latest request.
pub(super) async fn set(
    state: &State<'_, BrowserState>,
    tab_id: String,
    device: Option<DeviceEmulation>,
) -> Result<(), DeviceFailure> {
    let unavailable = |message: &str| DeviceFailure::new(message, None);
    let (webview, request) = {
        let mut tabs = state
            .tabs
            .lock()
            .map_err(|_| unavailable("browser unavailable"))?;
        let webview = tabs
            .handle(&tab_id)
            .cloned()
            .ok_or_else(|| unavailable("browser tab unavailable"))?;
        let request = tabs
            .request_device(&tab_id)
            .ok_or_else(|| unavailable("browser tab unavailable"))?;
        (webview, request)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let state = webview.app_handle().state::<BrowserState>();
        let _turn = state
            .device_updates
            .lock()
            .map_err(|_| unavailable("device toolbar unavailable"))?;
        let previous = {
            let tabs = state
                .tabs
                .lock()
                .map_err(|_| unavailable("browser unavailable"))?;
            if !tabs.latest_device(&tab_id, request) {
                return Ok(());
            }
            tabs.applied_device(&tab_id)
        };
        let held = held_after(device, previous, apply(&webview, device.as_ref()));
        let recorded = match &held {
            Ok(device) => *device,
            Err(failure) => failure.device,
        };
        state
            .tabs
            .lock()
            .map_err(|_| unavailable("browser unavailable"))?
            .settle_device(&tab_id, request, recorded);
        held.map(drop)
    })
    .await
    .map_err(|_| unavailable("device toolbar unavailable"))?
}

/// Sends the calls over WebView2's own CDP session, outside any Agent
/// permit. Runs off the main thread: each call waits for the WebView.
fn apply(webview: &Webview, emulation: Option<&DeviceEmulation>) -> Result<(), (String, Left)> {
    super::apply_with(emulation, |method, params| {
        crate::browser::devtools::device_call(webview, method, params)
            .map_err(|_| "device toolbar could not update the page".to_owned())
    })
}
