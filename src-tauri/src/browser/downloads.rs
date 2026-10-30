use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{BrowserState, admit_url};

mod close;
#[cfg(windows)]
mod native;
pub(crate) mod show;
pub(super) use close::cancel_tab_after_native;
#[cfg(windows)]
pub(super) use native::{cancel_native, register_native};

const MAX_DOWNLOADS: usize = 16;
const MAX_ACTIVE_DOWNLOADS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationProvenance {
    User,
    Agent,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadView {
    pub(super) tab_id: String,
    pub(super) id: u64,
    pub(super) source: String,
    pub(super) file_name: String,
    pub(super) state: &'static str,
    pub(super) received_bytes: u64,
    pub(super) total_bytes: Option<u64>,
}

pub(super) struct DownloadRegistry {
    next_id: u64,
    provenance: BTreeMap<String, NavigationProvenance>,
    agent_generation: BTreeMap<String, u64>,
    pending_user_navigation: BTreeMap<String, PendingUserNavigation>,
    entries: BTreeMap<u64, DownloadView>,
    destinations: BTreeMap<u64, PathBuf>,
}

struct PendingUserNavigation {
    url: String,
    navigation_id: Option<u64>,
    agent_generation: u64,
}

impl Default for DownloadRegistry {
    fn default() -> Self {
        Self {
            next_id: 1,
            provenance: BTreeMap::new(),
            agent_generation: BTreeMap::new(),
            pending_user_navigation: BTreeMap::new(),
            entries: BTreeMap::new(),
            destinations: BTreeMap::new(),
        }
    }
}

impl DownloadRegistry {
    pub(super) fn set_provenance(&mut self, tab_id: &str, provenance: NavigationProvenance) {
        if provenance == NavigationProvenance::Agent {
            let generation = self.agent_generation.entry(tab_id.to_owned()).or_default();
            *generation = generation.saturating_add(1);
            self.pending_user_navigation.remove(tab_id);
        }
        self.provenance.insert(tab_id.to_owned(), provenance);
    }

    pub(super) fn is_agent(&self, tab_id: &str) -> bool {
        self.provenance.get(tab_id) == Some(&NavigationProvenance::Agent)
    }

    pub(super) fn request_user_navigation(&mut self, tab_id: &str, url: &str) {
        self.pending_user_navigation.insert(
            tab_id.to_owned(),
            PendingUserNavigation {
                url: url.to_owned(),
                navigation_id: None,
                agent_generation: *self.agent_generation.get(tab_id).unwrap_or(&0),
            },
        );
    }

    pub(super) fn start_user_navigation(&mut self, tab_id: &str, id: u64, url: &str) {
        if let Some(pending) = self.pending_user_navigation.get_mut(tab_id)
            && pending.url == url
        {
            pending.navigation_id = Some(id);
        }
    }

    pub(super) fn finish_user_navigation(&mut self, tab_id: &str, id: u64, success: bool) {
        if !self
            .pending_user_navigation
            .get(tab_id)
            .is_some_and(|pending| pending.navigation_id == Some(id))
        {
            return;
        }
        let pending = self
            .pending_user_navigation
            .remove(tab_id)
            .expect("checked pending user navigation");
        if success && pending.agent_generation == *self.agent_generation.get(tab_id).unwrap_or(&0) {
            self.provenance
                .insert(tab_id.to_owned(), NavigationProvenance::User);
        }
    }

    pub(super) fn cancel_user_navigation(&mut self, tab_id: &str) {
        self.pending_user_navigation.remove(tab_id);
    }

    pub(super) fn begin(
        &mut self,
        tab_id: &str,
        source: &str,
        file_name: &str,
    ) -> Result<u64, String> {
        if self.provenance.get(tab_id) != Some(&NavigationProvenance::User) {
            return Err("agent download denied".into());
        }
        let source = admit_url(source)
            .map_err(str::to_owned)?
            .origin()
            .ascii_serialization();
        let file_name = admitted_file_name(file_name).ok_or("invalid download name")?;
        if self
            .entries
            .values()
            .filter(|entry| is_active(entry.state))
            .count()
            >= MAX_ACTIVE_DOWNLOADS
        {
            return Err("browser download limit reached".into());
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("browser download IDs exhausted")?;
        self.entries.insert(
            id,
            DownloadView {
                tab_id: tab_id.to_owned(),
                id,
                source,
                file_name,
                state: "choosingDestination",
                received_bytes: 0,
                total_bytes: None,
            },
        );
        while self.entries.len() > MAX_DOWNLOADS {
            let oldest_finished = self
                .entries
                .iter()
                .find(|(_, entry)| !is_active(entry.state))
                .map(|(id, _)| *id);
            if let Some(id) = oldest_finished {
                self.entries.remove(&id);
                self.destinations.remove(&id);
            } else {
                break;
            }
        }
        Ok(id)
    }

    pub(super) fn views(&self, tab_id: &str) -> Vec<DownloadView> {
        self.entries
            .values()
            .filter(|entry| entry.tab_id == tab_id)
            .cloned()
            .collect()
    }

    pub(super) fn record_destination(&mut self, id: u64, path: PathBuf) -> bool {
        if !path.is_absolute() || path.file_name().is_none() {
            return false;
        }
        if !self
            .entries
            .get(&id)
            .is_some_and(|entry| entry.state == "choosingDestination")
        {
            return false;
        }
        self.saved_as(id, &path);
        self.destinations.insert(id, path);
        true
    }

    /// Shows the name the file is (or was) actually saved under, which can
    /// differ from the server's suggestion (user rename, ` (1)` suffix).
    pub(super) fn saved_as(&mut self, id: u64, path: &std::path::Path) -> bool {
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(admitted_file_name)
        else {
            return false;
        };
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.file_name == name {
            return false;
        }
        entry.file_name = name;
        true
    }

    pub(super) fn destination_chosen(&mut self, id: u64) -> bool {
        if !self.destinations.contains_key(&id) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.state != "choosingDestination" {
            return false;
        }
        entry.state = "downloading";
        true
    }

    pub(super) fn folder_for_completed(&self, tab_id: &str, id: u64) -> Option<PathBuf> {
        let entry = self.entries.get(&id)?;
        if entry.tab_id != tab_id || entry.state != "completed" {
            return None;
        }
        self.destinations.get(&id)?.parent().map(PathBuf::from)
    }

    pub(super) fn progress(
        &mut self,
        id: u64,
        received_bytes: u64,
        total_bytes: Option<u64>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.state != "downloading" {
            return false;
        }
        entry.received_bytes = received_bytes;
        entry.total_bytes = total_bytes;
        true
    }

    pub(super) fn settle(&mut self, id: u64, state: &'static str) -> bool {
        if !matches!(state, "completed" | "interrupted" | "canceled" | "error") {
            return false;
        }
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if !is_active(entry.state) {
            return false;
        }
        entry.state = state;
        if state != "completed" {
            self.destinations.remove(&id);
        }
        true
    }

    pub(super) fn forget_tab(&mut self, tab_id: &str) {
        self.provenance.remove(tab_id);
        self.agent_generation.remove(tab_id);
        self.pending_user_navigation.remove(tab_id);
    }

    pub(super) fn cancel_tab(&mut self, tab_id: &str) -> Vec<u64> {
        self.provenance.remove(tab_id);
        self.agent_generation.remove(tab_id);
        self.pending_user_navigation.remove(tab_id);
        let ids = self
            .entries
            .iter_mut()
            .filter_map(|(id, entry)| {
                if entry.tab_id == tab_id && is_active(entry.state) {
                    entry.state = "canceled";
                    Some(*id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for id in &ids {
            self.destinations.remove(id);
        }
        ids
    }

    pub(super) fn cancel(&mut self, tab_id: &str, id: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.tab_id != tab_id || !is_active(entry.state) {
            return false;
        }
        entry.state = "canceled";
        self.destinations.remove(&id);
        true
    }
}

/// The row name: bidi override and format characters are dropped (as for
/// page titles) so a name cannot disguise its extension; then bounded.
fn admitted_file_name(name: &str) -> Option<String> {
    let name = name
        .chars()
        .filter(|character| {
            !matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect::<String>();
    (!name.is_empty()
        && name.len() <= 255
        && !name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':')))
    .then_some(name)
}

fn is_active(state: &str) -> bool {
    matches!(state, "choosingDestination" | "downloading")
}

impl BrowserState {
    pub(super) fn user_navigation_started(&self, tab_id: &str, id: u64, url: &str) {
        if let Ok(mut downloads) = self.downloads.lock() {
            downloads.start_user_navigation(tab_id, id, url);
        }
    }

    pub(super) fn user_navigation_finished(&self, tab_id: &str, id: u64, success: bool) {
        if let Ok(mut downloads) = self.downloads.lock() {
            downloads.finish_user_navigation(tab_id, id, success);
        }
    }
}

#[tauri::command]
pub(crate) async fn browser_downloads(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<Vec<DownloadView>, String> {
    if !state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .contains_key(&tab_id)
    {
        return Err("browser tab unavailable".into());
    }
    Ok(state
        .downloads
        .lock()
        .map_err(|error| error.to_string())?
        .views(&tab_id))
}

#[tauri::command]
pub(crate) async fn browser_cancel_download(
    app: AppHandle,
    state: State<'_, BrowserState>,
    tab_id: String,
    id: u64,
) -> Result<(), String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&tab_id)
        .map(|tab| tab.handle.clone())
        .ok_or("browser tab unavailable")?;
    let views = cancel_after_native(&state.downloads, &tab_id, id, || {
        #[cfg(windows)]
        cancel_native(&webview, vec![id])?;
        Ok(())
    })?;
    #[cfg(not(windows))]
    let _ = webview;
    let _ = app.emit_to(
        "main",
        "browser:downloads",
        serde_json::json!({
            "tabId": tab_id,
            "downloads": views,
        }),
    );
    Ok(())
}

fn cancel_after_native(
    downloads: &Mutex<DownloadRegistry>,
    tab_id: &str,
    id: u64,
    native: impl FnOnce() -> Result<(), String>,
) -> Result<Vec<DownloadView>, String> {
    let active = downloads
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&id)
        .is_some_and(|entry| entry.tab_id == tab_id && is_active(entry.state));
    if !active {
        return Err("browser download unavailable".into());
    }
    native()?;
    let mut registry = downloads.lock().map_err(|error| error.to_string())?;
    if !registry.cancel(tab_id, id) {
        return Err("browser download unavailable".into());
    }
    Ok(registry.views(tab_id))
}

#[cfg(test)]
mod tests;
