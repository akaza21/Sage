//! Native ownership of the primary workbench viewport. Renderer events can
//! coalesce a restore and a late capture rollback into no resize event at all.
#[cfg(windows)]
mod geometry;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod renderer;

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder = tauri::plugin::Builder::new("window-viewport");
    #[cfg(windows)]
    let builder = builder
        .on_webview_ready(geometry::observe)
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                geometry::observe(webview.clone());
            }
        })
        .on_event(|_, event| match event {
            tauri::RunEvent::WindowEvent { label, event, .. } if label == "main" => {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    geometry::stop();
                } else if matches!(
                    event,
                    tauri::WindowEvent::Resized(_)
                        | tauri::WindowEvent::Moved(_)
                        | tauri::WindowEvent::ScaleFactorChanged { .. }
                        | tauri::WindowEvent::Focused(_)
                ) {
                    geometry::reconcile();
                }
            }
            tauri::RunEvent::Exit => geometry::stop(),
            _ => {}
        });
    builder.build()
}
