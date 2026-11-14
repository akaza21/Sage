//! Every request defaults to a visible, origin-labelled prompt that only the
//! user can answer; agent tabs are denied outright.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::admit_url;
use crate::settings_store::{MAX_SITE_ORIGIN_BYTES, MAX_SITE_PERMISSIONS, SettingsStore};

pub(crate) mod commands;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(super) use native::{
    cancel_tab_native, deny_navigation_native, document_committed_native,
    navigation_settled_native, register_native, resolve_native,
};

const MAX_PENDING_PERMISSIONS: usize = 8;
const MAX_WAITERS_PER_PERMISSION: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    ClipboardRead,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Decision {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
    /// The page held an Allow that has ended and has not reloaded since.
    stale_access: bool,
}

/// `None` for a non-admitted URL or an origin too long to save.
pub(super) fn site_origin(source: &str) -> Option<String> {
    let origin = admit_url(source).ok()?.origin().ascii_serialization();
    (origin.len() <= MAX_SITE_ORIGIN_BYTES).then_some(origin)
}

#[derive(Default)]
pub(super) struct SitePermissions {
    next_id: u64,
    /// Mirrored from `store` so a request never waits on SQLite. Every change
    /// writes the store first.
    saved: BTreeMap<(String, PermissionKind), Decision>,
    store: Option<SettingsStore>,
    pending: BTreeMap<u64, PermissionPrompt>,
    waiters: BTreeMap<u64, usize>,
    active_origin: BTreeMap<String, String>,
    closing_tabs: BTreeSet<String>,
    /// Only the commit of the latest navigation that started after the mark (a
    /// document or an error page) proves the old page is gone; a late commit of a
    /// navigation already under way when the Allow ended proves nothing. Cleared
    /// otherwise only once the tab's native close succeeds.
    stale_tabs: BTreeMap<String, Option<u64>>,
}

impl SitePermissions {
    pub(super) fn attach_store(&mut self, store: SettingsStore) -> Result<(), String> {
        let rows = store
            .browser_site_permissions()
            .map_err(|error| error.to_string())?;
        let mut saved = BTreeMap::new();
        for (origin, kind, decision) in rows {
            if site_origin(&origin).as_deref() == Some(origin.as_str()) {
                saved.insert((origin, kind), decision);
            } else {
                // Deleted so the store's cap and this mirror count the same rows.
                store
                    .delete_browser_site_permission(&origin, kind)
                    .map_err(|error| error.to_string())?;
            }
        }
        self.saved = saved;
        self.store = Some(store);
        Ok(())
    }

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
        let Some(origin) = site_origin(source) else {
            return RequestOutcome::Deny;
        };
        if self
            .active_origin
            .get(tab_id)
            .is_some_and(|active| active != &origin)
        {
            return RequestOutcome::Deny;
        }
        match self.saved.get(&(origin.clone(), kind)) {
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
        // A full store could not keep the answer, so the request is denied
        // instead of asking a question whose answer would be lost.
        if self.pending.len() >= MAX_PENDING_PERMISSIONS || self.saved.len() >= MAX_SITE_PERMISSIONS
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

    /// A failed save reports `false` and changes nothing, so the caller denies
    /// the request instead of granting an unsaved answer.
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
        let (origin, kind) = (prompt.origin.clone(), prompt.kind);
        if self.save(origin, kind, decision).is_err() {
            return false;
        }
        self.pending.remove(&id);
        self.waiters.remove(&id);
        true
    }

    /// The other pending prompts the saved answer now settles, as `(id, tab)`;
    /// `None` when nothing was saved.
    pub(super) fn resolve_site(
        &mut self,
        id: u64,
        tab_id: &str,
        decision: Decision,
    ) -> Option<Vec<(u64, String)>> {
        let prompt = self.pending.get(&id)?;
        let (origin, kind) = (prompt.origin.clone(), prompt.kind);
        self.resolve(id, tab_id, decision)
            .then(|| self.settled_by_saved(&origin, kind))
    }

    fn save(
        &mut self,
        origin: String,
        kind: PermissionKind,
        decision: Decision,
    ) -> Result<(), String> {
        if let Some(store) = &self.store {
            store
                .put_browser_site_permission(&origin, kind, decision)
                .map_err(|error| error.to_string())?;
        } else if self.saved.len() >= MAX_SITE_PERMISSIONS
            && !self.saved.contains_key(&(origin.clone(), kind))
        {
            return Err(format!(
                "Sage keeps at most {MAX_SITE_PERMISSIONS} site permissions"
            ));
        }
        self.saved.insert((origin, kind), decision);
        Ok(())
    }

    /// Settings only edits answers the user gave a prompt, so an unsaved site is
    /// refused.
    pub(super) fn set(
        &mut self,
        origin: &str,
        kind: PermissionKind,
        decision: Decision,
    ) -> Result<Decision, String> {
        let origin = canonical_origin(origin)?;
        let Some(previous) = self.saved.get(&(origin.clone(), kind)).copied() else {
            return Err("no saved permission for this site".into());
        };
        self.save(origin, kind, decision)?;
        Ok(previous)
    }

    pub(super) fn reset(
        &mut self,
        origin: &str,
        kind: PermissionKind,
    ) -> Result<Option<Decision>, String> {
        let origin = canonical_origin(origin)?;
        let key = (origin, kind);
        if !self.saved.contains_key(&key) {
            return Ok(None);
        }
        if let Some(store) = &self.store {
            store
                .delete_browser_site_permission(&key.0, kind)
                .map_err(|error| error.to_string())?;
        }
        Ok(self.saved.remove(&key))
    }

    /// Returns the origins that had an Allow, whose pages may still hold access
    /// until they reload.
    pub(super) fn reset_all(&mut self) -> Result<BTreeSet<String>, String> {
        if let Some(store) = &self.store {
            store
                .clear_browser_site_permissions()
                .map_err(|error| error.to_string())?;
        }
        Ok(std::mem::take(&mut self.saved)
            .into_iter()
            .filter(|(_, decision)| *decision == Decision::Allow)
            .map(|((origin, _), _)| origin)
            .collect())
    }

    pub(super) fn settled_by_saved(
        &self,
        origin: &str,
        kind: PermissionKind,
    ) -> Vec<(u64, String)> {
        if !self.saved.contains_key(&(origin.to_owned(), kind)) {
            return Vec::new();
        }
        self.pending
            .values()
            .filter(|prompt| prompt.origin == origin && prompt.kind == kind)
            .map(|prompt| (prompt.id, prompt.tab_id.clone()))
            .collect()
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

    /// A tab already marked starts over: navigations that began before this Allow
    /// ended no longer count.
    pub(super) fn mark_stale<'a>(&mut self, tabs: impl IntoIterator<Item = &'a str>) {
        for tab in tabs {
            self.stale_tabs.insert(tab.to_owned(), None);
        }
    }

    pub(super) fn navigation_started(&mut self, tab_id: &str, navigation_id: u64) {
        if let Some(after) = self.stale_tabs.get_mut(tab_id) {
            *after = Some(navigation_id);
        }
    }

    pub(super) fn document_committed(&mut self, tab_id: &str, navigation_id: u64) -> bool {
        let replaced = self.stale_tabs.get(tab_id) == Some(&Some(navigation_id));
        if replaced {
            self.stale_tabs.remove(tab_id);
        }
        replaced
    }

    pub(super) fn is_stale(&self, tab_id: &str) -> bool {
        self.stale_tabs.contains_key(tab_id)
    }

    pub(super) fn stale_tabs(&self) -> BTreeSet<String> {
        self.stale_tabs.keys().cloned().collect()
    }

    /// Saved answers belong to the site and stay, and so does a stale mark: the
    /// page lives until the native close succeeds (`finish_close`).
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
        ids
    }

    pub(super) fn finish_close(&mut self, tab_id: &str) {
        self.closing_tabs.remove(tab_id);
        self.stale_tabs.remove(tab_id);
    }

    pub(super) fn pending(&self, tab_id: &str) -> Vec<PermissionPrompt> {
        self.pending
            .values()
            .filter(|prompt| prompt.tab_id == tab_id)
            .cloned()
            .collect()
    }

    pub(super) fn decisions_for(&self, origin: &str) -> Vec<PermissionDecisionView> {
        self.saved
            .range((origin.to_owned(), PermissionKind::Camera)..)
            .take_while(|((saved, _), _)| saved == origin)
            .map(view)
            .collect()
    }

    pub(super) fn decisions(&self) -> Vec<PermissionDecisionView> {
        self.saved.iter().map(view).collect()
    }
}

fn view(
    ((origin, kind), decision): (&(String, PermissionKind), &Decision),
) -> PermissionDecisionView {
    PermissionDecisionView {
        origin: origin.clone(),
        kind: *kind,
        decision: *decision,
    }
}

/// Admits only an already-canonical origin, so Settings can never save a
/// path, credentials or a non-http(s) scheme.
fn canonical_origin(origin: &str) -> Result<String, String> {
    match site_origin(origin) {
        Some(admitted) if admitted == origin => Ok(admitted),
        _ => Err("invalid permission origin".into()),
    }
}

#[cfg(test)]
#[path = "site_permissions/store_tests.rs"]
mod store_tests;
#[cfg(test)]
#[path = "site_permissions/tests.rs"]
mod tests;
