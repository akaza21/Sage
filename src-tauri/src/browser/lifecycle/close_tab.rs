use super::*;

pub(crate) async fn close_browser_tab(
    app: &AppHandle,
    state: &BrowserState,
    tab_id: String,
) -> Result<(), String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&tab_id)
        .map(|tab| tab.handle.clone())
        .ok_or("browser tab unavailable")?;
    let pending_permissions = state
        .permissions
        .lock()
        .map_err(|error| error.to_string())?
        .cancel_tab(&tab_id);
    #[cfg(windows)]
    site_permissions::cancel_tab_native(&webview, pending_permissions).await?;
    #[cfg(not(windows))]
    let _ = (webview, pending_permissions);
    close_with_unlocked(&state.tabs, &tab_id, |webview| {
        let closing_tab_id = tab_id.clone();
        async move {
            #[cfg(windows)]
            devtools::detach(&webview).map_err(|error| format!("{error:?}"))?;
            if let Err(error) =
                downloads::cancel_tab_after_native(&state.downloads, &closing_tab_id, |ids| {
                    #[cfg(windows)]
                    downloads::cancel_native(&webview, ids)?;
                    #[cfg(not(windows))]
                    let _ = ids;
                    Ok(())
                })
            {
                eprintln!("browser native download cancellation failed during tab close: {error}");
                if let Ok(registry) = state.downloads.lock() {
                    let _ = app.emit_to(
                        "main",
                        "browser:downloads",
                        serde_json::json!({
                            "tabId": closing_tab_id,
                            "downloads": registry.views(&closing_tab_id),
                        }),
                    );
                }
            }
            close_webview_on_main_thread(app, &state.tabs, webview, true).await
        }
    })
    .await?;
    Ok(())
}
