use super::*;

/// The WebView2 environment a popup tab must share with its opener. Other
/// platforms never pass one.
#[cfg(windows)]
pub(in crate::browser) type TabEnvironment =
    webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;
#[cfg(not(windows))]
pub(in crate::browser) type TabEnvironment = std::convert::Infallible;

/// Creates one policed browser child of `main`: the inert `about:blank`, the
/// browser profile and navigation admission. On Windows popups are decided
/// by Sage's own `NewWindowRequested` handler (`popups::native`), so no
/// `on_new_window` is set and wry only marks each request handled
/// (wry-0.55.1/src/webview2/mod.rs:781-783). Elsewhere the deny-and-enqueue
/// intent path stays as before.
pub(super) fn create_tab_webview(
    app: &AppHandle,
    window: &tauri::Window,
    profile_dir: &std::path::Path,
    label: &str,
    environment: Option<TabEnvironment>,
) -> Result<Webview, String> {
    let inert_url = Url::parse(popups::INITIAL_BLANK).map_err(|error| error.to_string())?;
    let builder = WebviewBuilder::new(label.to_owned(), WebviewUrl::External(inert_url))
        .data_directory(profile_dir.to_path_buf());
    #[cfg(windows)]
    let builder = {
        let navigation_app = app.clone();
        let navigation_label = label.to_owned();
        let builder = builder.on_navigation(move |next| {
            popups::navigation_admitted(&navigation_app, &navigation_label, next.as_str())
        });
        match environment {
            Some(environment) => builder.with_environment(environment),
            None => builder,
        }
    };
    #[cfg(not(windows))]
    let builder = {
        let _ = environment;
        let popup_app = app.clone();
        let popup_opener = label.to_owned();
        builder
            .on_navigation(allowed_browser_navigation)
            .on_new_window(move |requested, _| {
                popups::enqueue_intent(&popup_app, &popup_opener, &requested);
                NewWindowResponse::Deny
            })
    };
    window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1.0, 1.0),
        )
        .map_err(|error| format!("browser child creation failed: {error}"))
}

/// Windows, main thread only: reserves a cap slot and creates the popup's
/// tab in the opener's WebView2 environment through the shared `open_with`.
#[cfg(windows)]
pub(in crate::browser) fn open_popup_tab(
    app: &AppHandle,
    state: &BrowserState,
    environment: TabEnvironment,
    loading: bool,
) -> Result<String, String> {
    let window = app.get_window("main").ok_or("main window unavailable")?;
    let (label, _opening) = open_with(&state.tabs, &state.clearing_data, loading, |label| {
        create_tab_webview(app, &window, &state.profile_dir, label, Some(environment))
    })?;
    Ok(label)
}

/// Native event hooks every browser tab gets, in a fixed order.
pub(in crate::browser) async fn register_tab_hooks(
    app: &AppHandle,
    webview: &Webview,
) -> Result<(), String> {
    navigation::register_native(app.clone(), webview.clone())
        .await
        .map_err(|error| format!("browser navigation setup failed: {error}"))?;
    shortcuts::register_native(app.clone(), webview.clone())
        .await
        .map_err(|error| format!("browser shortcuts setup failed: {error}"))?;
    #[cfg(windows)]
    {
        downloads::register_native(app.clone(), webview.clone())
            .await
            .map_err(|error| format!("browser downloads setup failed: {error}"))?;
        site_permissions::register_native(app.clone(), webview.clone())
            .await
            .map_err(|error| format!("browser permissions setup failed: {error}"))?;
        popups::register_native(app.clone(), webview.clone())
            .await
            .map_err(|error| format!("browser popup setup failed: {error}"))?;
    }
    Ok(())
}
