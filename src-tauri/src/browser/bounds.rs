use tauri::{LogicalPosition, LogicalSize, State};

use super::BrowserState;

#[tauri::command]
pub(crate) async fn browser_set_bounds(
    state: State<'_, BrowserState>,
    tab_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if ![x, y, width, height].iter().all(|value| value.is_finite())
        || x < 0.0
        || y < 0.0
        || width < 1.0
        || height < 1.0
        || width > 10000.0
        || height > 10000.0
    {
        return Err("invalid browser bounds".into());
    }
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    let webview = tabs.handle(&tab_id).ok_or("browser tab unavailable")?;
    webview
        .set_position(LogicalPosition::new(x, y))
        .and_then(|()| webview.set_size(LogicalSize::new(width, height)))
        .and_then(|()| webview.show())
        .map_err(|error| error.to_string())
}
