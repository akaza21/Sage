//! Site permission commands. All of them run only for the main webview: the
//! invoke guard in `ipc_commands.rs` rejects every browser tab, and any other
//! origin, before a command runs. Origins from the renderer must already be
//! canonical http(s) origins; kinds and decisions are closed enums.

use std::collections::BTreeSet;

use tauri::{AppHandle, Emitter, State, Webview};

#[cfg(windows)]
use super::resolve_native;
use super::{
    Decision, PermissionDecisionView, PermissionKind, PermissionSnapshot, SitePermissions,
    site_origin,
};
use crate::browser::BrowserState;

const MAX_TAB_ID_BYTES: usize = 64;
const MAX_ORIGIN_ARGUMENT_BYTES: usize = 2048;

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    error.to_string()
}

pub(super) fn snapshot(state: &BrowserState, tab_id: &str) -> Result<PermissionSnapshot, String> {
    let url = state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .get(tab_id)
        .map(|tab| tab.committed_url.clone())
        .ok_or("browser tab unavailable")?;
    let permissions = state.permissions.lock().map_err(lock_error)?;
    let pending = permissions.pending(tab_id);
    let origin = url
        .as_deref()
        .and_then(site_origin)
        .or_else(|| pending.first().map(|prompt| prompt.origin.clone()));
    let decisions = origin
        .as_deref()
        .map(|origin| permissions.decisions_for(origin))
        .unwrap_or_default();
    Ok(PermissionSnapshot {
        tab_id: tab_id.to_owned(),
        origin: origin.unwrap_or_else(|| "unavailable".into()),
        pending,
        decisions,
    })
}

fn site_settings(state: &BrowserState) -> Result<Vec<PermissionDecisionView>, String> {
    Ok(state.permissions.lock().map_err(lock_error)?.decisions())
}

/// Tells the main webview the saved answers changed: every open tab's drawer
/// gets a fresh snapshot, and Settings gets the full list.
pub(super) fn emit_site_settings(app: &AppHandle, state: &BrowserState) {
    let labels = state
        .tabs
        .lock()
        .map(|tabs| tabs.entries.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for label in labels {
        if let Ok(updated) = snapshot(state, &label) {
            let _ = app.emit_to("main", "browser:permissions", updated);
        }
    }
    if let Ok(settings) = site_settings(state) {
        let _ = app.emit_to("main", "browser:site-settings", settings);
    }
}

/// Every tab showing one of `origins`, whoever drives it. A page that was
/// granted access keeps it until its document goes away, so these reload
/// after an Allow ends.
fn tabs_on(state: &BrowserState, origins: &BTreeSet<String>) -> Result<Vec<Webview>, String> {
    if origins.is_empty() {
        return Ok(Vec::new());
    }
    Ok(state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .values()
        .filter(|tab| {
            tab.committed_url
                .as_deref()
                .and_then(site_origin)
                .is_some_and(|origin| origins.contains(&origin))
        })
        .map(|tab| tab.handle.clone())
        .collect())
}

fn apply(
    app: &AppHandle,
    state: &BrowserState,
    change: impl FnOnce(&mut SitePermissions) -> Result<BTreeSet<String>, String>,
) -> Result<Vec<PermissionDecisionView>, String> {
    let ended_allows = change(&mut *state.permissions.lock().map_err(lock_error)?)?;
    let stale = tabs_on(state, &ended_allows)?;
    emit_site_settings(app, state);
    // Every tab is tried even if one fails, so no other tab keeps access.
    let failed = stale
        .iter()
        .filter(|webview| webview.reload().is_err())
        .count();
    if failed > 0 {
        return Err(format!(
            "The change was saved, but {failed} browser tab(s) could not reload"
        ));
    }
    site_settings(state)
}

fn admit_origin_argument(origin: &str) -> Result<(), String> {
    if origin.len() > MAX_ORIGIN_ARGUMENT_BYTES {
        return Err("invalid permission origin".into());
    }
    Ok(())
}

fn ended_allow(origin: &str, previous: Option<Decision>) -> BTreeSet<String> {
    if previous == Some(Decision::Allow) {
        BTreeSet::from([origin.to_owned()])
    } else {
        BTreeSet::new()
    }
}

#[tauri::command]
pub(crate) async fn browser_permission_status(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<PermissionSnapshot, String> {
    if tab_id.len() > MAX_TAB_ID_BYTES {
        return Err("browser tab unavailable".into());
    }
    snapshot(&state, &tab_id)
}

/// Every saved site permission, for Settings > Browser.
#[tauri::command]
pub(crate) async fn browser_site_settings(
    state: State<'_, BrowserState>,
) -> Result<Vec<PermissionDecisionView>, String> {
    site_settings(&state)
}

/// Switches a saved answer between Allow and Block; a site with no saved
/// answer is refused. Changing an Allow to Block reloads that site's tabs so
/// their access ends now.
#[tauri::command]
pub(crate) async fn browser_site_setting_set(
    app: AppHandle,
    state: State<'_, BrowserState>,
    origin: String,
    kind: PermissionKind,
    decision: Decision,
) -> Result<Vec<PermissionDecisionView>, String> {
    admit_origin_argument(&origin)?;
    apply(&app, &state, |permissions| {
        let previous = permissions.set(&origin, kind, decision)?;
        Ok(if decision == Decision::Deny {
            ended_allow(&origin, Some(previous))
        } else {
            BTreeSet::new()
        })
    })
}

/// Forgets one site's answer, so the site asks again. Ending an Allow reloads
/// that site's tabs.
#[tauri::command]
pub(crate) async fn browser_site_setting_reset(
    app: AppHandle,
    state: State<'_, BrowserState>,
    origin: String,
    kind: PermissionKind,
) -> Result<Vec<PermissionDecisionView>, String> {
    admit_origin_argument(&origin)?;
    apply(&app, &state, |permissions| {
        Ok(ended_allow(&origin, permissions.reset(&origin, kind)?))
    })
}

/// Forgets every saved answer and reloads tabs on sites that had an Allow.
#[tauri::command]
pub(crate) async fn browser_site_settings_reset_all(
    app: AppHandle,
    state: State<'_, BrowserState>,
) -> Result<Vec<PermissionDecisionView>, String> {
    apply(&app, &state, SitePermissions::reset_all)
}

#[tauri::command]
pub(crate) async fn browser_permission_resolve(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    id: u64,
    allow: bool,
) -> Result<(), String> {
    if tab_id.len() > MAX_TAB_ID_BYTES || id == 0 {
        return Err("browser permission request unavailable".into());
    }
    let webview = state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .get(&tab_id)
        .map(|tab| tab.handle.clone())
        .ok_or("browser tab unavailable")?;
    if !state
        .permissions
        .lock()
        .map_err(lock_error)?
        .pending(&tab_id)
        .iter()
        .any(|prompt| prompt.id == id)
    {
        return Err("browser permission request unavailable".into());
    }
    #[cfg(windows)]
    {
        resolve_native(
            app,
            webview,
            tab_id,
            id,
            if allow {
                Decision::Allow
            } else {
                Decision::Deny
            },
        )
        .await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, webview, allow);
        Err("browser permissions require Windows WebView2".into())
    }
}
