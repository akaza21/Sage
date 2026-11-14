//! Browser developer mode (ADR-0009b). Only WebView2 has CDP, so elsewhere the
//! switch is unavailable.
//! Main webview only: the guard in `ipc_commands.rs` rejects browser WebViews.

use tauri::State;

use crate::state::AppState;

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
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<BrowserDeveloperMode, String> {
    if !AVAILABLE {
        return Err("Developer mode needs the Windows built-in browser.".into());
    }
    // Calls already in flight, and cards already open, belong to the old
    // setting: they are refused from here on.
    let enabled = crate::acp::browser::developer::with_switch_change(|| {
        persist_after_revocation(&state.settings, enabled, || {
            state.acp.end_browser_developer_mode();
        })
    })?;
    Ok(BrowserDeveloperMode {
        available: AVAILABLE,
        enabled,
        allowed_methods: crate::acp::browser::developer::allowed_methods(),
    })
}

/// Runtime privilege ends even if persisting the new switch value fails.
/// Reconfiguring an enabled switch also retires sessions of its old epoch.
fn persist_after_revocation(
    settings: &crate::settings_store::SettingsStore,
    enabled: bool,
    revoke: impl FnOnce(),
) -> Result<bool, String> {
    revoke();
    settings
        .set_browser_developer_mode(enabled)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
