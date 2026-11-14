//! The invoke guard in `ipc_commands.rs` rejects every browser tab, and any
//! other origin, before a command runs.

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use serde::Serialize;
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
        stale_access: permissions.is_stale(tab_id),
        pending,
        decisions,
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SiteSettingsView {
    settings: Vec<PermissionDecisionView>,
    stale_sites: Vec<String>,
}

fn site_settings(state: &BrowserState) -> Result<SiteSettingsView, String> {
    let (settings, stale) = {
        let permissions = state.permissions.lock().map_err(lock_error)?;
        (permissions.decisions(), permissions.stale_tabs())
    };
    let stale_sites = state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .iter()
        .filter(|(label, _)| stale.contains(*label))
        .filter_map(|(_, tab)| tab.committed_url.as_deref().and_then(site_origin))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(SiteSettingsView {
        settings,
        stale_sites,
    })
}

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
    if let Ok(view) = site_settings(state) {
        let _ = app.emit_to("main", "browser:site-settings", view);
    }
}

fn tabs_on(
    state: &BrowserState,
    origins: &BTreeSet<String>,
) -> Result<Vec<(String, Webview)>, String> {
    if origins.is_empty() {
        return Ok(Vec::new());
    }
    Ok(state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .iter()
        .filter(|(_, tab)| {
            tab.committed_url
                .as_deref()
                .and_then(site_origin)
                .is_some_and(|origin| origins.contains(&origin))
        })
        .map(|(label, tab)| (label.clone(), tab.handle.clone()))
        .collect())
}

const RELOAD_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const RELOAD_CONFIRM_POLL: Duration = Duration::from_millis(50);

/// `WebView::reload` only queues the reload, so only the commit of a
/// navigation that started after the mark proves the old page is gone. Tabs
/// that do not get there stay marked.
async fn reload_and_confirm(state: &BrowserState, tabs: Vec<(String, Webview)>) {
    if tabs.is_empty() {
        return;
    }
    for (_, webview) in &tabs {
        // A failed request leaves the tab marked; the wait below reports it.
        let _ = webview.reload();
    }
    let deadline = Instant::now() + RELOAD_CONFIRM_TIMEOUT;
    while Instant::now() < deadline {
        let pending = state
            .permissions
            .lock()
            .is_ok_and(|permissions| tabs.iter().any(|(label, _)| permissions.is_stale(label)));
        if !pending {
            return;
        }
        tokio::time::sleep(RELOAD_CONFIRM_POLL).await;
    }
}

/// Tabs on sites whose Allow ended are marked stale before they reload, so a
/// page that keeps its document is reported rather than silently treated as
/// revoked.
async fn apply(
    app: &AppHandle,
    state: &BrowserState,
    change: impl FnOnce(&mut SitePermissions) -> Result<BTreeSet<String>, String>,
) -> Result<SiteSettingsView, String> {
    let ended_allows = change(&mut *state.permissions.lock().map_err(lock_error)?)?;
    let tabs = tabs_on(state, &ended_allows)?;
    state
        .permissions
        .lock()
        .map_err(lock_error)?
        .mark_stale(tabs.iter().map(|(label, _)| label.as_str()));
    emit_site_settings(app, state);
    reload_and_confirm(state, tabs).await;
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

#[tauri::command]
pub(crate) async fn browser_site_settings(
    state: State<'_, BrowserState>,
) -> Result<SiteSettingsView, String> {
    site_settings(&state)
}

#[tauri::command]
pub(crate) async fn browser_site_setting_set(
    app: AppHandle,
    state: State<'_, BrowserState>,
    origin: String,
    kind: PermissionKind,
    decision: Decision,
) -> Result<SiteSettingsView, String> {
    admit_origin_argument(&origin)?;
    apply(&app, &state, |permissions| {
        let previous = permissions.set(&origin, kind, decision)?;
        Ok(if decision == Decision::Deny {
            ended_allow(&origin, Some(previous))
        } else {
            BTreeSet::new()
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn browser_permission_reset(
    app: AppHandle,
    state: State<'_, BrowserState>,
    origin: String,
    kind: PermissionKind,
) -> Result<SiteSettingsView, String> {
    admit_origin_argument(&origin)?;
    apply(&app, &state, |permissions| {
        Ok(ended_allow(&origin, permissions.reset(&origin, kind)?))
    })
    .await
}

#[tauri::command]
pub(crate) async fn browser_site_settings_reset_all(
    app: AppHandle,
    state: State<'_, BrowserState>,
) -> Result<SiteSettingsView, String> {
    apply(&app, &state, SitePermissions::reset_all).await
}

#[tauri::command]
pub(crate) async fn browser_site_reload_tabs(
    app: AppHandle,
    state: State<'_, BrowserState>,
) -> Result<SiteSettingsView, String> {
    let stale = state.permissions.lock().map_err(lock_error)?.stale_tabs();
    let tabs = state
        .tabs
        .lock()
        .map_err(lock_error)?
        .entries
        .iter()
        .filter(|(label, _)| stale.contains(*label))
        .map(|(label, tab)| (label.clone(), tab.handle.clone()))
        .collect();
    reload_and_confirm(&state, tabs).await;
    emit_site_settings(&app, &state);
    site_settings(&state)
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
