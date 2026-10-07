use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::{BrowserState, admit_url};

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(super) use native::{
    cancel_tab_native, deny_navigation_native, navigation_settled_native, register_native,
    resolve_native,
};

const MAX_PENDING_PERMISSIONS: usize = 8;
const MAX_WAITERS_PER_PERMISSION: usize = 8;
const MAX_SAVED_PERMISSIONS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    ClipboardRead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Decision {
    Allow,
    Deny,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionPrompt {
    pub(super) id: u64,
    pub(super) tab_id: String,
    pub(super) origin: String,
    pub(super) kind: PermissionKind,
}

pub(super) enum RequestOutcome {
    Allow,
    Deny,
    Prompt(PermissionPrompt),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionDecisionView {
    origin: String,
    kind: PermissionKind,
    decision: Decision,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionSnapshot {
    tab_id: String,
    origin: String,
    pending: Vec<PermissionPrompt>,
    decisions: Vec<PermissionDecisionView>,
}

#[derive(Default)]
pub(super) struct SitePermissions {
    next_id: u64,
    decisions: BTreeMap<(String, String, PermissionKind), Decision>,
    pending: BTreeMap<u64, PermissionPrompt>,
    waiters: BTreeMap<u64, usize>,
    active_origin: BTreeMap<String, String>,
    closing_tabs: BTreeSet<String>,
}

impl SitePermissions {
    pub(super) fn request(
        &mut self,
        tab_id: &str,
        source: &str,
        kind: PermissionKind,
        agent: bool,
    ) -> RequestOutcome {
        if agent || self.closing_tabs.contains(tab_id) {
            return RequestOutcome::Deny;
        }
        let Ok(url) = admit_url(source) else {
            return RequestOutcome::Deny;
        };
        let origin = url.origin().ascii_serialization();
        if self
            .active_origin
            .get(tab_id)
            .is_some_and(|active| active != &origin)
        {
            return RequestOutcome::Deny;
        }
        match self
            .decisions
            .get(&(tab_id.to_owned(), origin.clone(), kind))
        {
            Some(Decision::Allow) => return RequestOutcome::Allow,
            Some(Decision::Deny) => return RequestOutcome::Deny,
            None => {}
        }
        if let Some(prompt) = self
            .pending
            .values()
            .find(|prompt| {
                prompt.tab_id == tab_id && prompt.origin == origin && prompt.kind == kind
            })
            .cloned()
        {
            let waiters = self.waiters.entry(prompt.id).or_insert(1);
            if *waiters >= MAX_WAITERS_PER_PERMISSION {
                return RequestOutcome::Deny;
            }
            *waiters += 1;
            return RequestOutcome::Prompt(prompt);
        }
        if self.pending.len() >= MAX_PENDING_PERMISSIONS
            || self.decisions.len() >= MAX_SAVED_PERMISSIONS
        {
            return RequestOutcome::Deny;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            return RequestOutcome::Deny;
        };
        self.next_id = id;
        let prompt = PermissionPrompt {
            id,
            tab_id: tab_id.to_owned(),
            origin,
            kind,
        };
        self.pending.insert(id, prompt.clone());
        self.waiters.insert(id, 1);
        RequestOutcome::Prompt(prompt)
    }

    pub(super) fn resolve(&mut self, id: u64, tab_id: &str, decision: Decision) -> bool {
        let Some(prompt) = self.pending.get(&id) else {
            return false;
        };
        if prompt.tab_id != tab_id {
            return false;
        }
        if self
            .active_origin
            .get(tab_id)
            .is_some_and(|active| active != &prompt.origin)
        {
            return false;
        }
        let prompt = self
            .pending
            .remove(&id)
            .expect("checked pending permission");
        self.waiters.remove(&id);
        self.decisions
            .insert((prompt.tab_id, prompt.origin, prompt.kind), decision);
        true
    }

    pub(super) fn cancel(&mut self, id: u64) -> bool {
        self.waiters.remove(&id);
        self.pending.remove(&id).is_some()
    }

    pub(super) fn remove_waiter(&mut self, id: u64) {
        if let Some(waiters) = self.waiters.get_mut(&id)
            && *waiters > 1
        {
            *waiters -= 1;
            return;
        }
        self.cancel(id);
    }

    pub(super) fn navigation_to(&mut self, tab_id: &str, source: &str) -> Vec<u64> {
        let origin = admit_url(source)
            .ok()
            .map(|url| url.origin().ascii_serialization());
        self.set_origin(tab_id, origin)
    }

    pub(super) fn navigation_settled(&mut self, tab_id: &str, source: Option<&str>) -> Vec<u64> {
        let origin = source
            .and_then(|source| admit_url(source).ok())
            .map(|url| url.origin().ascii_serialization());
        self.set_origin(tab_id, origin)
    }

    fn set_origin(&mut self, tab_id: &str, origin: Option<String>) -> Vec<u64> {
        if let Some(origin) = &origin {
            self.active_origin.insert(tab_id.to_owned(), origin.clone());
        } else {
            self.active_origin.remove(tab_id);
        }
        self.pending
            .iter()
            .filter_map(|(id, prompt)| {
                (prompt.tab_id == tab_id && origin.as_deref() != Some(prompt.origin.as_str()))
                    .then_some(*id)
            })
            .collect()
    }

    pub(super) fn revoke(&mut self, tab_id: &str, origin: &str, kind: PermissionKind) -> bool {
        self.decisions
            .remove(&(tab_id.to_owned(), origin.to_owned(), kind))
            .is_some()
    }

    pub(super) fn cancel_tab(&mut self, tab_id: &str) -> Vec<u64> {
        self.closing_tabs.insert(tab_id.to_owned());
        self.active_origin.remove(tab_id);
        let ids = self
            .pending
            .iter()
            .filter(|(_, prompt)| prompt.tab_id == tab_id)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in &ids {
            self.pending.remove(id);
            self.waiters.remove(id);
        }
        self.decisions.retain(|(owner, _, _), _| owner != tab_id);
        ids
    }

    pub(super) fn finish_close(&mut self, tab_id: &str) {
        self.closing_tabs.remove(tab_id);
    }

    pub(super) fn pending(&self, tab_id: &str) -> Vec<PermissionPrompt> {
        self.pending
            .values()
            .filter(|prompt| prompt.tab_id == tab_id)
            .cloned()
            .collect()
    }

    pub(super) fn decisions(&self, tab_id: &str) -> Vec<PermissionDecisionView> {
        self.decisions
            .iter()
            .filter(|((owner, _, _), _)| owner == tab_id)
            .map(|((_, origin, kind), decision)| PermissionDecisionView {
                origin: origin.clone(),
                kind: *kind,
                decision: *decision,
            })
            .collect()
    }
}

fn snapshot(state: &BrowserState, tab_id: &str) -> Result<PermissionSnapshot, String> {
    let url = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(tab_id)
        .map(|tab| tab.committed_url.clone())
        .ok_or("browser tab unavailable")?;
    let permissions = state
        .permissions
        .lock()
        .map_err(|error| error.to_string())?;
    let pending = permissions.pending(tab_id);
    let origin = url
        .as_deref()
        .and_then(|url| admit_url(url).ok())
        .map(|url| url.origin().ascii_serialization())
        .or_else(|| pending.first().map(|prompt| prompt.origin.clone()))
        .unwrap_or_else(|| "unavailable".into());
    Ok(PermissionSnapshot {
        tab_id: tab_id.to_owned(),
        origin,
        pending,
        decisions: permissions.decisions(tab_id),
    })
}

#[tauri::command]
pub(crate) async fn browser_permission_status(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<PermissionSnapshot, String> {
    if tab_id.len() > 64 {
        return Err("browser tab unavailable".into());
    }
    snapshot(&state, &tab_id)
}

#[tauri::command]
pub(crate) async fn browser_permission_revoke(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    origin: String,
    kind: PermissionKind,
) -> Result<(), String> {
    if tab_id.len() > 64 || origin.len() > 256 {
        return Err("browser tab unavailable".into());
    }
    let webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&tab_id)
        .map(|tab| tab.handle.clone())
        .ok_or("browser tab unavailable")?;
    let admitted_origin = admit_url(&origin)
        .map_err(str::to_owned)?
        .origin()
        .ascii_serialization();
    if admitted_origin != origin {
        return Err("invalid permission origin".into());
    }
    state
        .permissions
        .lock()
        .map_err(|error| error.to_string())?
        .revoke(&tab_id, &origin, kind);
    webview.reload().map_err(|error| error.to_string())?;
    if let Ok(updated) = snapshot(&state, &tab_id) {
        let _ = app.emit_to("main", "browser:permissions", updated);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn browser_permission_resolve(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    id: u64,
    allow: bool,
) -> Result<(), String> {
    if tab_id.len() > 64 || id == 0 {
        return Err("browser permission request unavailable".into());
    }
    let webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&tab_id)
        .map(|tab| tab.handle.clone())
        .ok_or("browser tab unavailable")?;
    if !state
        .permissions
        .lock()
        .map_err(|error| error.to_string())?
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

#[cfg(test)]
#[path = "site_permissions/tests.rs"]
mod tests;
