use tauri::{LogicalPosition, LogicalSize, Manager, State};

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
        .map_err(|error| error.to_string())?;
    // A page frozen for an Annotate comment stays hidden until the comment
    // is done. The check runs on the main thread, where Annotate's hide runs
    // too and only after it marks the page frozen, so a show checked before
    // the mark is always followed by that hide.
    let app = webview.app_handle().clone();
    let shown = webview.clone();
    app.clone()
        .run_on_main_thread(move || {
            if !app.state::<BrowserState>().annotate.is_frozen(&tab_id) {
                let _ = shown.show();
            }
        })
        .map_err(|error| error.to_string())
}
