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
    crate::browser::annotate::leave(app, &tab_id, crate::browser::annotate::Exit::Closed);
    let children = state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .take_for_opener(&tab_id);
    for (intent_id, child_id) in children {
        Box::pin(popups::cleanup::close_popup(
            app, state, intent_id, &child_id,
        ))
        .await
        .map_err(|error| format!("popup native close is unresolved: {error}"))?;
    }
    let pending_permissions = state
        .permissions
        .lock()
        .map_err(|error| error.to_string())?
        .cancel_tab(&tab_id);
    #[cfg(windows)]
    site_permissions::cancel_tab_native(&webview, pending_permissions).await?;
    #[cfg(not(windows))]
    let _ = (webview, pending_permissions);
    let closed = close_with_unlocked(&state.tabs, &tab_id, |webview| {
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
    .await;
    if closed.is_err()
        && let Some(webview) = state.webview_for_tab(&tab_id)
    {
        // A pre-close failure restored the owner. Reconcile activity and any
        // timer that expired while its tab was outside the registry.
        let _ = crate::browser::background::ensure_awake(&webview);
    }
    closed?;
    state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .forget_child(&tab_id);
    Ok(())
}
