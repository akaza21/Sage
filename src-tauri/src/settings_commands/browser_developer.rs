//! Browser developer mode (ADR-0009b): the Settings switch. Like the other
//! browser settings commands they run only for the main webview. Only
//! WebView2 has CDP, so elsewhere the switch is unavailable. Turning it off
//! ends every live grant at once and tells the browser chrome, so its
//! Developer mode tag goes away.

use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Whether this platform's browser can offer developer mode at all.
const AVAILABLE: bool = cfg!(windows);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserDeveloperMode {
    available: bool,
    enabled: bool,
    allowed_methods: Vec<&'static str>,
}

#[tauri::command]
pub(crate) fn browser_developer_mode(
    state: State<'_, AppState>,
) -> Result<BrowserDeveloperMode, String> {
    let enabled = state
        .settings
        .browser_developer_mode()
        .map_err(|error| error.to_string())?;
    Ok(BrowserDeveloperMode {
        available: AVAILABLE,
        enabled: AVAILABLE && enabled,
        allowed_methods: crate::acp::browser::developer::allowed_methods(),
    })
}

#[tauri::command]
pub(crate) fn browser_set_developer_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<BrowserDeveloperMode, String> {
    if !AVAILABLE {
        return Err("Developer mode needs the Windows built-in browser.".into());
    }
    // Calls already in flight, and cards already open, belong to the old
    // setting: they are refused from here on.
    crate::acp::browser::developer::bump_switch_generation();
    let enabled = state
        .settings
        .set_browser_developer_mode(enabled)
        .map_err(|error| error.to_string())?;
    if !enabled {
        for tab_id in state.acp.end_browser_developer_mode() {
            let _ = app.emit_to(
                "main",
                "browser:agent-control",
                serde_json::json!({ "tabId": tab_id }),
            );
        }
    }
    Ok(BrowserDeveloperMode {
        available: AVAILABLE,
        enabled,
        allowed_methods: crate::acp::browser::developer::allowed_methods(),
    })
}
