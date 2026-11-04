//! Windows caption integration. The platform config decides whether the native
//! caption is hidden; the workbench keeps ownership of its title-bar contents.
mod controls;
mod paint;

pub(crate) fn initialization_script() -> String {
    include_str!("window-controls.js").replace(
        "/* SAGE_WINDOW_FRAME_STYLE */ \"\"",
        &serde_json::to_string(include_str!("window-controls.css"))
            .expect("window frame stylesheet is JSON text"),
    )
}

/// Native caption input stays outside document-modal inertness. The trusted
/// workbench supplies layout and palette; native state owns maximization,
/// painting and accessibility.
#[tauri::command]
pub(crate) async fn set_window_caption_controls(
    webview: tauri::Webview,
    layout: Option<controls::CaptionLayout>,
) -> Result<(), String> {
    controls::set_layout(webview, layout).await
}

pub(crate) use controls::handle_window_event;
