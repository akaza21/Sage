use tauri::{Manager, State, Webview};

use super::{DeviceEmulation, DeviceFailure, DeviceHeld, Left, held_after};
use crate::browser::BrowserState;

/// Applies or resets the tab's emulation. Updates apply one at a time, and
/// one that a later request overtook while it waited does nothing, so the
/// page and what the tab records always end on the latest request.
pub(super) async fn set(
    state: &State<'_, BrowserState>,
    tab_id: String,
    device: Option<DeviceEmulation>,
) -> Result<(), DeviceFailure> {
    // The page was not reached, so what it holds is not known here.
    let unavailable = |message: &str| DeviceFailure::new(message, DeviceHeld::Unconfirmed);
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
        let latest = state
            .tabs
            .lock()
            .map_err(|_| unavailable("browser unavailable"))?
            .latest_device(&tab_id, request);
        if !latest {
            return Ok(());
        }
        let held = held_after(device, apply(&webview, device.as_ref()));
        let recorded = held.as_ref().map_or_else(DeviceFailure::held, |held| *held);
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
