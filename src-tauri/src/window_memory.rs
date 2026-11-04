//! Window events are inputs to the shared per-WebView memory policy.
#[cfg(windows)]
use tauri::Manager;

pub(crate) fn handle_window_event(app: &tauri::AppHandle, label: &str, event: &tauri::WindowEvent) {
    #[cfg(windows)]
    if matches!(event, tauri::WindowEvent::Resized(_))
        && let Some(window) = app.get_window(label)
    {
        for webview in window.webviews() {
            crate::browser::background::refresh(&webview);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, label, event);
}

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder = tauri::plugin::Builder::new("window-memory");
    #[cfg(windows)]
    let builder = builder.on_webview_ready(|webview| crate::browser::background::refresh(&webview));
    builder.build()
}
